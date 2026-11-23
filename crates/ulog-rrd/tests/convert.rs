//! A small ULog written by hand, converted, and read back with Rerun's decoder.

use std::collections::BTreeMap;
use std::io::Cursor;

use re_chunk::Chunk;
use re_log_encoding::Decoder;
use re_log_msg::LogMsg;
use ulog_rrd::{convert, Options};

/// A ULog writer: the header, then each message with its size and type.
struct Ulog(Vec<u8>);

impl Ulog {
    fn new(start_us: u64) -> Self {
        let mut bytes = b"ULog\x01\x12\x35\x01".to_vec();
        bytes.extend_from_slice(&start_us.to_le_bytes());
        let mut ulog = Self(bytes);
        ulog.message(b'B', &[0u8; 40]); // flag bits, which must come first
        ulog
    }

    fn message(&mut self, kind: u8, payload: &[u8]) {
        self.0.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        self.0.push(kind);
        self.0.extend_from_slice(payload);
    }

    fn format(&mut self, definition: &str) {
        self.message(b'F', definition.as_bytes());
    }

    fn subscribe(&mut self, msg_id: u16, multi_id: u8, name: &str) {
        let mut payload = vec![multi_id];
        payload.extend_from_slice(&msg_id.to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
        self.message(b'A', &payload);
    }

    fn data(&mut self, msg_id: u16, timestamp_us: u64, floats: &[f32]) {
        let mut payload = msg_id.to_le_bytes().to_vec();
        payload.extend_from_slice(&timestamp_us.to_le_bytes());
        floats.iter().for_each(|f| payload.extend_from_slice(&f.to_le_bytes()));
        self.message(b'D', &payload);
    }

    fn text(&mut self, level: u8, timestamp_us: u64, text: &str) {
        let mut payload = vec![b'0' + level];
        payload.extend_from_slice(&timestamp_us.to_le_bytes());
        payload.extend_from_slice(text.as_bytes());
        self.message(b'L', &payload);
    }

    /// An info message: `key` is the type and the name, as in `char[3] sys_name`.
    fn info(&mut self, key: &str, value: &[u8]) {
        let mut payload = vec![key.len() as u8];
        payload.extend_from_slice(key.as_bytes());
        payload.extend_from_slice(value);
        self.message(b'I', &payload);
    }

    /// A multi-part info message; `is_continued` joins it to the one before it.
    fn info_multi(&mut self, is_continued: bool, key: &str, value: &[u8]) {
        let mut payload = vec![is_continued as u8, key.len() as u8];
        payload.extend_from_slice(key.as_bytes());
        payload.extend_from_slice(value);
        self.message(b'M', &payload);
    }
}

/// A flight of three position samples north-east-down (1, 2, -3), (2, 2, -3), (3, 2, -3) with an
/// identity attitude, one home position, one warning, and a stray sample far outside the span.
fn flight() -> Vec<u8> {
    let mut ulog = Ulog::new(1_000_000);
    ulog.format("vehicle_local_position:uint64_t timestamp;float x;float y;float z;float vx;float vy;float vz;");
    ulog.format("vehicle_attitude:uint64_t timestamp;float[4] q;");
    ulog.format("home_position:uint64_t timestamp;float x;float y;float z;");
    ulog.subscribe(0, 0, "vehicle_local_position");
    ulog.subscribe(1, 0, "vehicle_attitude");
    ulog.subscribe(2, 0, "home_position");
    ulog.data(1, 1_000_000, &[1.0, 0.0, 0.0, 0.0]);
    ulog.data(2, 1_000_000, &[0.0, 0.0, 0.0]);
    for (i, x) in [1.0f32, 2.0, 3.0].into_iter().enumerate() {
        ulog.data(0, 1_000_000 + i as u64 * 100_000, &[x, 2.0, -3.0, 10.0, 0.0, 0.0]);
    }
    ulog.data(0, u32::MAX as u64, &[9.0, 9.0, 9.0, 0.0, 0.0, 0.0]);
    ulog.text(4, 1_050_000, "[commander] Takeoff detected");
    ulog.0
}

/// The decoded chunks by entity path, each with its rows in order.
fn chunks(rrd: &[u8]) -> BTreeMap<String, Vec<Chunk>> {
    let mut out: BTreeMap<String, Vec<Chunk>> = BTreeMap::new();
    for msg in Decoder::<LogMsg>::decode_lazy(Cursor::new(rrd)) {
        if let LogMsg::ArrowMsg(_, msg) = msg.expect("decode") {
            let chunk = Chunk::from_arrow_msg(&msg).expect("chunk");
            out.entry(chunk.entity_path().to_string()).or_default().push(chunk);
        }
    }
    out
}

fn rows(chunks: &BTreeMap<String, Vec<Chunk>>, path: &str) -> usize {
    chunks.get(path).map_or(0, |c| c.iter().map(Chunk::num_rows).sum())
}

/// One value of an info row, as the Arrow type of its instance says: text, a number or bytes.
#[derive(Debug, PartialEq)]
enum Value {
    Text(String),
    U32(u32),
    I32(i32),
    U64(u64),
    Bytes(Vec<u8>),
}

/// The values at `path` in row order, one per instance; empty when nothing is logged there.
fn values(chunks: &BTreeMap<String, Vec<Chunk>>, path: &str) -> Vec<Value> {
    use re_sdk_types::external::arrow::array::{
        Array, BinaryArray, FixedSizeListArray, Int32Array, ListArray, StringArray, UInt32Array, UInt64Array,
        UInt8Array,
    };
    use re_sdk_types::external::arrow::datatypes::DataType;

    let mut out = Vec::new();
    for chunk in chunks.get(path).into_iter().flatten() {
        let columns: Vec<_> = chunk.components().0.values().collect();
        assert_eq!(columns.len(), 1, "an info row is one component: {path}");
        let rows = &columns[0].list_array;
        for row in 0..rows.len() {
            let instances = rows.value(row);
            let any = instances.as_any();
            match instances.data_type() {
                DataType::Utf8 => out
                    .extend(any.downcast_ref::<StringArray>().unwrap().iter().flatten().map(|s| Value::Text(s.into()))),
                DataType::UInt32 => {
                    out.extend(any.downcast_ref::<UInt32Array>().unwrap().iter().flatten().map(Value::U32))
                }
                DataType::Int32 => {
                    out.extend(any.downcast_ref::<Int32Array>().unwrap().iter().flatten().map(Value::I32))
                }
                DataType::UInt64 => {
                    out.extend(any.downcast_ref::<UInt64Array>().unwrap().iter().flatten().map(Value::U64))
                }
                DataType::Binary => out.extend(
                    any.downcast_ref::<BinaryArray>().unwrap().iter().flatten().map(|b| Value::Bytes(b.into())),
                ),
                DataType::List(_) => out.extend((0..instances.len()).map(|i| {
                    let bytes = any.downcast_ref::<ListArray>().unwrap().value(i);
                    Value::Bytes(bytes.as_any().downcast_ref::<UInt8Array>().expect("bytes").values().to_vec())
                })),
                DataType::FixedSizeList(..) => out.extend((0..instances.len()).map(|i| {
                    let bytes = any.downcast_ref::<FixedSizeListArray>().unwrap().value(i);
                    Value::Bytes(bytes.as_any().downcast_ref::<UInt8Array>().expect("bytes").values().to_vec())
                })),
                other => panic!("an info value of Arrow type {other} at {path}"),
            }
        }
    }
    out
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn the_vehicle_flies_in_px4s_frame_and_the_stray_sample_is_dropped() {
    let rrd = convert(&flight(), &Options::default()).expect("convert");
    let chunks = chunks(&rrd);
    assert_eq!(rows(&chunks, "/vehicle"), 3, "{:?}", chunks.keys());
    assert_eq!(rows(&chunks, "/vehicle/body"), 3);
    assert_eq!(rows(&chunks, "/vehicle/velocity"), 4, "three samples and the static colour");
    assert_eq!(rows(&chunks, "/home"), 2, "one sample and the static marker");
    assert_eq!(rows(&chunks, "/log"), 1);
    assert_eq!(rows(&chunks, "/trail"), 1);
    assert!(chunks.contains_key("/"), "the root declares the frame");
    assert!(!chunks.contains_key("/timeseries/vehicle_local_position"), "scalars are opt in");

    // North 1, east 2, down -3 stays as it is; its time is ULog time.
    let vehicle = &chunks["/vehicle"][0];
    let time = vehicle.timelines().values().next().expect("one timeline");
    assert_eq!(time.timeline().name().as_str(), "time");
    assert_eq!(time.times_raw()[0], 1_000_000_000);
    let translation = vehicle.components().0.values().next().expect("translation").list_array.clone();
    let first = re_sdk_types::external::arrow::array::Array::slice(&translation, 0, 1);
    assert!(format!("{first:?}").replace(['\n', ' '], "").contains("[1.0,2.0,-3.0,]"), "{first:?}");
}

#[test]
fn a_prefix_moves_every_entity_and_carries_the_frame() {
    let opts = Options {
        entity_path_prefix: Some("px4_sitl/logs".into()),
        timeline: "sim".into(),
        time_offset_us: 1_000_000,
        scalars: true,
        rotation: Some([1.0, 0.0, 0.0, 0.0]),
        ..Options::default()
    };
    let chunks = chunks(&convert(&flight(), &opts).expect("convert"));
    assert!(chunks.keys().all(|k| k.starts_with("/px4_sitl/logs")), "{:?}", chunks.keys());
    assert_eq!(rows(&chunks, "/px4_sitl/logs"), 1, "the root's transform");
    assert_eq!(rows(&chunks, "/px4_sitl/logs/timeseries/vehicle_local_position/0"), 4, "three rows and the names");
    let vehicle = &chunks["/px4_sitl/logs/vehicle"][0];
    let time = vehicle.timelines().values().next().expect("one timeline");
    assert_eq!(time.timeline().name().as_str(), "sim");
    assert_eq!(time.times_raw()[0], 0, "the offset is the caller's time zero");
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert!(convert(b"not a ulog", &Options::default()).is_err());
}

#[cfg(feature = "append")]
#[test]
fn append_rewrites_a_recording_with_the_log_added_under_one_footer() {
    use re_chunk::{Chunk, RowId, TimePoint};
    use re_log_encoding::Encoder;
    use re_log_msg::{SetStoreInfo, StoreInfo, StoreSource};
    use re_log_types::StoreId;
    use re_sdk_types::archetypes::Points3D;

    // A recording another process wrote: one store, one static chunk.
    let store_id = StoreId::recording("app", "run");
    let info = StoreInfo::new(store_id.clone(), StoreSource::Unknown);
    let chunk = Chunk::builder("sim/vehicle/body")
        .with_archetype(RowId::new(), TimePoint::default(), &Points3D::new([[0.0, 0.0, 1.0]]))
        .build()
        .expect("chunk");
    let file = Encoder::encode(
        [
            LogMsg::SetStoreInfo(SetStoreInfo { row_id: *RowId::new(), info }),
            LogMsg::ArrowMsg(store_id, chunk.to_arrow_msg().expect("arrow")),
        ]
        .into_iter()
        .map(Ok),
    )
    .expect("encode");
    let dir = std::env::temp_dir().join(format!("ulog-rrd-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let rrd = dir.join("run.rrd");
    std::fs::write(&rrd, file).expect("write");

    let opts = Options {
        application_id: "app".into(),
        recording_id: Some("run".into()),
        entity_path_prefix: Some("px4_sitl/logs".into()),
        ..Options::default()
    };
    ulog_rrd::append(&flight(), &opts, &rrd).expect("append");

    let bytes = std::fs::read(&rrd).expect("read");
    let chunks = chunks(&bytes);
    assert_eq!(rows(&chunks, "/sim/vehicle/body"), 1, "the file's own rows stay");
    assert_eq!(rows(&chunks, "/px4_sitl/logs/vehicle"), 3);
    let stores: Vec<_> = Decoder::<LogMsg>::decode_lazy(Cursor::new(bytes.as_slice()))
        .filter(|m| matches!(m, Ok(LogMsg::SetStoreInfo(_))))
        .collect();
    assert_eq!(stores.len(), 1, "the file says what its store is once");
    assert!(!dir.join("run.rrd.tmp").exists());
    std::fs::remove_dir_all(dir).ok();
}

/// A plain info message is held under its key with its value.
#[test]
fn a_plain_info_message_is_held_under_its_key() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info("char[3] sys_name", b"PX4");
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    assert_eq!(values(&chunks, "/info/sys_name"), [text("PX4")]);
}

/// Each ULog type keeps its type and value.
#[test]
fn each_info_type_keeps_its_type_and_value() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info("char[3] sys_name", b"PX4");
    ulog.info("uint32_t ver_sw_release", &0x0102_0300u32.to_le_bytes());
    ulog.info("int32_t time_ref_utc", &(-3600i32).to_le_bytes());
    ulog.info("uint8_t[3] blob", &[7, 8, 9]);
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    let held: Vec<_> = ["sys_name", "ver_sw_release", "time_ref_utc", "blob"]
        .into_iter()
        .map(|key| values(&chunks, &format!("/info/{key}")))
        .collect();
    assert_eq!(
        held,
        [vec![text("PX4")], vec![Value::U32(0x0102_0300)], vec![Value::I32(-3600)], vec![Value::Bytes(vec![7, 8, 9])],]
    );
}

/// A `uint64_t` above 2^53 keeps every digit.
#[test]
fn a_uint64_above_2_pow_53_keeps_every_digit() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info("uint64_t hw_id", &9_007_199_254_740_993u64.to_le_bytes());
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    assert_eq!(values(&chunks, "/info/hw_id"), [Value::U64(9_007_199_254_740_993)]);
}

/// A multi-part value is one value, its messages joined in order.
#[test]
fn a_multi_part_value_is_one_value_joined_in_order() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info_multi(false, "uint8_t[2] metadata_events", &[1, 2]);
    ulog.info_multi(true, "uint8_t[1] metadata_events", &[3]);
    ulog.info_multi(true, "uint8_t[2] metadata_events", &[4, 5]);
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    assert_eq!(values(&chunks, "/info/metadata_events"), [Value::Bytes(vec![1, 2, 3, 4, 5])]);
}

