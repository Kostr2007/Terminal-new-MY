#!/usr/bin/env python3
"""Parse generated themes using the actual consumers, with isolated state."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tnmy").resolve())
project = Path(__file__).resolve().parent.parent
with tempfile.TemporaryDirectory(prefix="tnmy-tools-") as temporary:
    root = Path(temporary)
    env = dict(os.environ, TNMY_TOOL_THEME_DIR=str(root), TNMY_BINARY=binary,
               TNMY_FISH=str(project / "integrations/fish/tnmy.fish"))
    for mode in ("dark", "light"):
        palette = project / f"tests/palettes/blue-{mode}.json"
        subprocess.run([binary, "--palette", str(palette), "theme", "--export", str(root), "--build-bat"], check=True, env=env)
        assert json.loads((root / "manifest.json").read_text())["mode"] == mode
        script = '''
source "$TNMY_FISH"
myeza --color=always --long "$TNMY_FISH"; or exit 1
mybat --color=always --paging=never "$TNMY_FISH"; or exit 2
printf 'alpha\\nbeta\\n' | myfzf --filter alpha; or exit 3
mydiff --show-config; or exit 4
'''
        result = subprocess.run(["fish", "--no-config", "--interactive", "-c", script], env=env, capture_output=True, timeout=20)
        assert result.returncode == 0, result.stderr.decode(errors="replace")
        assert b"Unknown theme" not in result.stderr, result.stderr
        assert b"error" not in result.stderr.lower(), result.stderr
        assert b"\x1b[" in result.stdout
    print("Tool integration checks passed: eza, bat, fzf, delta; dark and light")
