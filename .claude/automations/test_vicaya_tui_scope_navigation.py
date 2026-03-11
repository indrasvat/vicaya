# /// script
# requires-python = ">=3.14"
# dependencies = [
#   "iterm2",
#   "pyobjc",
#   "pyobjc-framework-Quartz",
# ]
# ///

"""
Ksetra and navigation automation for vicaya-tui.

What this verifies:
1. Ctrl+K opens the direct-path overlay with path prompt.
2. Tab completion renders a completions list.
3. A second Tab applies the chosen completion, then Enter applies the ksetra and header breadcrumbs update.
4. In Sthana view, h/l push and pop the scope stack.
5. Ctrl+G changes grouping mode and results remain usable.

Expectation:
- The scope stack is keyboard-driven and visually obvious in the header.
- Overlay rendering remains intact while moving through completions.
"""

from __future__ import annotations

import sys

import iterm2

from vicaya_iterm2_utils import (
    Recorder,
    cleanup_tab,
    dump_screen,
    ensure_release_binaries,
    send_text,
    start_daemon_if_needed,
    start_tui_session,
    stop_daemon_if_started,
    wait_for,
    wait_for_all,
    wait_for_text,
)


async def main(connection) -> int:
    recorder = Recorder("vicaya-tui scope navigation test")
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
        tab, session = await start_tui_session(window, "vicaya-scope-nav")

        await send_text(session, "\x0b", delay=0.5)
        recorder.shot("vicaya_scope_01_ctrl_k_open")
        if await wait_for_all(session, ["path:", "Tab: complete", "Enter: set"], timeout=3.0):
            recorder.pass_("Ctrl+K overlay", "path prompt and hints visible")
        else:
            recorder.fail("Ctrl+K overlay", "missing path overlay markers")
            await dump_screen(session, "ctrl-k-open")

        await send_text(session, "~/code/github.com/indrasvat-vicaya/cr", delay=0.3)
        recorder.shot("vicaya_scope_02_partial_path")
        await send_text(session, "\t", delay=0.8)
        recorder.shot("vicaya_scope_03_completion_list")
        if await wait_for_text(session, "completions", timeout=3.0):
            recorder.pass_("ksetra completion list", "completion list rendered")
        else:
            recorder.fail("ksetra completion list", "completion list did not render")
            await dump_screen(session, "ksetra-completions")

        await send_text(session, "\t", delay=0.6)
        await send_text(session, "\r", delay=0.8)
        recorder.shot("vicaya_scope_04_ksetra_applied")
        if await wait_for(
            session,
            lambda text, _lines: "ksetra:" in text and "crates" in text and "Not a directory" not in text,
            timeout=4.0,
            description="ksetra applied",
        ):
            recorder.pass_("ksetra apply", "header breadcrumbs updated to crates")
        else:
            recorder.fail("ksetra apply", "header did not update after applying ksetra")
            await dump_screen(session, "ksetra-applied")

        await send_text(session, "\x14", delay=0.4)
        await send_text(session, "Sthana", delay=0.3)
        await send_text(session, "\r", delay=0.8)
        await send_text(session, "vicaya-cli", delay=1.0)
        recorder.shot("vicaya_scope_05_sthana_results")
        if await wait_for_text(session, "vicaya-cli/ (", timeout=4.0):
            recorder.pass_("Sthana scoped search", "directory result visible inside crates scope")
        else:
            recorder.fail("Sthana scoped search", "expected vicaya-cli directory missing")
            await dump_screen(session, "sthana-scoped-results")

        await send_text(session, "\x1b[B", delay=0.4)
        await send_text(session, "l", delay=0.8)
        recorder.shot("vicaya_scope_06_push_ksetra")
        if await wait_for(
            session,
            lambda text, _lines: "ksetra:" in text and "vicaya-cli" in text,
            timeout=4.0,
            description="push ksetra",
        ):
            recorder.pass_("push ksetra with l", "header includes nested scope")
        else:
            recorder.fail("push ksetra with l", "nested scope not visible in header")
            await dump_screen(session, "push-ksetra")

        await send_text(session, "h", delay=0.8)
        recorder.shot("vicaya_scope_07_pop_ksetra")
        if await wait_for(
            session,
            # Limit the "vicaya-cli" check to the first 120 chars after "ksetra:" so
            # results-list matches do not masquerade as header breadcrumbs.
            lambda text, _lines: "ksetra:" in text
            and "crates" in text
            and "vicaya-cli" not in text.split("ksetra:", 1)[-1][:120],
            timeout=4.0,
            description="scope pop",
        ):
            recorder.pass_("pop ksetra with h", "header returned to parent scope")
        else:
            recorder.fail("pop ksetra with h", "header did not return to parent scope")
            await dump_screen(session, "pop-ksetra")

        await send_text(session, "\x07", delay=0.5)
        recorder.shot("vicaya_scope_08_group_dir")
        if await wait_for_text(session, "varga:dir", timeout=3.0):
            recorder.pass_("grouping to dir", "results title updated")
        else:
            recorder.fail("grouping to dir", "dir grouping label not visible")
            await dump_screen(session, "grouping-dir")

        await send_text(session, "\x07", delay=0.5)
        recorder.shot("vicaya_scope_09_group_ext")
        if await wait_for_text(session, "varga:ext", timeout=3.0):
            recorder.pass_("grouping to ext", "results title updated again")
        else:
            recorder.fail("grouping to ext", "ext grouping label not visible")
            await dump_screen(session, "grouping-ext")

    finally:
        if tab and session:
            await cleanup_tab(tab, session)
        stop_daemon_if_started(started_daemon)

    return recorder.print_summary()


if __name__ == "__main__":
    code = iterm2.run_until_complete(main)
    sys.exit(code if isinstance(code, int) else 1)
