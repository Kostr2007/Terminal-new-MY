#!/usr/bin/env python3
"""Exercise real terminal behavior; no third-party Python modules required."""
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
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


def browser(root, width, keys):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 25, width, 0, 0))
    before = termios.tcgetattr(slave)
    env = dict(os.environ, TERM="xterm-256color", HOME=str(root), XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
    for key in ("MYT_PALETTE", "MYT_THEME_DIR", "MYT_CONFIG", "TNMY_PALETTE"):
        env.pop(key, None)
    env["NO_COLOR"] = "1"
    child = subprocess.Popen([BINARY, "--color", "always", "files", str(root)], stdin=slave, stdout=slave, stderr=slave, env=env)
    try:
        output = drain(master, 0.4)
        assert b"\x1b[?1049h" in output, output
        assert b"38;2;" in output, "explicit --color always did not override NO_COLOR"
        if width >= 35:
            assert b"Terminal New MY" in output, output
        assert drain(master, 0.2) == b"", "idle browser keeps redrawing"
        for key in keys:
            os.write(master, key)
            output += drain(master, 0.1)
        child.wait(timeout=5)
        output += drain(master)
        assert child.returncode == 0, output
        assert b"\x1b[?1049l" in output, "alternate screen not restored"
        assert termios.tcgetattr(slave) == before, "terminal flags not restored"
        assert b"panicked" not in output
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
    browser(root, 10, [b"\x03"])
    browser(root, 110, [b"/fifo", b"\r", b"q"])

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

print("PTY checks passed: navigation, filtering, narrow layouts, FIFO, idle, Ctrl+C, restoration, child SIGINT")