/// A key that repeats keeps each value, in log order.
#[test]
fn a_repeated_key_keeps_each_value_in_log_order() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info_multi(false, "char[1] excluded_optional_topics", b"a");
    ulog.info_multi(false, "char[1] excluded_optional_topics", b"b");
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    assert_eq!(values(&chunks, "/info/excluded_optional_topics"), [text("a"), text("b")]);
}

/// The array length in the type does not split a key.
#[test]
fn the_array_length_in_the_type_does_not_split_a_key() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info_multi(false, "char[3] k", b"abc");
    ulog.info_multi(false, "char[5] k", b"defgh");
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    let paths: Vec<_> = chunks.keys().map(String::as_str).filter(|k| k.starts_with("/info")).collect();
    assert_eq!((paths, values(&chunks, "/info/k")), (vec!["/info/k"], vec![text("abc"), text("defgh")]));
}

/// An info message after the first data message is held.
#[test]
fn an_info_message_after_the_first_data_message_is_held() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.format("home_position:uint64_t timestamp;float x;float y;float z;");
    ulog.subscribe(0, 0, "home_position");
    ulog.data(0, 1_000_000, &[0.0, 0.0, 0.0]);
    ulog.info_multi(false, "char[2] perf_counter_preflight", b"ab");
    let chunks = chunks(&convert(&ulog.0, &Options::default()).expect("convert"));
    assert_eq!(values(&chunks, "/info/perf_counter_preflight"), [text("ab")]);
}

