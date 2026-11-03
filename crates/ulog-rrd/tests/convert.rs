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

#[test]
fn the_vehicle_flies_in_the_z_up_frame_and_the_stray_sample_is_dropped() {
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

    // North 1, east 2, down -3 is east 2, north 1, up 3; its time is ULog time.
    let vehicle = &chunks["/vehicle"][0];
    let time = vehicle.timelines().values().next().expect("one timeline");
    assert_eq!(time.timeline().name().as_str(), "time");
    assert_eq!(time.times_raw()[0], 1_000_000_000);
    let translation = vehicle.components().0.values().next().expect("translation").list_array.clone();
    let first = re_sdk_types::external::arrow::array::Array::slice(&translation, 0, 1);
    assert!(format!("{first:?}").replace(['\n', ' '], "").contains("[2.0,1.0,3.0,]"), "{first:?}");
}

#[test]
fn a_prefix_moves_every_entity_and_carries_the_frame() {
    let opts = Options {
        entity_path_prefix: Some("px4_sitl/logs".into()),
        timeline: "sim".into(),
        time_offset_us: 1_000_000,
        scalars: true,
        rotation: Some([0.0, 0.0, -0.707, 0.707]),
        ..Options::default()
    };
    let chunks = chunks(&convert(&flight(), &opts).expect("convert"));
    assert!(chunks.keys().all(|k| k.starts_with("/px4_sitl/logs")), "{:?}", chunks.keys());
    assert_eq!(rows(&chunks, "/px4_sitl/logs"), 1, "the root's transform");
    assert_eq!(rows(&chunks, "/px4_sitl/logs/timeseries/vehicle_local_position"), 4, "three rows and the names");
    let vehicle = &chunks["/px4_sitl/logs/vehicle"][0];
    let time = vehicle.timelines().values().next().expect("one timeline");
    assert_eq!(time.timeline().name().as_str(), "sim");
    assert_eq!(time.times_raw()[0], 0, "the offset is the caller's time zero");
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    assert!(convert(b"not a ulog", &Options::default()).is_err());
}
