//! A recording back into the PX4 logs it holds: each log's topics, text messages and info.
//!
//! A file may hold several recordings, and each is read on its own. A log is wherever a recording
//! holds `header/timestamp_us`, which every conversion writes; what comes before it is the prefix the
//! log was logged under. The rest of the file, a blueprint or another process's entities, is left
//! out.

use std::collections::BTreeMap;
use std::io::Cursor;

use re_chunk::Chunk;
use re_log_encoding::Decoder;
use re_log_msg::LogMsg;
use re_log_types::StoreKind;
use re_sdk_types::archetypes::{Scalars, SeriesLines, TextLog};
use re_sdk_types::external::arrow::array::{
    Array, ArrayRef, AsArray as _, BinaryArray, BooleanArray, Float32Array, Float64Array, Int16Array, Int32Array,
    Int64Array, Int8Array, StringArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use re_sdk_types::external::arrow::datatypes::DataType;
use re_sdk_types::ComponentIdentifier;

use crate::{err, info, Error, InfoValue};

/// One PX4 log in a recording, found under the prefix it was logged with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    /// The recording the log sits in.
    pub recording_id: String,
    /// The entity path prefix the log sits under; `None` at the root.
    pub prefix: Option<String>,
    /// By name, then instance.
    pub topics: Vec<Topic>,
    pub messages: Vec<Message>,
    /// The info messages by key, each key's values in log order.
    pub info: BTreeMap<String, Vec<InfoValue>>,
}

/// One topic instance's numeric fields as time series.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Topic {
    pub name: String,
    /// The ULog multi id: 0 for the first instance.
    pub instance: u8,
    /// Each sample's time on the recording's timeline, in microseconds.
    pub timestamps_us: Vec<i64>,
    /// The fields in definition order, each with one value per sample.
    pub columns: Vec<(String, Vec<f64>)>,
}

/// A text message PX4 logged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message {
    pub timestamp_us: i64,
    /// The syslog severity, 0 (emergency) to 7 (debug).
    pub level: u8,
    pub text: String,
}

/// The PX4 logs in the `.rrd` bytes `rrd`, one per recording and prefix.
pub fn read(rrd: &[u8]) -> Result<Vec<Log>, Error> {
    // By recording id, then entity path.
    let mut recordings: BTreeMap<String, BTreeMap<String, Vec<Chunk>>> = BTreeMap::new();
    for msg in Decoder::<LogMsg>::decode_lazy(Cursor::new(rrd)) {
        if let LogMsg::ArrowMsg(store, msg) = msg.map_err(err)? {
            if store.kind() == StoreKind::Recording {
                let chunk = Chunk::from_arrow_msg(&msg).map_err(err)?;
                let entities = recordings.entry(store.recording_id().to_string()).or_default();
                entities.entry(chunk.entity_path().to_string()).or_default().push(chunk);
            }
        }
    }
    if recordings.is_empty() {
        return Err(Error("the bytes hold no recording".to_owned()));
    }
    recordings
        .iter()
        .flat_map(|(recording_id, entities)| {
            entities
                .keys()
                .filter_map(|path| path.strip_suffix("/header/timestamp_us"))
                .map(|base| log(recording_id, entities, base))
        })
        .collect()
}

/// The log whose entities sit under `base`, the prefix as an entity path: empty at the root, else
/// with its leading slash.
fn log(recording_id: &str, entities: &BTreeMap<String, Vec<Chunk>>, base: &str) -> Result<Log, Error> {
    let under = |dir: &str| {
        let start = format!("{base}/{dir}/");
        entities.iter().filter_map(move |(path, chunks)| Some((path.strip_prefix(&start)?, chunks)))
    };
    let mut topics = Vec::new();
    for (rest, chunks) in under("timeseries") {
        if let Some((name, instance)) = rest.split_once('/').and_then(|(n, i)| Some((n, i.parse().ok()?))) {
            topics.push(topic(name, instance, chunks)?);
        }
    }
    topics.sort_by(|a, b| (&a.name, a.instance).cmp(&(&b.name, b.instance)));
    let mut info = BTreeMap::new();
    for (key, chunks) in under("info") {
        let values = column(chunks, info::component(info::INFO, info::INFO_VALUE))
            .flat_map(|(_, row)| info_values(&row))
            .collect();
        info.insert(key.to_owned(), values);
    }
    Ok(Log {
        recording_id: recording_id.to_owned(),
        prefix: base.strip_prefix('/').map(str::to_owned),
        topics,
        messages: entities.get(&format!("{base}/log")).map_or_else(|| Ok(Vec::new()), |c| messages(c))?,
        info,
    })
}

