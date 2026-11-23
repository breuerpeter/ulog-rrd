//! A recording back into its PX4 logs, each answering the methods of Pulsar's `ParserLike`, so Pulsar
//! opens a log from a recording as it opens a ULog. The shapes are Pulsar's: times in microseconds,
//! numbers as `Float64Array`, and an array field `q[0]` named `q_0`. A method with no data in a
//! recording answers an empty value and never throws.

use js_sys::{Array, BigInt, Date, Float64Array, Map, Object, Reflect, Uint8Array};
use ulog_rrd::InfoValue;
use wasm_bindgen::prelude::*;

/// One PX4 log in a recording, under the prefix it was logged with.
#[wasm_bindgen]
pub struct Log(ulog_rrd::Log);

/// The PX4 logs in `.rrd` bytes, one per recording and prefix.
#[wasm_bindgen]
pub fn read(rrd: &[u8]) -> Result<Vec<Log>, JsError> {
    Ok(ulog_rrd::read(rrd).map_err(|e| JsError::new(&e.0))?.into_iter().map(Log).collect())
}

fn set(obj: &Object, key: &str, value: &JsValue) {
    // Setting a key on a plain object does not fail.
    let _ = Reflect::set(obj, &JsValue::from_str(key), value);
}

/// A field as Pulsar names it: each array index `[i]` as `_i`.
fn field_name(name: &str) -> String {
    name.replace('[', "_").replace(']', "")
}

/// The base names of the array fields, as Pulsar names them, in field order.
fn array_fields(topic: &ulog_rrd::Topic) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, _) in &topic.columns {
        if let Some(base) = name.strip_suffix(']').and_then(|n| n.rsplit_once('[')).map(|(base, _)| field_name(base)) {
            if !out.contains(&base) {
                out.push(base);
            }
        }
    }
    out
}

fn strings(values: impl IntoIterator<Item = String>) -> Array {
    values.into_iter().map(|v| JsValue::from_str(&v)).collect()
}

fn topic_info(topic: &ulog_rrd::Topic) -> JsValue {
    let obj = Object::new();
    set(&obj, "name", &JsValue::from_str(&topic.name));
    set(&obj, "messageCount", &JsValue::from_f64(topic.timestamps_us.len() as f64));
    set(&obj, "multiId", &JsValue::from_f64(topic.instance.into()));
    set(&obj, "fields", &strings(topic.columns.iter().map(|(name, _)| field_name(name))));
    let arrays = array_fields(topic);
    if !arrays.is_empty() {
        set(&obj, "arrayFields", &strings(arrays));
    }
    obj.into()
}

fn times(topic: &ulog_rrd::Topic) -> Vec<f64> {
    topic.timestamps_us.iter().map(|&t| t as f64).collect()
}

fn topic_data(topic: Option<&ulog_rrd::Topic>) -> JsValue {
    let obj = Object::new();
    let columns = Object::new();
    let timestamp = topic.map_or_else(Vec::new, times);
    for (name, values) in topic.iter().flat_map(|t| &t.columns) {
        set(&columns, &field_name(name), &Float64Array::from(values.as_slice()));
    }
    set(&obj, "timestamp", &Float64Array::from(timestamp.as_slice()));
    set(&obj, "columns", &columns);
    obj.into()
}

impl Log {
    fn topic(&self, name: &str, multi_id: Option<u8>) -> Option<&ulog_rrd::Topic> {
        let instance = multi_id.unwrap_or(0);
        self.0.topics.iter().find(|t| t.name == name && t.instance == instance)
    }
}

#[wasm_bindgen]
impl Log {
    /// The recording the log sits in.
    #[wasm_bindgen(getter, js_name = recordingId)]
    pub fn recording_id(&self) -> String {
        self.0.recording_id.clone()
    }

    /// The entity path prefix the log sits under; unset at the root.
    #[wasm_bindgen(getter)]
    pub fn prefix(&self) -> Option<String> {
        self.0.prefix.clone()
    }

