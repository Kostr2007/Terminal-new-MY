#!/usr/bin/env python3
"""Exercise real terminal behavior; no third-party Python modules required."""
import base64
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
            assert b"Contents" in output, output
        assert drain(master, 0.2) == b"", "idle browser keeps redrawing"
        if resize:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 25, 110, 0, 0))
            os.kill(child.pid, signal.SIGWINCH)
            resized = drain(master, .3)
            assert b"Preview" in resized, resized
            output += resized
        for key in keys:
            if key is None:
                output += drain(master, 0.7)
                assert drain(master, 0.2) == b"", "selection keeps redrawing after settling"
            else:
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
        return output
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

with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    (root / "folder").mkdir()
    (root / "folder" / "example.txt").touch()
    (root / "link").symlink_to("folder", target_is_directory=True)
    env = dict(os.environ, TERM="xterm-kitty", KITTY_WINDOW_ID="1")
    for key in ("MYT_PALETTE", "MYT_THEME_DIR", "MYT_CONFIG", "TNMY_PALETTE", "TNMY_GRAPHICS",
                "TMUX", "STY", "NO_COLOR", "MYT_NO_ANIMATION", "TNMY_REDUCED_MOTION"):
        env.pop(key, None)
    env.update(HOME=str(root), XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
    plain = subprocess.check_output([BINARY, "--graphics", "kitty", "tree", "-L", "3", str(root)], env=env)
    assert b"\x1b_G" not in plain
    assert plain.count(b"example.txt") == 1 and b"link@" in plain
    assert "└─ ".encode() in plain and "├─ ".encode() in plain

    def kitty_output(args, environment, input_bytes=None, columns=100):
        master, slave = pty.openpty()
        try:
            try:
                fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 25, columns, columns * 12, 600))
                child = subprocess.Popen([BINARY, *args], stdin=slave if input_bytes is None else subprocess.PIPE,
                                         stdout=slave, stderr=slave, env=environment)
            finally:
                os.close(slave)
            with child:
                try:
                    if child.stdin is not None:
                        child.stdin.write(input_bytes)
                        child.stdin.close()
                    output = drain(master, 5)
                    return child.wait(timeout=5), output
                finally:
                    if child.poll() is None:
                        child.kill()
        finally:
            os.close(master)

    code, tree = kitty_output(["tree", str(root)], env)
    assert code == 0 and b"\x1b_Ga=t,I=" in tree and b"\x1b_Ga=p,I=" in tree
    assert tree.count(b"example.txt") == 1
    assert b"\x1b_Ga=f," in tree and b"s=2,c=1" in tree
    assert b"\x1b[?1049h" not in tree and b"\x1b[?2026h" not in tree
    assert "└─ ".encode() not in tree and "├─ ".encode() not in tree

    large = root / "large"
    large.mkdir()
    for index in range(1001):
        (large / f"file-{index:05}.txt").touch()
    code, large_tree = kitty_output(["tree", str(large)], env)
    assert code == 0 and large_tree.count(b"file-") == 1000
    assert b"file-01000.txt" not in large_tree and b"display limit reached" in large_tree
    last_row = next(row for row in large_tree.split(b"\r\n") if b"file-00999.txt" in row)
    assert last_row.count(b"\x1b_Ga=p,I=") == 3, "last row needs a branch, icon and label reveal"
    assert b",p=1000," in last_row and b",z=1," in last_row
    assert large_tree.index(last_row) < large_tree.index(b"s=2,c=1"), "play after writing the tree"
    assert large_tree.count(b"\x1b_Ga=t,I=") == 3, "share each image across rows"

    for args, settings in (
        (["tree", "--no-motion", str(root)], env),
        (["tree", str(root)], dict(env, TNMY_REDUCED_MOTION="1")),
        (["tree", str(root)], dict(env, MYT_NO_ANIMATION="1")),
    ):
        code, still_tree = kitty_output(args, settings)
        assert code == 0 and b"\x1b_Ga=p,I=" in still_tree
        assert b"\x1b_Ga=f," not in still_tree and b"example.txt" in still_tree
        assert b",z=1," not in still_tree and b"s=2,c=1" not in still_tree
    code, text_tree = kitty_output(["tree", str(root)], dict(env, TERM="xterm-256color"))
    assert code == 0 and b"example.txt" in text_tree and b"\x1b_G" not in text_tree
    assert "└─ ".encode() in text_tree and "├─ ".encode() in text_tree

    code, listing = kitty_output(["--density", "compact", "ls", "-l", str(root), str(large)], env)
    assert code == 0 and listing.count(b"file-") == 1001
    assert b"link" in listing and " → folder".encode() in listing and b"file-01000.txt" in listing
    assert listing.count(b"\x1b_Ga=t,I=") == 2, "one image per visible section"
    assert b"file-01000.txt" in listing[:listing.index(b"s=2,c=1")]
    assert listing.count(b"\x1b_Ga=p,I=") == 2
    assert b",r=4,z=-1," in listing and b",r=24,z=-1," in listing
    assert b",z=28," in listing
    assert "▪".encode() in listing
    code, grid = kitty_output(["ls", str(large)], env)
    assert code == 0 and grid.count(b"file-") == 1001
    assert any(row.count(b"file-") >= 2 for row in grid.splitlines()), "short lists use multiple columns"
    assert len(grid.splitlines()) < 350, "one terminal row serves several entries"
    code, static_list = kitty_output(["ls", "--no-motion", str(root)], env)
    assert code == 0 and b"\x1b_Ga=p,I=" in static_list and b"\x1b_Ga=f," not in static_list
    code, plain_list = kitty_output(["--graphics", "never", "ls", str(root)], env)
    assert code == 0 and b"\x1b_G" not in plain_list and b"Folders" in plain_list
    code, json_list = kitty_output(["ls", "--json", str(root)], env)
    assert code == 0 and json_list.lstrip().startswith(b"[") and b"\x1b_G" not in json_list
    code, raw_list = kitty_output(["ls", "--zero", str(root)], env)
    assert code == 0 and os.fsencode(root / "folder") + b"\0" in raw_list and b"\x1b_G" not in raw_list

    preview_root = root / "previews"
    preview_root.mkdir()
    png = preview_root / "a.png"
    png.write_bytes(base64.b64decode(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j2uoAAAAASUVORK5CYII="
    ))
    (preview_root / "b.txt").write_text("Back to text preview")
    browser_env = dict(env, TNMY_GRAPHICS="kitty", NO_COLOR="1")
    preview = browser(preview_root, 110, [b"j", b"k", None, b"q"], extra=("--no-git",), environment=browser_env)
    assert preview.count(b"f=100,") == 2 and b"Back" in preview and b"s=2,c=1" in preview
    assert preview.count(b",z=-1,") >= 4, "selection surface follows focus"
    assert b",Y=" in preview, "focus moves through pixel positions between rows"
    leave = preview.index(b"\x1b[?1049l")
    assert b"\x1b_Ga=d,d=N,I=" in preview[:leave]
    chosen = browser(preview_root, 35, [b"p"], extra=("--no-git", "--pick", "--no-motion"),
                     expected_path=png, environment=browser_env, resize=True)
    assert b"f=100," in chosen and b"\x1b_Ga=f," not in chosen and b",Y=" not in chosen

    shell = os.environ.get("TNMY_TEST_SH", "/bin/sh")
    wrapped = ["progress", "--", shell, "-c", "printf 'original output'; exit 7"]
    code, activity = kitty_output(wrapped, env)
    assert code == 7 and b"s=3,v=1,c=1" in activity and b"original output" in activity
    assert activity.index(b"original output") < activity.index(b"\x1b_Ga=d,d=N,I=")
    assert b"\x1b]9;4;3;0" in activity and b"\x1b]9;4;0;0" in activity
    plain = subprocess.run([BINARY, *wrapped], env=env, capture_output=True, timeout=5)
    assert plain.returncode == 7 and plain.stdout == b"original output" and plain.stderr == b""

    counts = b"0\n3\n3\n6\n7\n"
    measured_args = ["progress", "--total", "7"]
    code, meter = kitty_output(measured_args, env, counts)
    assert code == 0 and b" 42%" in meter and b" 85%" in meter and b"100%" in meter
    assert meter.count(b"\x1b_Ga=f,r=1,") == 3 and meter.count(b"\x1b_Ga=p,I=") == 1
    code, narrow = kitty_output(measured_args, env, b"3\n7\n", columns=11)
    assert code == 0 and b"\x1b_G" not in narrow and b"100%" in narrow
    plain = subprocess.run([BINARY, *measured_args], input=b"3\n", env=env, capture_output=True, timeout=5)
    assert plain.returncode == 1 and plain.stdout == b"" and plain.stderr.decode() == "42% · 3/7\n"
    invalid = subprocess.run([BINARY, *measured_args], input=b"8\n", env=env, capture_output=True, timeout=5)
    assert invalid.returncode == 1 and b"counts must be integers from 0 to 7" in invalid.stderr

    nix = root / "nix"
    nix.write_text(f'#!{shell}\nprintf \'%s\\n\' \'{{"json-log-path":{{}}}}\'\n')
    nix.chmod(0o755)
    nh = root / "nh"
    nh.write_text(f"""#!{shell}
printf '%s\\n' '{"{"}"action":"start","type":104,"id":1{"}"}' '{"{"}"action":"result","type":105,"id":1,"fields":[0,1,0,1]{"}"}' > "$5"
exit 7
""")
    nh.chmod(0o755)
    env["PATH"] = str(root)
    code, active = kitty_output(["update", "--action", "build"], env)
    assert code == 7 and b"s=3,v=1,c=1" in active and b"\x1b_Ga=d,d=N,I=" in active
    assert b"0 complete" in active and b"1 failed" in active
    logs = list((root / "state" / "terminal-new-my" / "builds").glob("*.jsonl"))
    assert len(logs) == 1 and logs[0].stat().st_mode & 0o777 == 0o600
    code, still = kitty_output(["update", "--action", "build"], dict(env, TNMY_REDUCED_MOTION="1"))
    assert code == 7 and b"\x1b_Ga=p,I=" in still and b"\x1b_Ga=f," not in still
    assert b"\x1b]9;4;" not in still
    print("Kitty checks passed: animated and static trees, 1,000-entry list, PNG previews and picker, command activity, measured progress, text fallback, build log and status")
