# /// script
# requires-python = ">=3.14"
# dependencies = [
#   "iterm2",
#   "pyobjc",
#   "pyobjc-framework-Quartz",
# ]
# ///

"""Shared helpers for defensive iTerm2 automation around vicaya-tui."""

from __future__ import annotations

import asyncio
import os
import subprocess
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

import Quartz
import iterm2

PROJECT_ROOT = Path(__file__).resolve().parents[2]
SCREENSHOT_DIR = PROJECT_ROOT / ".claude" / "screenshots"
CLI_BINARY = PROJECT_ROOT / "target" / "release" / "vicaya"
DAEMON_BINARY = PROJECT_ROOT / "target" / "release" / "vicaya-daemon"
TUI_BINARY = PROJECT_ROOT / "target" / "release" / "vicaya-tui"
BUILD_CMD = [
    "cargo",
    "build",
    "--release",
    "-p",
    "vicaya-cli",
    "-p",
    "vicaya-daemon",
    "-p",
    "vicaya-tui",
]

CONFLICTING_PROCESS_PATTERNS = [
    "vicaya-daemon",
    "vicaya-tui",
]
PROCESS_CLEANUP_SETTLE_SECONDS = 0.8


def get_iterm2_window_id() -> int | None:
    """Return the active iTerm2 CGWindowID if available."""
    windows = Quartz.CGWindowListCopyWindowInfo(
        Quartz.kCGWindowListOptionOnScreenOnly | Quartz.kCGWindowListExcludeDesktopElements,
        Quartz.kCGNullWindowID,
    )
    for window in windows:
        owner = window.get("kCGWindowOwnerName", "")
        if "iTerm" in owner:
            return window.get("kCGWindowNumber")
    return None


def capture_screenshot(name: str) -> str:
    """Capture an iTerm2-window screenshot into .claude/screenshots."""
    SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    path = SCREENSHOT_DIR / f"{name}_{timestamp}.png"

    window_id = get_iterm2_window_id()
    if window_id:
        subprocess.run(["screencapture", "-x", "-l", str(window_id), str(path)], check=True)
    else:
        subprocess.run(["screencapture", "-x", str(path)], check=True)

    return str(path)


def ensure_release_binaries() -> None:
    """Always build release binaries so automation exercises the current branch."""
    print("Building release binaries for automation...")
    subprocess.run(BUILD_CMD, cwd=PROJECT_ROOT, check=True)


def stop_conflicting_vicaya_processes() -> None:
    """Stop resident vicaya processes so automation uses this branch's binaries only."""
    if CLI_BINARY.exists():
        subprocess.run(
            [str(CLI_BINARY), "daemon", "stop"],
            cwd=PROJECT_ROOT,
            check=False,
            capture_output=True,
            text=True,
        )

    for pattern in CONFLICTING_PROCESS_PATTERNS:
        subprocess.run(["pkill", "-f", pattern], cwd=PROJECT_ROOT, check=False)

    time.sleep(PROCESS_CLEANUP_SETTLE_SECONDS)


def daemon_running() -> bool:
    """Best-effort daemon state check via CLI status output."""
    if not CLI_BINARY.exists():
        return False

    result = subprocess.run(
        [str(CLI_BINARY), "daemon", "status"],
        cwd=PROJECT_ROOT,
        capture_output=True,
        text=True,
    )
    combined = f"{result.stdout}\n{result.stderr}"
    return "Daemon is running" in combined


def start_daemon_if_needed() -> bool:
    """Restart the daemon from this branch's binary for a clean automation session."""
    stop_conflicting_vicaya_processes()
    print("Starting vicaya daemon for automation...")
    subprocess.run([str(CLI_BINARY), "daemon", "start"], cwd=PROJECT_ROOT, check=True)
    time.sleep(1.5)
    return True


def stop_daemon_if_started(started_here: bool) -> None:
    """Stop the daemon if this automation session started it."""
    if not started_here:
        return
    subprocess.run([str(CLI_BINARY), "daemon", "stop"], cwd=PROJECT_ROOT, check=False)


async def send_text(session, text: str, delay: float = 0.3) -> None:
    await session.async_send_text(text)
    await asyncio.sleep(delay)


async def get_screen_lines(session) -> list[str]:
    screen = await session.async_get_screen_contents()
    return [screen.line(i).string for i in range(screen.number_of_lines)]


async def get_screen_text(session) -> str:
    return "\n".join(await get_screen_lines(session))