    /// `{ metadata, topics }`. The metadata holds the span of the series; the rest of it, which Pulsar
    /// draws from the parameters and the info, is left empty.
    #[wasm_bindgen(js_name = parseHeader)]
    pub fn parse_header(&self) -> JsValue {
        let all = self.0.topics.iter().flat_map(|t| t.timestamps_us.iter().copied());
        let (start, end) = all.fold((i64::MAX, i64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let (start, end) = if start > end { (0, 0) } else { (start, end) };
        let metadata = Object::new();
        set(&metadata, "duration", &JsValue::from_f64((end - start) as f64 / 1e6));
        set(&metadata, "startTimestampUs", &JsValue::from_f64(start as f64));
        for key in ["vehicleType", "firmwareVersion", "airframe"] {
            set(&metadata, key, &JsValue::from_str(""));
        }
        set(&metadata, "timestamp", &Date::new(&JsValue::from_f64(0.0)));
        let obj = Object::new();
        set(&obj, "metadata", &metadata);
        set(&obj, "topics", &self.0.topics.iter().map(topic_info).collect::<Array>());
        obj.into()
    }

    /// `{ timestamp, columns }`, empty for a topic the log does not hold.
    #[wasm_bindgen(js_name = getTopicData)]
    pub fn get_topic_data(&self, name: &str, multi_id: Option<u8>) -> JsValue {
        topic_data(self.topic(name, multi_id))
    }

    /// `Map<string, TopicData | null>`, keyed by requested name.
    #[wasm_bindgen(js_name = getTopicsData)]
    pub fn get_topics_data(&self, names: Vec<String>, multi_id: Option<u8>) -> Map {
        let map = Map::new();
        for name in names {
            let data = self.topic(&name, multi_id).map_or(JsValue::NULL, |t| topic_data(Some(t)));
            map.set(&JsValue::from_str(&name), &data);
        }
        map
    }

    #[wasm_bindgen(js_name = hasTopic)]
    pub fn has_topic(&self, name: &str, multi_id: Option<u8>) -> bool {
        self.topic(name, multi_id).is_some()
    }

    /// `[{ timestamp, level, message }]`, the level the syslog severity.
    #[wasm_bindgen(js_name = getLoggedMessages)]
    pub fn get_logged_messages(&self) -> Array {
        self.0
            .messages
            .iter()
            .map(|m| {
                let obj = Object::new();
                set(&obj, "timestamp", &JsValue::from_f64(m.timestamp_us as f64));
                set(&obj, "level", &JsValue::from_f64(m.level.into()));
                set(&obj, "message", &JsValue::from_str(&m.text));
                JsValue::from(obj)
            })
            .collect()
    }

    /// `[{ timestamp, id, logLevel, args: Uint8Array }]`, from the `event` topic: the external log
    /// level, the high four bits of `log_levels`, and the argument bytes.
    #[wasm_bindgen(js_name = getEvents)]
    pub fn get_events(&self) -> Array {
        let events = Array::new();
        for topic in self.0.topics.iter().filter(|t| t.name == "event") {
            let field = |name: &str| topic.columns.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_slice());
            let (Some(ids), Some(levels)) = (field("id"), field("log_levels")) else { continue };
            let args: Vec<&[f64]> = (0..).map_while(|i| field(&format!("arguments[{i}]"))).collect();
            for (row, &time) in topic.timestamps_us.iter().enumerate() {
                let obj = Object::new();
                set(&obj, "timestamp", &JsValue::from_f64(time as f64));
                set(&obj, "id", &JsValue::from_f64(ids[row]));
                set(&obj, "logLevel", &JsValue::from_f64(((levels[row] as u32 >> 4) & 0xf).into()));
                let bytes: Vec<u8> = args.iter().map(|a| a[row] as u8).collect();
                set(&obj, "args", &Uint8Array::from(bytes.as_slice()));
                events.push(&obj);
            }
        }
        events
    }

    /// `{ [name]: value }`, each parameter's last value.
    #[wasm_bindgen(js_name = getParameters)]
    pub fn get_parameters(&self) -> Object {
        let obj = Object::new();
        for (name, rows) in &self.0.params {
            if let Some(&(_, value)) = rows.last() {
                set(&obj, name, &JsValue::from_f64(value));
            }
        }
        obj
    }

    /// `{ [name]: [{ value, timestamp }] }` for each parameter that changed in flight or was first
    /// logged in flight, as Pulsar's parser answers it: the start value at timestamp 0, then each
    /// change at its time in µs. The recording does not say where the definitions section ends, so a
    /// first row is the start value when it sits at the earliest time of any parameter, where every
    /// row from the definitions section sits.
    #[wasm_bindgen(js_name = getChangedParameters)]
    pub fn get_changed_parameters(&self) -> Object {
        let params = &self.0.params;
        let start = params.values().filter_map(|rows| rows.first()).map(|&(time, _)| time).min();
        let obj = Object::new();
        for (name, rows) in params {
            let at_start = rows.first().map(|&(time, _)| time) == start;
            if at_start && rows.len() < 2 {
                continue;
            }
            let entries: Array = rows
                .iter()
                .enumerate()
                .map(|(i, &(time, value))| {
                    let entry = Object::new();
                    set(&entry, "value", &JsValue::from_f64(value));
                    set(&entry, "timestamp", &JsValue::from_f64(if i == 0 && at_start { 0.0 } else { time as f64 }));
                    JsValue::from(entry)
                })
                .collect();
            set(&obj, name, &entries);
        }
        obj
    }

    /// The parameter's last value; `undefined` for one the log does not hold.
    #[wasm_bindgen(js_name = getParameter)]
    pub fn get_parameter(&self, name: &str) -> Option<f64> {
        self.0.params.get(name)?.last().map(|&(_, value)| value)
    }

    /// `Map<string, string | number | bigint>`: each info key's first value, bytes left out.
    #[wasm_bindgen(js_name = getInformation)]
    pub fn get_information(&self) -> Map {
        let map = Map::new();
        for (key, values) in &self.0.info {
            let value = match values.first() {
                Some(InfoValue::Text(v)) => JsValue::from_str(v),
                Some(InfoValue::Bool(v)) => JsValue::from_f64(f64::from(u8::from(*v))),
                Some(InfoValue::U8(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::U16(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::U32(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::I8(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::I16(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::I32(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::F32(v)) => JsValue::from_f64((*v).into()),
                Some(InfoValue::F64(v)) => JsValue::from_f64(*v),
                Some(InfoValue::U64(v)) => BigInt::from(*v).into(),
                Some(InfoValue::I64(v)) => BigInt::from(*v).into(),
                Some(InfoValue::Bytes(_)) | None => continue,
            };
            map.set(&JsValue::from_str(key), &value);
        }
        map
    }

    /// The firmware's event definitions, the `metadata_events` info, as `Uint8Array`, or `null`.
    #[wasm_bindgen(js_name = getEventMetadataBlob)]
    pub fn get_event_metadata_blob(&self) -> JsValue {
        match self.0.info.get("metadata_events").and_then(|v| v.first()) {
            Some(InfoValue::Bytes(b)) => Uint8Array::from(b.as_slice()).into(),
            _ => JsValue::NULL,
        }
    }
}
