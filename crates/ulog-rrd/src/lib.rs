//! PX4 ULog → Rerun recording, as `.rrd` bytes.
//!
//! [`convert`] is the one entry point. The wasm package that pulsar loads and the Python wheel that
//! nexus imports both call it, so the visualizations live here once.
//!
//! The recording is encoded from chunks directly, not through Rerun's `RecordingStream`: the stream
//! starts threads and pulls in networking, and neither builds for wasm32.
//!
//! What a recording holds, each path under [`Options::entity_path_prefix`]:
//!
//! | Path | Rows |
//! |---|---|
//! | `vehicle` | The estimated position, a `Transform3D` per `vehicle_local_position` sample |
//! | `vehicle/body` | The estimated attitude, a `Transform3D` per sample |
//! | `vehicle/body/frd` | The body axes, static: forward red, right green, down blue |
//! | `vehicle/body/marker` | A dot sized in screen points, so the vehicle shows at any zoom |
//! | `vehicle/body/sensor_mag/<n>` | Each magnetometer's field in the body frame, an arrow |
//! | `vehicle/body/mag_cal/<n>` | Each magnetometer's calibration samples, a point cloud |
//! | `vehicle/velocity` | The velocity, an arrow at the vehicle labelled with the speed |
//! | `trail` | The flown path, one static line strip |
//! | `home` | The home position, each time PX4 sets it |
//! | `setpoint`, `setpoint/yaw` | The position setpoint and its yaw |
//! | `mission_item` | The current mission item, labelled with its command |
//! | `log` | PX4's text messages, a `TextLog` each |
//! | `timeseries/<topic>` | Every numeric field of every topic, one series per field (opt in) |
//!
//! Positions are in the z-up frame Rerun draws by default: east, north, up. PX4's north, east,
//! down becomes that by swapping x and y and flipping z. Without a prefix the recording's root
//! declares that frame; under a prefix the caller places the subtree in its own world through
//! [`Options::translation`] and [`Options::rotation`].

mod parse;
mod viz;
mod write;

use re_chunk::RowId;
use re_log_encoding::Encoder;
use re_log_msg::{LogMsg, SetStoreInfo, StoreInfo, StoreSource};
use re_log_types::{ApplicationId, RecordingId, StoreId};

/// This crate's version, which is also the version of the npm package and the wheel.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Rerun version the recordings are encoded for. A consumer needs the same Rerun version.
pub const RERUN_VERSION: &str = "0.41.0";

/// What the caller decides; everything else is the converter's.
#[derive(Clone, Debug)]
pub struct Options {
    /// The application the recording belongs to. Rerun applies a blueprint by this id.
    pub application_id: String,
    /// The recording the rows join; `None` makes one up, so the recording stands alone.
    pub recording_id: Option<String>,
    /// Put before every entity path, such as `px4_sitl/logs`; `None` puts the rows at the root.
    pub entity_path_prefix: Option<String>,
    /// The duration timeline the rows sit on.
    pub timeline: String,
    /// Subtracted from every ULog timestamp, in microseconds: the caller's time zero.
    pub time_offset_us: i64,
    /// Log every numeric field of every topic as a series under `timeseries/`.
    pub scalars: bool,
    /// Where the ULog's frame sits in the caller's world: a translation in metres and a rotation
    /// quaternion (x, y, z, w), logged once as the transform of the prefix root. `None` is identity.
    /// Needs a prefix: at the root there is no entity to carry it.
    pub translation: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            application_id: "ulog-rrd".to_owned(),
            recording_id: None,
            entity_path_prefix: None,
            timeline: "time".to_owned(),
            time_offset_us: 0,
            scalars: false,
            translation: None,
            rotation: None,
        }
    }
}

/// Why a conversion failed: the ULog did not parse, or Rerun refused a chunk.
#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}

/// The recording of `ulog` as `.rrd` bytes, in the store the options name.
pub fn convert(ulog: &[u8], opts: &Options) -> Result<Vec<u8>, Error> {
    let log = parse::parse(ulog)?;
    let mut writer = write::Writer::new(opts)?;
    viz::visualize(&log, &mut writer)?;
    let chunks = writer.finish()?;

    let recording_id = match &opts.recording_id {
        Some(id) => RecordingId::from(id.as_str()),
        None => RecordingId::random(),
    };
    let application_id = ApplicationId::try_new(opts.application_id.clone()).map_err(err)?;
    let store_id = StoreId::recording(application_id, recording_id);
    let info = StoreInfo::new(store_id.clone(), StoreSource::Other(format!("ulog-rrd {VERSION}")));
    let mut encoder = Encoder::local().map_err(err)?;
    encoder.append(&LogMsg::SetStoreInfo(SetStoreInfo { row_id: *RowId::new(), info })).map_err(err)?;
    for chunk in chunks {
        let msg = chunk.to_arrow_msg().map_err(err)?;
        encoder.append(&LogMsg::ArrowMsg(store_id.clone(), msg)).map_err(err)?;
    }
    encoder.finish().map_err(err)?;
    encoder.into_inner().map_err(err)
}