/// The ULog file header's timestamp is held as one value in µs.
#[test]
fn the_file_headers_timestamp_is_held_in_microseconds() {
    let chunks = chunks(&convert(&Ulog::new(1_000_000).0, &Options::default()).expect("convert"));
    assert_eq!(values(&chunks, "/header/timestamp_us"), [Value::U64(1_000_000)]);
}

/// Info rows sit under the entity path prefix, as every row does.
#[test]
fn info_rows_sit_under_the_entity_path_prefix() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.info("char[3] sys_name", b"PX4");
    let opts = Options { entity_path_prefix: Some("a/logs".into()), ..Options::default() };
    let chunks = chunks(&convert(&ulog.0, &opts).expect("convert"));
    assert_eq!(
        (values(&chunks, "/a/logs/info/sys_name"), values(&chunks, "/info/sys_name")),
        (vec![text("PX4")], vec![])
    );
}

/// A log with no info messages still converts, with no info rows.
#[test]
fn a_log_with_no_info_messages_converts_with_no_info_rows() {
    let chunks = chunks(&convert(&flight(), &Options::default()).expect("convert"));
    assert!(!chunks.keys().any(|k| k.starts_with("/info")), "{:?}", chunks.keys());
}

// The reader: a recording back into the PX4 logs it holds.

