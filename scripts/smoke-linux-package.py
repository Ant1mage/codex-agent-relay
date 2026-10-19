"""Check an extracted Debian package without installing or modifying user state."""

import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request


def main(package):
    with tempfile.TemporaryDirectory(prefix="relay-package-") as temporary:
        root = Path(temporary)
        extracted = root / "extracted"
        subprocess.run(["dpkg-deb", "-x", package, str(extracted)], check=True)
        daemons = list(extracted.glob("usr/lib/*/bin/relayd"))
        assert len(daemons) == 1, f"Expected one bundled daemon, found {daemons}"
        resources = daemons[0].parent.parent
        for name in ("relayd", "relay-mcp"):
            binary = resources / "bin" / name
            assert os.access(binary, os.X_OK), f"Not executable: {binary}"
            subprocess.run([str(binary), "--version"], check=True, timeout=10)
        for relative in (
            "ui/index.html",
            "assets/appicon/png/light/relay-icon-32.png",
            "integrations/codex/plugin.json",
            "integrations/codex/skills/relay/SKILL.md",
        ):
            assert (resources / relative).is_file(), f"Missing {relative}"
        assert list((resources / "ui").glob("*.wasm")), "Missing compiled WASM UI"
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        state = root / "state"
        environment = {
            **os.environ,
            "RELAY_HOME": str(state),
            "RELAY_PORT": str(port),
            "RELAY_RESOURCES_DIR": str(resources),
        }
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with (root / "daemon.log").open("w+") as log:
            process = subprocess.Popen(
                [str(daemons[0])], env=environment, stdout=log, stderr=log
            )
            try:
                deadline = time.monotonic() + 45
                while not (state / "server.json").is_file():
                    if process.poll() is not None or time.monotonic() > deadline:
                        log.seek(0)
                        raise RuntimeError(f"Daemon did not start: {log.read()}")
                    time.sleep(0.1)
                info = json.loads((state / "server.json").read_text())
                health_url = f"{info['url']}/api/health?token={info['token']}"
                with opener.open(health_url, timeout=10) as response:
                    health = json.load(response)
                assert health["pid"] == process.pid, "Wrong daemon answered"
                for route in ("/", "/panel/"):
                    with opener.open(info["url"] + route, timeout=10) as response:
                        html = response.read()
                        assert b"wasm" in html, f"UI missing at {route}"
                print("Package smoke check passed: binaries, resources, API, UI")
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            assert process.returncode == 0, f"Daemon exit: {process.returncode}"


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: smoke-linux-package.py package.deb")
    main(sys.argv[1])
