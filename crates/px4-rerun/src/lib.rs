//! ULog → Rerun recording, as RRD bytes. Every front end calls [`convert`]: the wasm package that
//! pulsar loads, and the Rerun importer binary that nexus and `rerun flight.ulg` run.
//!
//! The recording is encoded from chunks directly, not through `RecordingStream`, since the stream
//! starts threads and pulls in networking, and neither builds for wasm32.

use std::collections::HashMap;
use std::io::Cursor;

use re_chunk::{Chunk, ChunkBuilder, RowId};
use re_log_encoding::Encoder;
use re_log_msg::{LogMsg, SetStoreInfo, StoreInfo, StoreSource};
use re_log_types::{ApplicationId, EntityPath, StoreId, TimeInt, TimePoint, Timeline};
use re_sdk_types::archetypes::{Arrows3D, LineStrips3D, Points3D, Scalars, TextLog, Transform3D, ViewCoordinates};
use re_sdk_types::components::{Color, Radius, TextLogLevel};
use re_sdk_types::datatypes::Quaternion;
use re_sdk_types::AsComponents;
use yule_log::model::{inst::FieldValue, msg::UlogMessage};
use yule_log::parser::ULogParser;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Rows per chunk: big enough to amortize the chunk header, small enough for the viewer's index.
const ROWS_PER_CHUNK: usize = 4096;

/// What the caller decides; everything else is px4-rerun's.
#[derive(Clone, Debug)]
pub struct Options {
    /// The store the rows belong to: the caller's own, so they join its recording.
    pub application_id: String,
    pub recording_id: String,
    /// Put before every entity path, such as `px4_sitl`; none puts PX4's rows at the root.
    pub entity_path_prefix: Option<String>,
    /// The duration timeline the rows sit on.
    pub timeline: String,
    /// Subtracted from every ULog timestamp, in microseconds: the caller's time zero.
    pub time_offset_us: i64,
    /// Log every numeric field of every topic as a series, under `timeseries/`.
    pub all_scalars: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            application_id: "px4-rerun".to_owned(),
            recording_id: "px4-rerun".to_owned(),
            entity_path_prefix: None,
            timeline: "time".to_owned(),
            time_offset_us: 0,
            all_scalars: false,
        }
    }
}

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

/// One topic instance's numeric scalar fields, as columns.
#[derive(Default)]
struct Table {
    timestamps: Vec<u64>,
    columns: HashMap<String, Vec<f64>>,
}

impl Table {
    fn get(&self, field: &str) -> Option<&[f64]> {
        self.columns.get(field).map(Vec::as_slice)
    }
}

struct Ulog {
    tables: HashMap<String, Table>,
    /// (timestamp µs, level 0–7, text)
    messages: Vec<(u64, u8, String)>,
}

fn scalar(value: &FieldValue) -> Option<f64> {
    Some(match value {
        FieldValue::ScalarU8(v) => *v as f64,
        FieldValue::ScalarU16(v) => *v as f64,
        FieldValue::ScalarU32(v) => *v as f64,
        FieldValue::ScalarU64(v) => *v as f64,
        FieldValue::ScalarI8(v) => *v as f64,
        FieldValue::ScalarI16(v) => *v as f64,
        FieldValue::ScalarI32(v) => *v as f64,
        FieldValue::ScalarI64(v) => *v as f64,
        FieldValue::ScalarF32(v) => *v as f64,
        FieldValue::ScalarF64(v) => *v,
        FieldValue::ScalarBool(v) => *v as u8 as f64,
        _ => return None,
    })
}

fn parse(bytes: &[u8]) -> Result<Ulog, Error> {
    let mut tables: HashMap<String, Table> = HashMap::new();
    let mut messages = Vec::new();
    for msg in ULogParser::new(Cursor::new(bytes)).map_err(err)? {
        match msg.map_err(err)? {
            UlogMessage::LoggedData(data) => {
                let topic = match data.data.multi_id_index {
                    Some(i) if i > 0 => format!("{}_{i}", data.data.name),
                    _ => data.data.name.to_string(),
                };
                let table = tables.entry(topic).or_default();
                table.timestamps.push(data.timestamp);
                let row = table.timestamps.len();
                for field in &data.data.fields {
                    let values: Vec<(String, f64)> = match &field.value {
                        FieldValue::ArrayF32(a) => a.iter().enumerate().map(|(i, v)| (format!("{}[{i}]", field.name), *v as f64)).collect(),
                        FieldValue::ArrayF64(a) => a.iter().enumerate().map(|(i, v)| (format!("{}[{i}]", field.name), *v)).collect(),
                        value => scalar(value).map(|v| vec![(field.name.to_string(), v)]).unwrap_or_default(),
                    };
                    for (name, v) in values {
                        if name.starts_with("_padding") {
                            continue;
                        }
                        let column = table.columns.entry(name).or_default();
                        // A field missing on earlier rows reads NaN there.
                        column.resize(row - 1, f64::NAN);
                        column.push(v);
                    }
                }
            }
            UlogMessage::LoggedString(s) | UlogMessage::TaggedLoggedString(s) => {
                messages.push((s.timestamp, (s.level as u8).saturating_sub(b'0'), s.msg));
            }
            _ => {}
        }
    }
    Ok(Ulog { tables, messages })
}