fn scalars() -> Options {
    Options { scalars: true, ..Options::default() }
}

/// The topic `name`, instance `instance`, in any log `logs` answer.
fn topic<'a>(logs: &'a [ulog_rrd::Log], name: &str, instance: u8) -> Option<&'a ulog_rrd::Topic> {
    logs.iter().flat_map(|log| &log.topics).find(|t| t.name == name && t.instance == instance)
}

/// Each topic comes back with its field names in definition order, one value per field per sample,
/// and each sample's ULog time.
#[test]
fn each_topic_comes_back_with_its_fields_values_and_ulog_times() {
    let logs = ulog_rrd::read(&convert(&flight(), &scalars()).expect("convert")).expect("read");
    let got = topic(&logs, "vehicle_local_position", 0).map(|t| {
        let names: Vec<&str> = t.columns.iter().map(|(name, _)| name.as_str()).collect();
        (names, t.timestamps_us.clone(), t.columns[0].1.clone())
    });
    assert_eq!(
        got,
        Some((vec!["x", "y", "z", "vx", "vy", "vz"], vec![1_000_000, 1_100_000, 1_200_000], vec![1.0, 2.0, 3.0]))
    );
}

/// Two instances of a topic come back as one name with instances 0 and 1, and
/// `actuator_controls_status_1` comes back under that name as instance 0.
#[test]
fn an_instance_comes_back_as_its_name_and_multi_id() {
    let mut ulog = Ulog::new(1_000_000);
    ulog.format("sensor_mag:uint64_t timestamp;float x;");
    ulog.format("actuator_controls_status_1:uint64_t timestamp;float x;");
    ulog.subscribe(0, 0, "sensor_mag");
    ulog.subscribe(1, 1, "sensor_mag");
    ulog.subscribe(2, 0, "actuator_controls_status_1");
    for msg_id in 0..3 {
        ulog.data(msg_id, 1_000_000, &[1.0]);
        ulog.data(msg_id, 1_100_000, &[2.0]);
    }
    let logs = ulog_rrd::read(&convert(&ulog.0, &scalars()).expect("convert")).expect("read");
    let mut got: Vec<(&str, u8)> =
        logs.iter().flat_map(|log| &log.topics).map(|t| (t.name.as_str(), t.instance)).collect();
    got.sort();
    assert_eq!(got, [("actuator_controls_status_1", 0), ("sensor_mag", 0), ("sensor_mag", 1)]);
}

