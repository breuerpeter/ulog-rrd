"""The Python package over the converter: convert, append and the views."""

import struct
import subprocess
import sys
from pathlib import Path

import pytest
import rerun as rr
import rerun.blueprint as rrb

import ulog_rrd


def _message(kind: bytes, payload: bytes) -> bytes:
    return struct.pack("<HB", len(payload), kind[0]) + payload


def flight() -> bytes:
    """A ULog of two position samples with an identity attitude and one text message."""
    out = b"ULog\x01\x12\x35\x01" + struct.pack("<Q", 1_000_000)
    out += _message(b"B", bytes(40))
    out += _message(b"F", b"vehicle_local_position:uint64_t timestamp;float x;float y;float z;")
    out += _message(b"F", b"vehicle_attitude:uint64_t timestamp;float[4] q;")
    out += _message(b"A", struct.pack("<BH", 0, 0) + b"vehicle_local_position")
    out += _message(b"A", struct.pack("<BH", 0, 1) + b"vehicle_attitude")
    out += _message(b"D", struct.pack("<HQ4f", 1, 1_000_000, 1.0, 0.0, 0.0, 0.0))
    for i, x in enumerate((1.0, 2.0)):
        out += _message(b"D", struct.pack("<HQ3f", 0, 1_000_000 + i * 100_000, x, 2.0, -3.0))
    out += _message(b"L", b"6" + struct.pack("<Q", 1_050_000) + b"[commander] Takeoff detected")
    return out


def entities(rrd: Path) -> set[str]:
    """The entity paths in an .rrd, from the bundled command-line tool, which reads the file as the
    viewer does."""
    import re

    out = subprocess.run(
        [sys.executable, "-m", "rerun", "rrd", "print", str(rrd)], capture_output=True, text=True, check=True
    )
    return set(re.findall(r" - (/\S*) - ", out.stdout))


def test_convert_is_a_recording_of_the_vehicle(tmp_path):
    rrd = tmp_path / "flight.rrd"
    rrd.write_bytes(ulog_rrd.convert(flight()))
    assert {"/vehicle", "/vehicle/body", "/trail", "/log"} <= entities(rrd)
    assert not any(e.startswith("/timeseries") for e in entities(rrd)), "scalars are opt in"


def test_append_joins_the_files_recording_and_keeps_its_blueprint(tmp_path):
    ulg = tmp_path / "flight.ulg"
    ulg.write_bytes(flight())
    rrd = tmp_path / "run.rrd"
    rr.init("app", recording_id="run")
    rr.save(str(rrd))
    rr.set_time("time", duration=0.0)
    rr.log("sim/vehicle/body", rr.Transform3D(translation=[0.0, 0.0, 1.0]))
    rr.send_blueprint(rrb.Blueprint(rrb.Spatial3DView(origin="/", name="Scene")), make_active=True)
    rr.disconnect()
    before = entities(rrd)

    ulog_rrd.append(
        ulg, rrd, application_id="app", recording_id="run", entity_path_prefix="px4_sitl/logs", scalars=True
    )

    after = entities(rrd)
    assert before <= after, "nothing of the file is lost, the blueprint included"
    assert {
        "/px4_sitl/logs",
        "/px4_sitl/logs/vehicle",
        "/px4_sitl/logs/log",
        "/px4_sitl/logs/timeseries/vehicle_attitude",
    } <= after
    assert len([e for e in after if e.startswith("/view/")]) == len([e for e in before if e.startswith("/view/")])
    # One footer: the chunk reader, which reads the file by its footer, sees the new rows too.
    from rerun.chunk import RrdReader

    assert "/px4_sitl/logs/vehicle" in {chunk.entity_path for chunk in RrdReader(str(rrd)).stream()}
    assert not rrd.with_suffix(".rrd.tmp").exists()


def test_append_needs_both_files(tmp_path):
    with pytest.raises(FileNotFoundError):
        ulog_rrd.append(tmp_path / "flight.ulg", tmp_path / "missing.rrd", application_id="app", recording_id="run")


def test_garbage_is_a_value_error():
    with pytest.raises(ValueError):
        ulog_rrd.convert(b"not a ulog")


def test_views_name_the_prefix():
    bp = ulog_rrd.blueprint("px4_sitl/logs")
    assert isinstance(bp, rrb.Blueprint)
    vehicle, flight = ulog_rrd.views("px4_sitl/logs")[0].contents
    assert (vehicle.origin, flight.origin) == ("/px4_sitl/logs/vehicle", "/px4_sitl/logs")
    assert ulog_rrd.RERUN_VERSION == rr.__version__
