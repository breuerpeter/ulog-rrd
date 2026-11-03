"""PX4 ULog to Rerun: convert a flight log to ``.rrd`` bytes, or append it to a recording on disk.

The converter is Rust, shared with the npm package that pulsar runs in the browser. This package
adds what Python callers need: appending a converted log to an ``.rrd`` another process wrote, and the
blueprint views of the entities the converter logs.

The entities, under ``entity_path_prefix``: ``vehicle`` (position) with ``vehicle/body`` (attitude,
axes, marker, magnetometers) and ``vehicle/velocity``; ``trail``, ``home``, ``setpoint``,
``mission_item``; ``log`` (``TextLog``); and ``timeseries/<topic>`` when ``scalars`` is set.
"""

from __future__ import annotations

from pathlib import Path

from ulog_rrd._ulog_rrd import RERUN_VERSION, __version__
from ulog_rrd._ulog_rrd import convert as _convert
from ulog_rrd.views import blueprint, views

__all__ = ["RERUN_VERSION", "__version__", "append", "blueprint", "convert", "views"]


def convert(
    ulog: bytes,
    *,
    application_id: str = "ulog-rrd",
    recording_id: str | None = None,
    entity_path_prefix: str | None = None,
    timeline: str = "time",
    time_offset_us: int = 0,
    scalars: bool = False,
    translation: tuple[float, float, float] | None = None,
    rotation: tuple[float, float, float, float] | None = None,
) -> bytes:
    """The recording of a ULog as ``.rrd`` bytes.

    Args:
        ulog: The ULog file's bytes.
        application_id: The application the recording belongs to; Rerun applies a blueprint by it.
        recording_id: The recording the rows join; ``None`` makes one up.
        entity_path_prefix: Put before every entity path, such as ``px4_sitl/logs``; ``None`` puts
            the rows at the root, with Rerun's z-up frame declared there.
        timeline: The duration timeline the rows sit on.
        time_offset_us: Subtracted from every ULog timestamp, in microseconds.
        scalars: Log every numeric field of every topic as a series under ``timeseries/``.
        translation: Where the ULog's frame, east-north-up, sits in the caller's world, in metres,
            logged as the transform of the prefix root. Needs a prefix.
        rotation: The quaternion ``(x, y, z, w)`` rotating the ULog's frame into the caller's world,
            logged with ``translation``.

    Returns:
        The ``.rrd`` bytes: one stream, which a viewer opens and ``append`` adds to a file.

    Raises:
        ValueError: The bytes are not a ULog this converter reads.
    """
    return _convert(
        ulog,
        application_id=application_id,
        recording_id=recording_id,
        entity_path_prefix=entity_path_prefix,
        timeline=timeline,
        time_offset_us=time_offset_us,
        scalars=scalars,
        translation=translation,
        rotation=rotation,
    )


def append(ulog: str | Path, rrd: str | Path, *, application_id: str, recording_id: str, **options) -> None:
    """Convert the ULog at ``ulog`` and append it to the recording at ``rrd``.

    An ``.rrd`` file is a sequence of streams, so the converted log goes on the end of the file
    and the viewer reads both. The rows join the file's recording when ``application_id`` and
    ``recording_id`` are the file's; the file's blueprint stays as it is.

    Args:
        ulog: The ULog file.
        rrd: The recording to append to, which a closed ``rr.save`` wrote.
        application_id: The file's application id.
        recording_id: The file's recording id.
        **options: The other arguments of :func:`convert`.
    """
    data = convert(Path(ulog).read_bytes(), application_id=application_id, recording_id=recording_id, **options)
    with open(rrd, "ab") as f:
        f.write(data)
