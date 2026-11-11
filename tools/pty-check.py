#!/usr/bin/env python3
"""Exercise real terminal behavior; no third-party Python modules required."""
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

BINARY = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tnmy").resolve())


def drain(master, timeout=0.2):
    data = bytearray()
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if select.select([master], [], [], min(0.03, max(0, end - time.monotonic())))[0]:
            try:
                chunk = os.read(master, 65536)
                if not chunk:
                    break
                data.extend(chunk)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
    return bytes(data)


def browser(root, width, keys, extra=(), expected_path=None, expected_code=0, environment=None, resize=False):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 25, width, 0, 0))
    before = termios.tcgetattr(slave)
    env = dict(os.environ, TERM="xterm-256color", HOME=str(root), XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
    for key in ("MYT_PALETTE", "MYT_THEME_DIR", "MYT_CONFIG", "TNMY_PALETTE"):
        env.pop(key, None)
    env["NO_COLOR"] = "1"
    if environment:
        env.update(environment)
    pick = "--pick" in extra or "--pick-directory" in extra
    child = subprocess.Popen([BINARY, "--color", "always", "files", str(root), *extra], stdin=slave, stdout=subprocess.PIPE if pick else slave, stderr=slave, env=env)
    try:
        output = drain(master, 0.4)
        assert b"\x1b[?1049h" in output, output
        assert b"38;2;" in output, ("explicit --color always did not override NO_COLOR", output)
        if width >= 35:
            assert b"Terminal New MY" in output, output
        assert drain(master, 0.2) == b"", "idle browser keeps redrawing"
        if resize:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 25, 110, 0, 0))
            os.kill(child.pid, signal.SIGWINCH)
            resized = drain(master, .3)
            assert b"Preview" in resized, resized
            output += resized
        for key in keys:
            os.write(master, key)
            output += drain(master, 0.1)
        child.wait(timeout=5)
        output += drain(master)
        assert child.returncode == expected_code, output
        if pick:
            chosen = child.stdout.read()
            assert chosen == (os.fsencode(expected_path) + b"\0" if expected_path is not None else b""), chosen
        assert b"\x1b[?1049l" in output, "alternate screen not restored"
        assert termios.tcgetattr(slave) == before, "terminal flags not restored"
        assert b"panicked" not in output
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        os.close(master)
        os.close(slave)


def fish_directory_picker(root):
    fish = shutil.which("fish")
    if fish is None:
        print("fish picker check skipped here; Nix check supplies fish")
        return
    root = root / "space and\nline '\""
    root.mkdir()
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 25, 110, 0, 0))
    before = termios.tcgetattr(slave)
    env = dict(os.environ, TERM="xterm-256color", TNMY_DENSITY="compact", TNMY_PICK_ROOT=str(root),
               TNMY_FISH=str(Path(__file__).resolve().parent.parent / "integrations/fish/tnmy.fish"),
               PATH=str(Path(BINARY).parent) + os.pathsep + os.environ.get("PATH", ""))
    script = 'source "$TNMY_FISH"; mycd --no-git "$TNMY_PICK_ROOT"; printf \'%s\\0\' "$PWD"'
    child = subprocess.Popen([fish, "--no-config", "--interactive", "-c", script], stdin=slave, stdout=subprocess.PIPE, stderr=slave, env=env)
    try:
        drain(master, .4)
        os.write(master, b"c")
        drain(master, .2)
        child.wait(timeout=5)
        assert child.returncode == 0
        selected = child.stdout.read()
        assert selected == os.fsencode(root) + b"\0", selected
        assert termios.tcgetattr(slave) == before
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        os.close(master)
        os.close(slave)


with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    (root / "folder").mkdir()
    (root / "hello.txt").write_text("Material You preview\nПривет\n")
    (root / ".hidden").write_text("hidden")
    os.mkfifo(root / "fifo")
    browser(root, 110, [b"\r", b"\x1b[D", b"/hello", b"\r", b"r", b"a", b"s", b"q"])
    browser(root, 35, [b"/nothing-matches", b"\r", b"q"])
    browser(root, 35, [b"c"], extra=("--pick-directory", "--no-git"), expected_path=root, resize=True)
    browser(root, 10, [b"\x03"])
    browser(root, 110, [b"/fifo", b"\r", b"q"])
    browser(root, 110, [b"/hello", b"\r", b"\x1b[6~", b"\t", b"j", b"d", b"q"])
    browser(root, 110, [b"c"], extra=("--pick-directory", "--no-git"), expected_path=root)
    browser(root, 110, [b"/hello", b"\r", b"\r"], extra=("--pick", "--no-git"), expected_path=root / "hello.txt")
    browser(root, 110, [b"q"], extra=("--pick-directory",), expected_code=130)
    fish_directory_picker(root)
    large = root / "large"
    large.mkdir()
    for index in range(10000):
        (large / f"file-{index:05}.txt").touch()
    started = time.monotonic()
    browser(large, 110, [b"/09999", b"\r", b"q"], extra=("--no-git",))
    elapsed = time.monotonic() - started
    assert elapsed < 5, "10,000-entry browser exceeded interaction budget"
    print(f"10,000-entry interaction sequence: {elapsed:.3f}s (includes intentional PTY waits)")


    # A fake child receives an interrupt just as nh would. No rebuild is run.
    shell = os.environ.get("TNMY_TEST_SH", "/bin/sh")
    fake = root / "nh"
    fake.write_text(f"#!{shell}\nkill -INT $$\n")
    fake.chmod(0o755)
    env = dict(os.environ, PATH=str(root))
    for key in ("MYT_PALETTE", "MYT_THEME_DIR", "MYT_CONFIG", "TNMY_PALETTE"):
        env.pop(key, None)
    result = subprocess.run([BINARY, "update", "--action", "build"], env=env, capture_output=True, timeout=5)
    assert result.returncode == 130, result
    assert b"exit 130" in result.stderr

print("PTY checks passed: 10,000 entries, picking, preview scrolling, density, navigation, filtering, narrow layouts, FIFO, idle, Ctrl+C, restoration, child SIGINT")
