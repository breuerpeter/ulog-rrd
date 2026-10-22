"""px4-rerun's Python side: log a ULog into a recording, and the blueprint views of what it logs.

The importer, ``rerun-importer-ulog``, installs with this package. ``rr.log_file_from_path`` on a
``.ulg`` file runs it from the ``PATH`` with Rerun's options only. :func:`log_ulog` runs it by its
installed path with px4-rerun's options too.
"""

import subprocess
from importlib.metadata import distribution
from pathlib import Path

import rerun as rr
import rerun.blueprint as rrb


def _importer() -> Path:
    """The importer binary this package installed."""
    for file in distribution("px4-rerun").files or []:
        if file.name in ("rerun-importer-ulog", "rerun-importer-ulog.exe"):
            return Path(file.locate()).resolve()
    raise FileNotFoundError("px4-rerun installed no rerun-importer-ulog")


def log_ulog(
    path: str | Path,
    *,
    entity_path_prefix: str | None = None,
    timeline: str = "time",
    time_offset_us: int = 0,
    recording: rr.RecordingStream | None = None,
) -> None:
    """Log the ULog at ``path`` into ``recording``, the current one by default, on ``timeline``.

    ``time_offset_us`` is subtracted from every ULog timestamp.
    """
    rec = recording or rr.get_data_recording()
    args = [str(_importer()), str(path), "--application-id", rec.get_application_id(), "--recording-id", rec.get_recording_id()]
    if entity_path_prefix:
        args += ["--entity-path-prefix", entity_path_prefix]
    args += ["--timeline", timeline, "--time-offset-us", str(time_offset_us)]
    rrd = subprocess.run(args, check=True, capture_output=True).stdout
    rr.log_file_from_contents(f"{Path(path).stem}.rrd", rrd, recording=rec)


def _root(prefix: str) -> str:
    return "/" + prefix.strip("/") if prefix.strip("/") else ""


def views(prefix: str = "") -> list[rrb.View | rrb.Container]:
    """The views of the vehicle under ``prefix``: a camera that follows the vehicle beside the
    whole flight, and its log."""
    root = _root(prefix)
    # Every entity px4-rerun draws in 3D, so a new one shows without a change here.
    contents = [f"+ {root}/**", f"- {root}/log", f"- {root}/timeseries/**"]
    return [
        rrb.Horizontal(
            # The origin moves with the vehicle and keeps the world's axes, so the eye stays behind it.
            rrb.Spatial3DView(
                name="PX4 vehicle",
                origin=f"{root}/vehicle",
                contents=contents,
                eye_controls=rrb.archetypes.EyeControls3D(
                    kind="Orbital", position=[-1.2, -1.2, 0.8], look_target=[0.0, 0.0, 0.0], eye_up=[0.0, 0.0, 1.0]
                ),
            ),
            rrb.Spatial3DView(name="PX4 flight", origin=root or "/", contents=contents),
            name="PX4 3D",
        ),
        rrb.TextLogView(name="PX4 log", origin=f"{root}/log"),
    ]
