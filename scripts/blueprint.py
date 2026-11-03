"""Write the blueprint the wasm package embeds, for the default application id.

Run before `wasm-pack build`, with rerun-sdk installed:

    python scripts/blueprint.py

It imports the views from the source tree, so the Python extension need not be built.
"""

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VIEWS = ROOT / "crates" / "ulog-rrd-py" / "python" / "ulog_rrd" / "views.py"

# By path, not as `ulog_rrd.views`: the package's `__init__` imports the extension.
spec = importlib.util.spec_from_file_location("views", VIEWS)
views = importlib.util.module_from_spec(spec)
spec.loader.exec_module(views)

APPLICATION_ID = "ulog-rrd"  # ulog_rrd::Options::default().application_id

out = ROOT / "crates" / "ulog-rrd-wasm" / "blueprint.rbl"
views.blueprint().save(APPLICATION_ID, str(out))
print(out)
