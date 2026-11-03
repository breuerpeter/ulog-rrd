"""Finish the npm package wasm-pack wrote: its name, the files it ships and the Rerun web viewer
version the recordings are encoded for, as a peer dependency.

    python scripts/package_npm.py crates/ulog-rrd-wasm/pkg
"""

import json
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
pkg = Path(sys.argv[1])

cargo = (ROOT / "Cargo.toml").read_text()
version = re.search(r'^version = "([^"]+)"', cargo, re.M).group(1)
rerun = re.search(r'^re_sdk_types = \{ version = "=([^"]+)"', cargo, re.M).group(1)

manifest = json.loads((pkg / "package.json").read_text())
manifest.update(
    {
        "name": "ulog-rrd",
        "version": version,
        "description": "PX4 ULog to Rerun .rrd in the browser: the vehicle in 3D, its log and its time series",
        "license": "MIT",
        "repository": {"type": "git", "url": "git+https://github.com/breuerpeter/ulog-rrd.git"},
        "keywords": ["px4", "ulog", "rerun", "drone", "flight-log", "wasm"],
        "files": ["ulog_rrd_bg.wasm", "ulog_rrd.js", "ulog_rrd.d.ts", "README.md", "LICENSE"],
        "peerDependencies": {"@rerun-io/web-viewer": rerun},
    }
)
(pkg / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
shutil.copy(ROOT / "README.md", pkg / "README.md")
shutil.copy(ROOT / "LICENSE", pkg / "LICENSE")
print(f"ulog-rrd {version} for @rerun-io/web-viewer {rerun}")
