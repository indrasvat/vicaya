# /// script
# requires-python = ">=3.14"
# dependencies = [
#   "iterm2",
#   "pyobjc",
#   "pyobjc-framework-Quartz",
# ]
# ///

"""
Startup-scope automation for vicaya-tui.

What this verifies:
1. `vicaya-tui .` starts with repo ksetra already applied.
2. `vicaya-tui <dir>` starts scoped for github.com, Documents, Desktop, and Downloads.
3. Scoped searches remain visually tied to the requested directory.
4. Help and action-palette overlays still work under startup scope.
5. Directory navigation still works when startup ksetra is already active.

Expectation:
- Startup scope is visible in the header before any manual Ctrl+K interaction.
- Screenshots are captured for every scope and important overlay.
- Queries and fixture selection are discovered at runtime without checking sensitive names into git.
"""

from __future__ import annotations

import sys
from pathlib import Path

import iterm2

from vicaya_iterm2_utils import (
    PROJECT_ROOT,
    Recorder,
    cleanup_tab,
    discover_fixture,
    dump_screen,
    ensure_release_binaries,
    get_screen_text,
    send_text,
    start_daemon_if_needed,
    start_tui_session,
    stop_daemon_if_started,
    wait_for,
)

HOME = Path.home()
GITHUB_ROOT = HOME / "code" / "github.com"
DOCUMENTS_ROOT = HOME / "Documents"
DESKTOP_ROOT = HOME / "Desktop"
DOWNLOADS_ROOT = HOME / "Downloads"


def search_term_for(path: Path) -> str:
    stem = path.stem
    if len(stem) >= 5:
        return stem[:48]
    return path.name[:48]


async def verify_startup_scope(session, recorder: Recorder, case_id: str, scope_path: Path) -> None:
    recorder.shot(f"{case_id}_startup")
    scope_marker = scope_path.name or str(scope_path)
    if await wait_for(
        session,
        lambda text, _lines: "ksetra:" in text and scope_marker in text,
        timeout=4.0,
        description=f"startup scope {scope_marker}",
    ):
        recorder.pass_(f"{case_id} startup scope", f"ksetra shows {scope_marker}")
    else:
        recorder.fail(f"{case_id} startup scope", f"missing ksetra marker for {scope_marker}")
        await dump_screen(session, f"{case_id}-startup")


async def search_and_verify(
    session,
    recorder: Recorder,
    *,
    case_id: str,
    query: str,
    expected_marker: str,
    forbidden_markers: list[str] | None = None,
) -> None:
    await send_text(session, "\x1b", delay=0.2)
    await send_text(session, "\x1b", delay=0.2)
    await send_text(session, query, delay=0.5)
    settled = await wait_for(
        session,
        lambda text, _lines: expected_marker in text and "searching" not in text.lower(),
        timeout=8.0,
        description=f"query {query!r}",
    )
    recorder.shot(f"{case_id}_query")
    screen_text = await get_screen_text(session)
    if settled:
        bad = [marker for marker in forbidden_markers or [] if marker in screen_text]
        if bad:
            recorder.fail(
                f"{case_id} scoped results",
                f"unexpected out-of-scope markers visible: {bad}",
            )
            await dump_screen(session, f"{case_id}-forbidden")
        else:
            recorder.pass_(f"{case_id} scoped results", f"query {query!r} rendered")
    else:
        recorder.fail(f"{case_id} scoped results", f"missing marker {expected_marker!r}")
        await dump_screen(session, f"{case_id}-query")


async def run_repo_case(window, recorder: Recorder) -> None:
    tab = session = None
    try:
        tab, session = await start_tui_session(
            window,
            "vicaya-startup-repo",
            cwd=PROJECT_ROOT,
            args=["."],
        )
        await verify_startup_scope(session, recorder, "startup_scope_repo", PROJECT_ROOT)
        await search_and_verify(
            session,
            recorder,
            case_id="startup_scope_repo",
            query="CLAUDE.md",
            expected_marker="CLAUDE.md",
            forbidden_markers=["/Documents/", "/Downloads/"],
        )
    finally:
        if tab and session:
            await cleanup_tab(tab, session)