/// NED (north, east, down) → the z-up frame px4-rerun draws in: east, north, up.
fn ned_to_zup(x: f64, y: f64, z: f64) -> [f32; 3] {
    [y as f32, x as f32, -z as f32]
}

/// PX4's attitude quaternion in the z-up frame, as px4-rerun's C++ library converts it.
fn ned_quat_to_zup(w: f64, x: f64, y: f64, z: f64) -> Quaternion {
    Quaternion::from_xyzw([-y as f32, -x as f32, z as f32, w as f32])
}

fn level(severity: u8) -> TextLogLevel {
    match severity {
        0..=3 => TextLogLevel::ERROR.into(),
        4 => TextLogLevel::WARN.into(),
        5 | 6 => TextLogLevel::INFO.into(),
        _ => TextLogLevel::DEBUG.into(),
    }
}

/// Builds the rows entity by entity, splitting each into chunks of `ROWS_PER_CHUNK`.
struct Writer<'a> {
    opts: &'a Options,
    timeline: Timeline,
    chunks: Vec<Chunk>,
    open: HashMap<String, (ChunkBuilder, usize)>,
}

impl<'a> Writer<'a> {
    fn new(opts: &'a Options) -> Self {
        Self { opts, timeline: Timeline::new_duration(re_log_types::TimelineName::try_new(&opts.timeline).expect("timeline name")), chunks: Vec::new(), open: HashMap::new() }
    }

    fn path(&self, path: &str) -> EntityPath {
        match &self.opts.entity_path_prefix {
            Some(prefix) => EntityPath::parse_forgiving(&format!("{}/{path}", prefix.trim_end_matches('/'))),
            None => EntityPath::parse_forgiving(path),
        }
    }

    fn at(&self, timestamp_us: u64) -> TimePoint {
        let ns = (timestamp_us as i64 - self.opts.time_offset_us) * 1000;
        TimePoint::default().with(self.timeline, TimeInt::new_temporal(ns))
    }

    fn row(&mut self, path: &str, time: TimePoint, archetype: &dyn AsComponents) -> Result<(), Error> {
        let entity = self.path(path);
        let (builder, n) = self.open.remove(path).unwrap_or_else(|| (Chunk::builder(entity.clone()), 0));
        let builder = builder.with_archetype(RowId::new(), time, archetype);
        if n + 1 >= ROWS_PER_CHUNK {
            self.chunks.push(builder.build().map_err(err)?);
        } else {
            self.open.insert(path.to_owned(), (builder, n + 1));
        }
        Ok(())
    }

