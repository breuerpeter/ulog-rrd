"""The blueprint views of what the converter logs. Rerun's blueprint API only; the converter is not
needed, so ``scripts/blueprint.py`` imports this module from the source tree."""

from __future__ import annotations

import rerun.blueprint as rrb


def _root(prefix: str) -> str:
    prefix = prefix.strip("/")
    return f"/{prefix}" if prefix else ""


def views(prefix: str = "") -> list[rrb.View | rrb.Container]:
    """The views of the vehicle logged under ``prefix``: a camera that follows the vehicle beside
    the whole flight, and the log below.

    Args:
        prefix: The ``entity_path_prefix`` the log was converted with, or empty for the root.

    Returns:
        A container of the two 3D views and the log view, for a ``rrb.Blueprint`` or a container.
    """
    root = _root(prefix)
    # Every entity the converter draws in 3D, so a new one shows without a change here.
    contents = [f"+ {root}/**", f"- {root}/log", f"- {root}/timeseries/**"]
    return [
        rrb.Horizontal(
            # The origin moves with the vehicle and keeps the world's axes, so the eye stays behind it:
            # south-west of it and above, in PX4's north-east-down world.
            rrb.Spatial3DView(
                name="Vehicle",
                origin=f"{root}/vehicle",
                contents=contents,
                eye_controls=rrb.archetypes.EyeControls3D(
                    kind="Orbital", position=[-1.2, -1.2, -0.8], look_target=[0.0, 0.0, 0.0], eye_up=[0.0, 0.0, -1.0]
                ),
            ),
            rrb.Spatial3DView(name="Flight", origin=root or "/", contents=contents),
        ),
        rrb.TextLogView(name="Log", origin=f"{root}/log"),
    ]


def blueprint(prefix: str = "") -> rrb.Blueprint:
    """The blueprint of a standalone recording: :func:`views`, the 3D pair over the log, panels
    collapsed."""
    return rrb.Blueprint(rrb.Vertical(*views(prefix), row_shares=[3, 1]), collapse_panels=True)
