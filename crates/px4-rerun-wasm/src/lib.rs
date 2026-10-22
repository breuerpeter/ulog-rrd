//! px4-rerun for the browser: ULog bytes in, RRD bytes out, for `WebViewer.open_channel().send_rrd()`.

use wasm_bindgen::prelude::*;

/// The px4-rerun version, which names the Rerun version it encodes for in `rerunVersion`.
#[wasm_bindgen]
pub fn version() -> String {
    px4_rerun::VERSION.to_owned()
}

/// The recording of a ULog as RRD bytes.
///
/// `timeOffsetUs` is subtracted from every ULog timestamp, so the rows sit on the caller's time.
#[wasm_bindgen]
pub fn convert(
    ulog: &[u8],
    #[wasm_bindgen(js_name = applicationId)] application_id: String,
    #[wasm_bindgen(js_name = recordingId)] recording_id: String,
    #[wasm_bindgen(js_name = entityPathPrefix)] entity_path_prefix: Option<String>,
    timeline: String,
    #[wasm_bindgen(js_name = timeOffsetUs)] time_offset_us: f64,
) -> Result<Vec<u8>, JsError> {
    let opts = px4_rerun::Options {
        application_id,
        recording_id,
        entity_path_prefix,
        timeline,
        time_offset_us: time_offset_us as i64,
        all_scalars: false,
    };
    px4_rerun::convert(ulog, &opts).map_err(|e| JsError::new(&e.0))
}