/// Each row of `component` in `chunks`, in order: the row's time in nanoseconds, `None` when
/// static, and its instances.
fn column(chunks: &[Chunk], component: ComponentIdentifier) -> impl Iterator<Item = (Option<i64>, ArrayRef)> + '_ {
    chunks.iter().flat_map(move |chunk| {
        let times = chunk.timelines().values().next().map(|t| t.times_raw());
        let rows = chunk.components().get(component).map(|c| &c.list_array);
        rows.into_iter().flat_map(move |rows| (0..rows.len()).map(move |i| (times.map(|t| t[i]), rows.value(i))))
    })
}

fn topic(name: &str, instance: u8, chunks: &[Chunk]) -> Result<Topic, Error> {
    // A static row replaces the one before it, as in the viewer, so a log appended twice still
    // names its fields once.
    let names: Vec<String> = column(chunks, SeriesLines::descriptor_names().component)
        .filter(|(time, _)| time.is_none())
        .last()
        .map_or_else(Vec::new, |(_, row)| row.as_string::<i32>().iter().flatten().map(str::to_owned).collect());
    let mut columns: Vec<(String, Vec<f64>)> = names.into_iter().map(|n| (n, Vec::new())).collect();
    let mut timestamps_us = Vec::new();
    for (time, row) in column(chunks, Scalars::descriptor_scalars().component) {
        let values = row.as_any().downcast_ref::<Float64Array>().ok_or_else(|| Error(format!("{name}: not f64")))?;
        if values.len() != columns.len() {
            return Err(Error(format!("{name}: a sample of {} values for {} fields", values.len(), columns.len())));
        }
        timestamps_us.push(time.unwrap_or(0) / 1000);
        for ((_, column), value) in columns.iter_mut().zip(values.values()) {
            column.push(*value);
        }
    }
    Ok(Topic { name: name.to_owned(), instance, timestamps_us, columns })
}

fn messages(chunks: &[Chunk]) -> Result<Vec<Message>, Error> {
    let texts = column(chunks, TextLog::descriptor_text().component);
    let levels = column(chunks, info::component(info::MESSAGE, info::MESSAGE_SEVERITY));
    texts
        .zip(levels)
        .map(|((time, text), (_, level))| {
            let text = text.as_string::<i32>().iter().flatten().next().unwrap_or_default().to_owned();
            let level = level.as_any().downcast_ref::<UInt8Array>().and_then(|l| l.iter().flatten().next());
            Ok(Message {
                timestamp_us: time.unwrap_or(0) / 1000,
                level: level.ok_or_else(|| err("no severity"))?,
                text,
            })
        })
        .collect()
}

/// The values of one info row, in the type the ULog gave them.
fn info_values(row: &ArrayRef) -> Vec<InfoValue> {
    macro_rules! of {
        ($variant:ident, $array:ty) => {
            row.as_any()
                .downcast_ref::<$array>()
                .map_or_else(Vec::new, |a| a.iter().flatten().map(|v| InfoValue::$variant(v.to_owned())).collect())
        };
    }
    match row.data_type() {
        DataType::Utf8 => row
            .as_any()
            .downcast_ref::<StringArray>()
            .map_or_else(Vec::new, |a| a.iter().flatten().map(|v| InfoValue::Text(v.to_owned())).collect()),
        DataType::Binary => row
            .as_any()
            .downcast_ref::<BinaryArray>()
            .map_or_else(Vec::new, |a| a.iter().flatten().map(|v| InfoValue::Bytes(v.to_owned())).collect()),
        DataType::Boolean => of!(Bool, BooleanArray),
        DataType::UInt8 => of!(U8, UInt8Array),
        DataType::UInt16 => of!(U16, UInt16Array),
        DataType::UInt32 => of!(U32, UInt32Array),
        DataType::UInt64 => of!(U64, UInt64Array),
        DataType::Int8 => of!(I8, Int8Array),
        DataType::Int16 => of!(I16, Int16Array),
        DataType::Int32 => of!(I32, Int32Array),
        DataType::Int64 => of!(I64, Int64Array),
        DataType::Float32 => of!(F32, Float32Array),
        DataType::Float64 => of!(F64, Float64Array),
        _ => Vec::new(),
    }
}