async def dump_screen(session, label: str, max_lines: int = 80) -> None:
    lines = await get_screen_lines(session)
    print(f"\n--- SCREEN DUMP: {label} ---")
    for idx, line in enumerate(lines[:max_lines]):
        if line.strip():
            print(f"{idx:02d}: {line}")
    print("--- END SCREEN DUMP ---\n")


async def wait_for(
    session,
    predicate,
    timeout: float = 10.0,
    interval: float = 0.25,
    description: str = "condition",
) -> bool:
    """Poll screen contents until predicate(text, lines) returns truthy."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        lines = await get_screen_lines(session)
        text = "\n".join(lines)
        if predicate(text, lines):
            return True
        await asyncio.sleep(interval)
    print(f"Timed out waiting for {description}")
    return False


async def wait_for_text(session, needle: str, timeout: float = 10.0) -> bool:
    return await wait_for(
        session,
        lambda text, _lines: needle in text,
        timeout=timeout,
        description=f"text {needle!r}",
    )


async def wait_for_all(session, needles: list[str], timeout: float = 10.0) -> bool:
    return await wait_for(
        session,
        lambda text, _lines: all(needle in text for needle in needles),
        timeout=timeout,
        description=f"all markers {needles!r}",
    )


async def start_tui_session(window, tab_name: str):
    """Open a fresh tab, launch vicaya-tui, and wait for stable header text."""
    tab = await window.async_create_tab()
    session = tab.current_session
    await session.async_set_name(tab_name)
    # Clear tty discard before launch so control-key automation has a chance to reach the TUI.
    await send_text(
        session,
        f"cd {PROJECT_ROOT} && stty discard undef 2>/dev/null; {TUI_BINARY}\n",
        delay=1.8,
    )

    ready = await wait_for_all(session, ["vicaya", "drishti:", "ksetra:"], timeout=15.0)
    if not ready:
        await dump_screen(session, "tui-launch-failure")
        raise RuntimeError("vicaya-tui did not render expected header markers")
    return tab, session


async def cleanup_tab(tab, session) -> None:
    """Best-effort TUI shutdown followed by tab close."""
    try:
        await session.async_send_text("q")
        await asyncio.sleep(0.2)
    except Exception:
        pass

    try:
        await session.async_send_text("\x03")
        await asyncio.sleep(0.2)
    except Exception:
        pass

    try:
        await session.async_send_text("exit\n")
        await asyncio.sleep(0.2)
    except Exception:
        pass

    for tab_session in tab.sessions:
        try:
            await tab_session.async_close()
        except Exception:
            pass


@dataclass
class Recorder:
    """Collect pass/fail state and screenshots consistently."""

    name: str
    passed: int = 0
    failed: int = 0
    unverified: int = 0
    screenshots: list[str] = field(default_factory=list)
    failures: list[str] = field(default_factory=list)
    started_at: datetime = field(default_factory=datetime.now)

    def shot(self, name: str) -> str:
        screenshot = capture_screenshot(name)
        self.screenshots.append(screenshot)
        print(f"  screenshot: {screenshot}")
        return screenshot

    def pass_(self, label: str, details: str = "") -> None:
        self.passed += 1
        print(f"  PASS: {label}" + (f" - {details}" if details else ""))

    def fail(self, label: str, details: str = "") -> None:
        self.failed += 1
        entry = f"{label}: {details}" if details else label
        self.failures.append(entry)
        print(f"  FAIL: {entry}")

    def unverified_(self, label: str, details: str = "") -> None:
        self.unverified += 1
        print(f"  UNVERIFIED: {label}" + (f" - {details}" if details else ""))

    def exit_code(self) -> int:
        return 1 if self.failed else 0

    def print_summary(self) -> int:
        duration = (datetime.now() - self.started_at).total_seconds()
        total = self.passed + self.failed + self.unverified
        print("\n" + "=" * 72)
        print(self.name)
        print("=" * 72)
        print(f"Duration:   {duration:.1f}s")
        print(f"Total:      {total}")
        print(f"Passed:     {self.passed}")
        print(f"Failed:     {self.failed}")
        print(f"Unverified: {self.unverified}")
        print(f"Screenshots:{len(self.screenshots)}")
        if self.failures:
            print("\nFailures:")
            for failure in self.failures:
                print(f"  - {failure}")
        print("=" * 72)
        return self.exit_code()