async def run_github_case(window, recorder: Recorder) -> None:
    tab = session = None
    try:
        tab, session = await start_tui_session(
            window,
            "vicaya-startup-github",
            cwd=PROJECT_ROOT,
            args=[str(GITHUB_ROOT)],
        )
        await verify_startup_scope(session, recorder, "startup_scope_github", GITHUB_ROOT)
        await search_and_verify(
            session,
            recorder,
            case_id="startup_scope_github",
            query="CLAUDE.md",
            expected_marker="CLAUDE.md",
            forbidden_markers=["/Documents/", "/Downloads/"],
        )

        await send_text(session, "\x1b[B", delay=0.3)
        await send_text(session, "?", delay=0.4)
        recorder.shot("startup_scope_github_help")
        if await wait_for(
            session,
            lambda text, _lines: "drishti / ksetra quick help" in text and "Ctrl+K" in text,
            timeout=3.0,
            description="help overlay",
        ):
            recorder.pass_("github help overlay")
        else:
            recorder.fail("github help overlay", "missing help markers")
            await dump_screen(session, "github-help")
        await send_text(session, "\x1b", delay=0.3)

        await send_text(session, "\x10", delay=0.4)
        recorder.shot("startup_scope_github_palette")
        if await wait_for(
            session,
            lambda text, _lines: "kriya:" in text.lower() and "choose" in text.lower(),
            timeout=3.0,
            description="action palette",
        ):
            recorder.pass_("github action palette")
        else:
            recorder.fail("github action palette", "missing action-palette markers")
            await dump_screen(session, "github-palette")
    finally:
        if tab and session:
            await cleanup_tab(tab, session)


async def run_runtime_fixture_case(
    window,
    recorder: Recorder,
    *,
    case_id: str,
    scope_root: Path,
    preferred_names: list[str],
    allowed_suffixes: tuple[str, ...],
) -> None:
    fixture = discover_fixture(
        scope_root,
        preferred_names=preferred_names,
        allowed_suffixes=allowed_suffixes,
    )
    if fixture is None:
        recorder.unverified_(case_id, f"no suitable fixture found under {scope_root}")
        return

    print(f"{case_id} fixture: {fixture}")
    query = search_term_for(fixture)

    tab = session = None
    try:
        tab, session = await start_tui_session(
            window,
            f"vicaya-{case_id}",
            cwd=PROJECT_ROOT,
            args=[str(scope_root)],
        )
        await verify_startup_scope(session, recorder, case_id, scope_root)
        await search_and_verify(
            session,
            recorder,
            case_id=case_id,
            query=query,
            expected_marker=fixture.name,
        )

        if case_id == "startup_scope_desktop":
            await send_text(session, "\x14", delay=0.4)
            await send_text(session, "Sthana", delay=0.3)
            await send_text(session, "\r", delay=0.7)
            recorder.shot("startup_scope_desktop_sthana")
            if await wait_for(
                session,
                lambda text, _lines: "drishti: Sthana (Directories)" in text,
                timeout=3.0,
                description="desktop sthana",
            ):
                recorder.pass_("desktop drishti switch")
            else:
                recorder.fail("desktop drishti switch", "failed to enter Sthana view")
                await dump_screen(session, "desktop-sthana")
    finally:
        if tab and session:
            await cleanup_tab(tab, session)


async def main(connection) -> int:
    recorder = Recorder("vicaya-tui startup scope test")
    ensure_release_binaries()
    started_daemon = start_daemon_if_needed()

    app = await iterm2.async_get_app(connection)
    window = app.current_terminal_window
    if not window:
        print("No active iTerm2 window")
        return 1

    try:
        await run_repo_case(window, recorder)
        await run_github_case(window, recorder)
        await run_runtime_fixture_case(
            window,
            recorder,
            case_id="startup_scope_documents",
            scope_root=DOCUMENTS_ROOT,
            preferred_names=["Hack-Regular.ttf", "sample-report.pdf"],
            allowed_suffixes=(".pdf", ".txt", ".ttf"),
        )
        await run_runtime_fixture_case(
            window,
            recorder,
            case_id="startup_scope_desktop",
            scope_root=DESKTOP_ROOT,
            preferred_names=["example-note.txt", "Screenshot.png"],
            allowed_suffixes=(".png", ".txt"),
        )
        await run_runtime_fixture_case(
            window,
            recorder,
            case_id="startup_scope_downloads",
            scope_root=DOWNLOADS_ROOT,
            preferred_names=["verification_report.txt", "energy_analysis.html"],
            allowed_suffixes=(".html", ".txt", ".png"),
        )
    finally:
        stop_daemon_if_started(started_daemon)

    return recorder.print_summary()


if __name__ == "__main__":
    code = iterm2.run_until_complete(main)
    sys.exit(code if isinstance(code, int) else 1)
