#!/usr/bin/env python3
"""Download the pinned official Core for opt-in smoke tests; never used by the app."""
import argparse
import hashlib
import io
import os
from pathlib import Path
import platform
import re
import urllib.request
import zipfile

VERSION = "v26.11.9"
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--destination", type=Path, default=Path(".test-core"))
parser.add_argument("--github-env", action="store_true")
args = parser.parse_args()
if platform.machine().lower() not in ("x86_64", "amd64"):
    parser.error("The smoke fixture currently supports x86_64 only")
if platform.system() not in ("Windows", "Linux"):
    parser.error("Use a manually selected Core on this platform")
windows = platform.system() == "Windows"
archive = "Xray-windows-64.zip" if windows else "Xray-linux-64.zip"
url = f"https://github.com/XTLS/Xray-core/releases/download/{VERSION}/{archive}"

def download(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()

payload = download(url)
digests = download(url + ".dgst").decode("ascii")
expected = re.search(r"^SHA2-256=\s*([0-9a-fA-F]{64})\s*$", digests, re.MULTILINE)
if expected is None or hashlib.sha256(payload).hexdigest() != expected.group(1).lower():
    raise SystemExit("Official release SHA-256 verification failed")
args.destination.mkdir(parents=True, exist_ok=True)
exe = args.destination.resolve() / ("xray.exe" if windows else "xray")
with zipfile.ZipFile(io.BytesIO(payload)) as bundle:
    exe.write_bytes(bundle.read(exe.name))
if not windows:
    exe.chmod(0o700)
print(f"Verified {VERSION}: {exe}")
if args.github_env:
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as env:
        env.write(f"XRAY_TEST_CORE={exe}\n")