    fn static_row(&mut self, path: &str, archetype: &dyn AsComponents) -> Result<(), Error> {
        let chunk = Chunk::builder(self.path(path))
            .with_archetype(RowId::new(), TimePoint::default(), archetype)
            .build()
            .map_err(err)?;
        self.chunks.push(chunk);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<Chunk>, Error> {
        for (_, (builder, _)) in self.open.drain() {
            self.chunks.push(builder.build().map_err(err)?);
        }
        Ok(self.chunks)
    }
}

/// The visualizations: each reads the tables it needs and writes its own entities.
fn visualize(ulog: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    if w.opts.entity_path_prefix.is_none() {
        // Standalone: px4-rerun's z-up frame is the world. Under a prefix the caller places it.
        w.static_row("", &ViewCoordinates::RFU())?;
    }
    pose(ulog, w)?;
    for (ts, severity, text) in &ulog.messages {
        w.row("log", w.at(*ts), &TextLog::new(text.as_str()).with_level(level(*severity)))?;
    }
    if w.opts.all_scalars {
        for (topic, table) in &ulog.tables {
            for (field, column) in &table.columns {
                for (ts, v) in table.timestamps.iter().zip(column) {
                    if v.is_finite() {
                        w.row(&format!("timeseries/{topic}/{field}"), w.at(*ts), &Scalars::single(*v))?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// The vehicle's estimated pose, its body axes, and the flown trail.
///
/// `vehicle` carries the position alone and `vehicle/body` the attitude, so a view with `vehicle` as
/// its origin follows the vehicle with the world's axes: a chase view. The trail sits in the world.
fn pose(ulog: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    let (Some(pos), Some(att)) = (ulog.tables.get("vehicle_local_position"), ulog.tables.get("vehicle_attitude")) else {
        return Ok(());
    };
    let (Some(x), Some(y), Some(z)) = (pos.get("x"), pos.get("y"), pos.get("z")) else { return Ok(()) };
    let q: Vec<&[f64]> = (0..4).filter_map(|i| att.get(&format!("q[{i}]"))).collect();
    if q.len() != 4 || att.timestamps.is_empty() {
        return Ok(());
    }
    w.static_row(
        "vehicle/body/frd",
        &Arrows3D::from_vectors([[0.3, 0.0, 0.0], [0.0, -0.3, 0.0], [0.0, 0.0, -0.3]])
            .with_colors([Color::from_rgb(255, 0, 0), Color::from_rgb(0, 255, 0), Color::from_rgb(0, 0, 255)]),
    )?;
    // A marker sized in screen points, so the vehicle stays visible over a flight of any size
    w.static_row(
        "vehicle/body/marker",
        &Points3D::new([[0.0, 0.0, 0.0]])
            .with_radii([Radius::new_ui_points(5.0)])
            .with_colors([Color::from_rgb(255, 255, 255)])
    ,
    )?;
    let mut trail = Vec::with_capacity(pos.timestamps.len());
    let mut j = 0;
    for (i, ts) in pos.timestamps.iter().enumerate() {
        if !(x[i].is_finite() && y[i].is_finite() && z[i].is_finite()) {
            continue;
        }
        // The attitude sample nearest in time.
        while j + 1 < att.timestamps.len() && att.timestamps[j + 1].abs_diff(*ts) <= att.timestamps[j].abs_diff(*ts) {
            j += 1;
        }
        let p = ned_to_zup(x[i], y[i], z[i]);
        let rot = ned_quat_to_zup(q[0][j], q[1][j], q[2][j], q[3][j]);
        w.row("vehicle", w.at(*ts), &Transform3D::from_translation(p))?;
        w.row("vehicle/body", w.at(*ts), &Transform3D::from_rotation(rot))?;
        trail.push(p);
        if let (Some(vx), Some(vy), Some(vz)) = (pos.get("vx"), pos.get("vy"), pos.get("vz")) {
            // 10 m/s draws as long as the body axes, 0.3 m.
            let v = ned_to_zup(vx[i] * 0.03, vy[i] * 0.03, vz[i] * 0.03);
            let speed = (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i]).sqrt();
            let arrow = Arrows3D::from_vectors([v])
                .with_origins([[0.0, 0.0, 0.0]])
                .with_colors([Color::from_rgb(0, 255, 255)])
                .with_labels([format!("{speed:.1} m/s")]);
            w.row("vehicle/velocity", w.at(*ts), &arrow)?;
        }
    }
    w.static_row("trail", &LineStrips3D::new([trail]).with_colors([Color::from_unmultiplied_rgba(255, 255, 255, 200)]))?;
    Ok(())
}

/// The recording of `ulog` as RRD bytes, in the store the options name.
pub fn convert(ulog: &[u8], opts: &Options) -> Result<Vec<u8>, Error> {
    let parsed = parse(ulog)?;
    let mut writer = Writer::new(opts);
    visualize(&parsed, &mut writer)?;
    let chunks = writer.finish()?;

    let store_id = StoreId::recording(ApplicationId::new_or_unknown(opts.application_id.clone()), opts.recording_id.clone());
    let mut msgs = vec![LogMsg::SetStoreInfo(SetStoreInfo {
        row_id: *RowId::new(),
        info: StoreInfo::new(store_id.clone(), StoreSource::Other(format!("px4-rerun {VERSION}"))),
    })];
    for chunk in chunks {
        msgs.push(LogMsg::ArrowMsg(store_id.clone(), chunk.to_arrow_msg().map_err(err)?));
    }
    Encoder::encode(msgs.into_iter().map(Ok)).map_err(err)
}
