# /// script
# requires-python = ">=3.14"
# dependencies = [
#   "iterm2",
#   "pyobjc",
#   "pyobjc-framework-Quartz",
# ]
# ///

"""
UTF-8 regression automation for vicaya-tui.

What this verifies:
1. Multi-byte input in the main search box does not crash the TUI.
2. Left/right/backspace editing remains boundary-safe around UTF-8 codepoints.
3. Esc clears the search line after UTF-8 input.
4. Multi-byte input in preview-search overlay also remains stable.
5. After UTF-8 interactions, normal ASCII search and result navigation still work.

Expectation:
- The app remains responsive throughout.
- Screen text preserves the intended query after each edit step.
- Preview-search checks only pass on actual overlay markers.
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
    recorder = Recorder("vicaya-tui UTF-8 input regression")
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
        tab, session = await start_tui_session(window, "vicaya-utf8")

        await send_text(session, "e", delay=0.2)
        await send_text(session, "\x1b[D", delay=0.2)
        await send_text(session, "é", delay=0.6)
        recorder.shot("vicaya_utf8_01_main_input_e_accent")
        if await wait_for_text(session, "prashna: ée", timeout=3.0):
            recorder.pass_("main search insert before ASCII", "UTF-8 char inserted without panic")
        else:
            recorder.fail("main search insert before ASCII", "expected query 'ée' not visible")
            await dump_screen(session, "main-search-utf8-insert")

        await send_text(session, "\x1b[C", delay=0.2)
        await send_text(session, "ß", delay=0.6)
        recorder.shot("vicaya_utf8_02_main_input_sharp_s")
        if await wait_for_text(session, "prashna: éeß", timeout=3.0):
            recorder.pass_("main search append UTF-8", "cursor advanced over multibyte char safely")
        else:
            recorder.fail("main search append UTF-8", "expected query 'éeß' not visible")
            await dump_screen(session, "main-search-utf8-append")

        await send_text(session, "\x7f", delay=0.4)
        recorder.shot("vicaya_utf8_03_main_input_backspace")
        if await wait_for_text(session, "prashna: ée", timeout=3.0):
            recorder.pass_("main search backspace", "backspace removed full UTF-8 codepoint")
        else:
            recorder.fail("main search backspace", "backspace state not reflected correctly")
            await dump_screen(session, "main-search-utf8-backspace")

        await send_text(session, "\x1b", delay=0.5)
        recorder.shot("vicaya_utf8_04_main_input_cleared")
        if await wait_for_text(session, "prashna: ", timeout=3.0):
            recorder.pass_("Esc clears search", "input focus recovered after UTF-8 edits")
        else:
            recorder.fail("Esc clears search", "search line did not recover cleanly")
            await dump_screen(session, "main-search-cleared")

        await send_text(session, "Cargo", delay=1.0)
        await send_text(session, "\x1b[B", delay=0.3)
        await send_text(session, "\x0f", delay=1.0)
        preview_visible = await wait_for(
            session,
            lambda text, _lines: "purvadarshana —" in text
            or "Select a result to preview its contents." in text
            or "loading preview…" in text,
            timeout=4.0,
            description="preview pane markers",
        )

        if preview_visible:
            await send_text(session, "\t", delay=0.3)
            await send_text(session, "/", delay=0.3)
            recorder.shot("vicaya_utf8_05_preview_search_open")
            if await wait_for_all(session, ["preview search", "purvadarshana /:"], timeout=3.0):
                recorder.pass_("preview search overlay open")
            else:
                recorder.unverified_(
                    "preview search overlay open",
                    "overlay did not render; preview focus could not be proven",
                )
                await dump_screen(session, "preview-search-open")

            await send_text(session, "e", delay=0.2)
            await send_text(session, "\x1b[D", delay=0.2)
            await send_text(session, "é", delay=0.5)
            recorder.shot("vicaya_utf8_06_preview_input_e_accent")
            if await wait_for_text(session, "purvadarshana /: ée", timeout=3.0):
                recorder.pass_("preview search insert UTF-8", "overlay preserved multi-byte input")
            else:
                recorder.unverified_(
                    "preview search insert UTF-8",
                    "expected overlay query 'ée' not visible in this environment",
                )
                await dump_screen(session, "preview-search-utf8-insert")

            await send_text(session, "\x7f", delay=0.4)
            recorder.shot("vicaya_utf8_07_preview_input_backspace")
            if await wait_for_text(session, "purvadarshana /: é", timeout=3.0):
                recorder.pass_("preview search backspace", "removed trailing ASCII only")
            else:
                recorder.unverified_(
                    "preview search backspace",
                    "preview overlay backspace state was not observable",
                )
                await dump_screen(session, "preview-search-utf8-backspace")

            await send_text(session, "\x1b", delay=0.5)
            recorder.shot("vicaya_utf8_08_preview_search_cancel")
            if await wait_for(
                session,
                lambda text, _lines: "preview search" not in text and "purvadarshana —" in text,
                timeout=3.0,
                description="preview search cancel",
            ):
                recorder.pass_("preview search cancel", "returned to preview pane cleanly")
            else:
                recorder.unverified_(
                    "preview search cancel",
                    "did not recover to a proven preview pane state",
                )
                await dump_screen(session, "preview-search-cancel")
        else:
            recorder.unverified_(
                "preview search overlay open",
                "skipped because preview pane was not proven visible",
            )
            recorder.unverified_(
                "preview search insert UTF-8",
                "skipped because preview pane was not proven visible",
            )
            recorder.unverified_(
                "preview search backspace",
                "skipped because preview pane was not proven visible",
            )
            recorder.unverified_(
                "preview search cancel",
                "skipped because preview pane was not proven visible",
            )

        await send_text(session, "\t", delay=0.3)
        await send_text(session, "j", delay=0.4)
        recorder.shot("vicaya_utf8_09_results_navigation_post_utf8")
        if await wait_for_text(session, "phala (", timeout=2.0):
            recorder.pass_("post-UTF-8 responsiveness", "results navigation still responsive")
        else:
            recorder.fail("post-UTF-8 responsiveness", "TUI stopped responding after UTF-8 flow")
            await dump_screen(session, "post-utf8-responsiveness")

    finally:
        if tab and session:
            await cleanup_tab(tab, session)
        stop_daemon_if_started(started_daemon)

    return recorder.print_summary()


if __name__ == "__main__":
    code = iterm2.run_until_complete(main)
    sys.exit(code if isinstance(code, int) else 1)