/// A recording holds each topic's series at `timeseries/<name>/<n>`, instance 0 included.
#[test]
fn a_topics_series_sit_at_its_name_and_instance() {
    let chunks = chunks(&convert(&flight(), &scalars()).expect("convert"));
    assert_eq!(
        (
            rows(&chunks, "/timeseries/vehicle_local_position/0") > 0,
            rows(&chunks, "/timeseries/vehicle_local_position")
        ),
        (true, 0)
    );
}

/// A recording another process wrote, with a series of its own at `sim/speed`, in a fresh directory
/// named for `test`.
#[cfg(feature = "append")]
fn recording_with_its_own_series(test: &str) -> std::path::PathBuf {
    use re_chunk::{RowId, TimePoint};
    use re_log_encoding::Encoder;
    use re_log_msg::{SetStoreInfo, StoreInfo, StoreSource};
    use re_log_types::StoreId;
    use re_sdk_types::archetypes::Scalars;

    let store_id = StoreId::recording("app", "run");
    let info = StoreInfo::new(store_id.clone(), StoreSource::Unknown);
    let chunk = Chunk::builder("sim/speed")
        .with_archetype(RowId::new(), TimePoint::default(), &Scalars::single(1.0))
        .build()
        .expect("chunk");
    let file = Encoder::encode(
        [
            LogMsg::SetStoreInfo(SetStoreInfo { row_id: *RowId::new(), info }),
            LogMsg::ArrowMsg(store_id, chunk.to_arrow_msg().expect("arrow")),
        ]
        .into_iter()
        .map(Ok),
    )
    .expect("encode");
    let dir = std::env::temp_dir().join(format!("ulog-rrd-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let rrd = dir.join("run.rrd");
    std::fs::write(&rrd, file).expect("write");
    rrd
}

/// `flight()` appended to the recording at `rrd` under `prefix`, with its series.
#[cfg(feature = "append")]
fn append_flight(rrd: &std::path::Path, prefix: &str) {
    let opts = Options {
        application_id: "app".into(),
        recording_id: Some("run".into()),
        entity_path_prefix: Some(prefix.into()),
        scalars: true,
        ..Options::default()
    };
    ulog_rrd::append(&flight(), &opts, rrd).expect("append");
}

/// The reader finds the prefix on its own and answers only the PX4 topics under it.
#[cfg(feature = "append")]
#[test]
fn the_reader_finds_the_prefix_and_answers_only_its_topics() {
    let rrd = recording_with_its_own_series("prefix");
    append_flight(&rrd, "px4_sitl/logs");
    let logs = ulog_rrd::read(&std::fs::read(&rrd).expect("read file")).expect("read");
    std::fs::remove_dir_all(rrd.parent().unwrap()).ok();
    let got: Vec<(Option<&str>, Vec<&str>)> =
        logs.iter().map(|log| (log.prefix.as_deref(), log.topics.iter().map(|t| t.name.as_str()).collect())).collect();
    assert_eq!(got, [(Some("px4_sitl/logs"), vec!["home_position", "vehicle_attitude", "vehicle_local_position"])]);
}

/// Two logs under two prefixes come back as two logs, each with its own topics.
#[cfg(feature = "append")]
#[test]
fn two_logs_under_two_prefixes_come_back_as_two_logs() {
    let rrd = recording_with_its_own_series("two-prefixes");
    append_flight(&rrd, "a/logs");
    append_flight(&rrd, "b/logs");
    let logs = ulog_rrd::read(&std::fs::read(&rrd).expect("read file")).expect("read");
    std::fs::remove_dir_all(rrd.parent().unwrap()).ok();
    let mut got: Vec<(Option<&str>, bool)> = logs
        .iter()
        .map(|log| (log.prefix.as_deref(), log.topics.iter().any(|t| t.name == "vehicle_local_position")))
        .collect();
    got.sort();
    assert_eq!(got, [(Some("a/logs"), true), (Some("b/logs"), true)]);
}

/// A recording with no series comes back with no topics and no error: the log is there, with its
/// messages and info, and no topics.
#[test]
fn a_recording_with_no_series_comes_back_with_no_topics() {
    let got = ulog_rrd::read(&convert(&flight(), &Options::default()).expect("convert"))
        .map(|logs| logs.iter().map(|log| log.topics.len()).collect::<Vec<_>>())
        .map_err(|e| e.0);
    assert_eq!(got, Ok(vec![0]));
}

/// Bytes that are not a recording are an error, not a panic.
#[test]
fn bytes_that_are_not_a_recording_are_an_error() {
    assert!(ulog_rrd::read(b"garbage").is_err());
}

/// PX4's text messages come back with time, level and text.
#[test]
fn text_messages_come_back_with_time_level_and_text() {
    let logs = ulog_rrd::read(&convert(&flight(), &Options::default()).expect("convert")).expect("read");
    let got: Vec<(i64, u8, &str)> =
        logs.iter().flat_map(|log| &log.messages).map(|m| (m.timestamp_us, m.level, m.text.as_str())).collect();
    assert_eq!(got, [(1_050_000, 4, "[commander] Takeoff detected")]);
}

/// The log's header info comes back.
#[test]
fn the_logs_header_info_comes_back() {
    use ulog_rrd::InfoValue;

    let mut ulog = Ulog::new(1_000_000);
    ulog.info("char[3] sys_name", b"PX4");
    ulog.info("char[8] ver_sw", b"abcdef12");
    let logs = ulog_rrd::read(&convert(&ulog.0, &Options::default()).expect("convert")).expect("read");
    let got: Vec<_> = logs.iter().map(|log| (log.info.get("sys_name"), log.info.get("ver_sw"))).collect();
    assert_eq!(got, [(Some(&vec![InfoValue::Text("PX4".into())]), Some(&vec![InfoValue::Text("abcdef12".into())]))]);
}

/// Two recordings in one file under the same prefix come back as two logs, each with its own
/// samples, not merged into one.
#[test]
fn two_recordings_under_one_prefix_come_back_as_two_logs() {
    let rrd: Vec<u8> = ["a", "b"]
        .into_iter()
        .flat_map(|id| {
            let opts = Options {
                recording_id: Some(id.into()),
                entity_path_prefix: Some("px4_sitl/logs".into()),
                ..scalars()
            };
            convert(&flight(), &opts).expect("convert")
        })
        .collect();
    let logs = ulog_rrd::read(&rrd).expect("read");
    let got: Vec<(&str, Option<usize>)> = logs
        .iter()
        .map(|log| {
            let samples = log.topics.iter().find(|t| t.name == "vehicle_local_position").map(|t| t.timestamps_us.len());
            (log.recording_id.as_str(), samples)
        })
        .collect();
    assert_eq!(got, [("a", Some(3)), ("b", Some(3))]);
}
