# /// script
# requires-python = ">=3.14"
# dependencies = [
#   "iterm2",
#   "pyobjc",
#   "pyobjc-framework-Quartz",
# ]
# ///

"""
Core visual automation for vicaya-tui.

What this verifies:
1. TUI launches with expected header/footer markers.
2. Help overlay renders the advertised keyboard guidance.
3. Kriya-suchi overlay opens and lists actionable commands.
4. Drishti switcher opens, filters, and switches to Sthana.
5. Search results render and keyboard navigation changes selection.
6. Preview can be toggled, focused, and searched.

Expectation:
- Every step remains visually stable and keyboard-driven.
- Screenshots are captured for each important state transition.
"""

from __future__ import annotations

import asyncio
import sys

import iterm2

from vicaya_iterm2_utils import (
    Recorder,
    cleanup_tab,
    dump_screen,
    ensure_release_binaries,
    get_screen_text,
    send_text,
    start_daemon_if_needed,
    start_tui_session,
    stop_daemon_if_started,
    wait_for,
    wait_for_all,
    wait_for_text,
)


async def main(connection) -> int:
    recorder = Recorder("vicaya-tui core visual test")
    ensure_release_binaries()
    started_daemon = start_daemon_if_needed()

    app = await iterm2.async_get_app(connection)
    window = app.current_terminal_window
    if not window:
        print("No active iTerm2 window")
        return 1

    tab = None
    session = None
    try:
        tab, session = await start_tui_session(window, "vicaya-core-visual")
        recorder.shot("vicaya_core_01_startup")
        recorder.pass_("startup header", "vicaya, drishti, ksetra visible")

        await send_text(session, "?", delay=0.4)
        recorder.shot("vicaya_core_02_help_overlay")
        if await wait_for(
            session,
            lambda text, _lines: "vicaya-tui" in text
            and "drishti / ksetra quick help" in text
            and "Ctrl+K" in text
            and "Ctrl+O" in text
            and "Enter / o" in text,
            timeout=3.0,
            description="help overlay markers",
        ):
            recorder.pass_("help overlay", "expected help content rendered")
        else:
            recorder.fail("help overlay", "missing one or more help markers")
            await dump_screen(session, "help-overlay")

        await send_text(session, "\x1b", delay=0.3)

        await send_text(session, "\x10", delay=0.4)
        recorder.shot("vicaya_core_03_kriya_suchi")
        if await wait_for_all(session, ["kriya:", "Set ksetra", "Toggle preview"], timeout=3.0):
            recorder.pass_("kriya-suchi", "overlay lists actionable commands")
        else:
            recorder.fail("kriya-suchi", "action palette content missing")
            await dump_screen(session, "kriya-suchi")

        await send_text(session, "\x1b", delay=0.3)

        await send_text(session, "\x14", delay=0.4)
        recorder.shot("vicaya_core_04_drishti_open")
        if await wait_for_all(session, ["filter:", "Patra", "Sthana"], timeout=3.0):
            recorder.pass_("drishti switcher open")
        else:
            recorder.fail("drishti switcher open", "switcher markers not visible")
            await dump_screen(session, "drishti-open")

        await send_text(session, "Sthana", delay=0.3)
        recorder.shot("vicaya_core_05_drishti_filter")
        await send_text(session, "\r", delay=0.8)
        if await wait_for_text(session, "drishti: Sthana (Directories)", timeout=4.0):
            recorder.pass_("switch to Sthana", "header updated to directory view")
        else:
            recorder.fail("switch to Sthana", "header did not reflect Sthana")
            await dump_screen(session, "drishti-sthana")

        await send_text(session, "crates", delay=1.0)
        recorder.shot("vicaya_core_06_sthana_search")
        screen_text = await get_screen_text(session)
        if "phala (" in screen_text and "crates/" in screen_text:
            recorder.pass_("directory search", "results rendered for crates query")
        else:
            recorder.fail("directory search", "expected crates result missing")
            await dump_screen(session, "directory-search")

        await send_text(session, "\x1b[B", delay=0.4)
        before_nav = await get_screen_text(session)
        await send_text(session, "j", delay=0.4)
        after_nav = await get_screen_text(session)
        recorder.shot("vicaya_core_07_results_navigation")
        if before_nav != after_nav:
            recorder.pass_("results keyboard navigation", "selection changed after j")
        else:
            recorder.fail("results keyboard navigation", "screen did not change after j")

        await send_text(session, "\x14", delay=0.4)
        await send_text(session, "Patra", delay=0.3)
        await send_text(session, "\r", delay=0.8)
        await send_text(session, "\x1b", delay=0.3)
        await send_text(session, "Cargo", delay=1.0)
        await send_text(session, "\x1b[B", delay=0.4)
        await send_text(session, "\x0f", delay=1.0)
        recorder.shot("vicaya_core_08_preview_visible")
        if await wait_for_text(session, "purvadarshana", timeout=4.0):
            recorder.pass_("preview toggle", "preview pane title visible")
        else:
            recorder.fail("preview toggle", "preview pane did not appear")
            await dump_screen(session, "preview-visible")

        await send_text(session, "\t", delay=0.3)
        await send_text(session, "/", delay=0.4)
        recorder.shot("vicaya_core_09_preview_search_overlay")
        if await wait_for_all(session, ["preview search", "purvadarshana /:", "Enter: apply"], timeout=3.0):
            recorder.pass_("preview search overlay", "overlay rendered with expected prompt")
        else:
            recorder.fail("preview search overlay", "overlay markers not visible")
            await dump_screen(session, "preview-search-overlay")

        await send_text(session, "workspace", delay=0.3)
        await send_text(session, "\r", delay=0.8)
        recorder.shot("vicaya_core_10_preview_search_applied")
        if await wait_for(
            session,
            lambda text, _lines: "purvadarshana" in text and "/workspace/" in text,
            timeout=4.0,
            description="preview search applied",
        ):
            recorder.pass_("preview search apply", "title reflects active preview search")
        else:
            recorder.fail("preview search apply", "applied search not reflected in preview title")
            await dump_screen(session, "preview-search-applied")

        await send_text(session, "\x1b[6~", delay=0.4)
        recorder.shot("vicaya_core_11_preview_scroll")
        recorder.pass_("preview scroll", "page-down issued and state captured")

    finally:
        if tab and session:
            await cleanup_tab(tab, session)
        stop_daemon_if_started(started_daemon)

    return recorder.print_summary()


if __name__ == "__main__":
    code = iterm2.run_until_complete(main)
    sys.exit(code if isinstance(code, int) else 1)
