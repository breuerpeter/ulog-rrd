# ulog-rrd

PX4 ULog → Rerun `.rrd`. One Rust converter, shipped two ways:

- **npm `ulog-rrd`**: wasm for the browser. [Pulsar](https://github.com/breuerpeter/pulsar) runs it
  in a worker for *Open in Rerun*.
- **PyPI `ulog-rrd`**: a wheel for Python. [nexus](https://github.com/breuerpeter/nexus) appends
  the PX4 SITL flight log to its own recording at teardown.

Both encode for one Rerun version, which the package names: `@rerun-io/web-viewer` as a peer
dependency, `rerun-sdk` as a dependency. Another Rerun version does not read the recordings.

## What a recording holds

Each path sits under the `entity_path_prefix` the caller gives, or at the root.

| Path | Rows |
|---|---|
| `vehicle` | The estimated position, a `Transform3D` per `vehicle_local_position` sample |
| `vehicle/body` | The estimated attitude, a `Transform3D` per sample |
| `vehicle/body/frd` | The body axes, static: forward red, right green, down blue |
| `vehicle/body/marker` | A dot sized in screen points, so the vehicle shows at any zoom |
| `vehicle/body/sensor_mag/<n>` | Each magnetometer's field in the body frame, an arrow |
| `vehicle/body/mag_cal/<n>` | Each magnetometer's calibration samples, a point cloud |
| `vehicle/velocity` | The velocity, an arrow at the vehicle labelled with the speed |
| `trail` | The flown path, one static line strip |
| `home` | The home position, each time PX4 sets it |
| `setpoint`, `setpoint/yaw` | The position setpoint and its yaw |
| `mission_item` | The current mission item, labelled with its command |
| `log` | PX4's text messages, a `TextLog` each |
| `timeseries/<topic>` | Every numeric field of every topic, one series per field (`scalars`) |

Positions are in Rerun's default z-up frame: east, north, up. Without a prefix the root declares
that frame. Under a prefix the caller places the subtree in its own world with `translation` and
`rotation`, logged once as the transform of the prefix root.

## Browser

```ts
import init, { convert, Options } from "ulog-rrd";

await init();
const options = new Options();
options.scalars = true;
options.blueprint = true; // the vehicle up close beside the whole flight, the log below
const rrd = convert(new Uint8Array(ulogBytes), options);
viewer.open_channel("flight").send_rrd(rrd);
```

`Options` has `applicationId`, `recordingId`, `entityPathPrefix`, `timeline`, `timeOffsetUs`,
`scalars`, `translation`, `rotation` and `blueprint`. The blueprint applies to the default
application id only, which it names.

## Python

```python
import ulog_rrd

# A standalone recording
rrd = ulog_rrd.convert(open("flight.ulg", "rb").read(), scalars=True)

# Appended to a recording another process wrote, under its own prefix, in its own frame
ulog_rrd.append(
    "flight.ulg",
    "run.rrd",
    application_id="nexus",
    recording_id="nexus",
    entity_path_prefix="px4_sitl/logs",
    rotation=(0.0, 0.0, -0.7071068, 0.7071068),
)

# The blueprint views of what was logged, for a blueprint of your own
ulog_rrd.views("px4_sitl/logs")
```

An `.rrd` file is a sequence of streams, so `append` writes the converted log on the end of the
file. The file's blueprint stays as it is.

## Build

[Rust 1.98](rust-toolchain.toml), [wasm-pack](https://rustwasm.github.io/wasm-pack/),
[maturin](https://www.maturin.rs/) and a Python with `rerun-sdk` installed.

```bash
cargo test                                                  # the converter
python scripts/blueprint.py                                 # the blueprint the wasm embeds
CARGO_PROFILE_RELEASE_OPT_LEVEL=z wasm-pack build crates/ulog-rrd-wasm --target web --release --out-dir pkg --out-name ulog_rrd
python scripts/package_npm.py crates/ulog-rrd-wasm/pkg     # → npm package in crates/ulog-rrd-wasm/pkg
maturin build --release -m crates/ulog-rrd-py/Cargo.toml   # → wheel in target/wheels
```

The wasm builds at `opt-level = "z"`, since it downloads; the wheel at the release profile's
`opt-level = 3`, since it is 1.7× faster.

A conversion of a 415 MB log takes about 11 s natively. The example `cargo run --release -p ulog-rrd
--example convert -- flight.ulg flight.rrd --scalars` converts a file.

## Release

Commits follow [Conventional Commits](https://www.conventionalcommits.org/); release-please opens
the release pull request, and merging it tags the release. The release workflow then publishes the
npm package and the wheel through trusted publishing.
