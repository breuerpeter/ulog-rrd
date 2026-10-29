"""Finish the npm package wasm-pack wrote: the blueprints, from the same views the wheel ships, and
the Rerun web viewer version the recordings are encoded for, as a peer dependency.

Run with the wheel installed: python scripts/package_npm.py <npm dir>
"""

import json
import sys
from pathlib import Path

import px4_rerun
import rerun.blueprint as rrb

npm = Path(sys.argv[1])
(npm / "blueprints").mkdir(exist_ok=True)
rrb.Blueprint(*px4_rerun.views(), collapse_panels=True).save("px4-rerun", str(npm / "blueprints" / "vehicle.rbl"))

pkg = json.loads((npm / "package.json").read_text())
if "blueprints" not in pkg["files"]:
    pkg["files"].append("blueprints")
pkg["peerDependencies"] = {"@rerun-io/web-viewer": "~0.40.0"}
pkg["exports"] = {".": "./px4_rerun_wasm.js", "./blueprints/*": "./blueprints/*"}
(npm / "package.json").write_text(json.dumps(pkg, indent=2) + "\n")
