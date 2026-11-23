//! The rows ulog-rrd defines itself, of native Arrow values, so a reader gets each value back in the
//! type the ULog gave it: an info value, and a text message's severity.

use std::sync::Arc;

use re_sdk_types::external::arrow::array::{
    ArrayRef, BinaryArray, BooleanArray, Float32Array, Float64Array, Int16Array, Int32Array, Int64Array, Int8Array,
    StringArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use re_sdk_types::DynamicArchetype;

use crate::parse::InfoValue;

/// The archetype of an info row, and its one component.
pub const INFO: &str = "ulog_rrd.Info";
pub const INFO_VALUE: &str = "value";

/// The archetype of the severities beside the `TextLog` rows, and its one component.
pub const MESSAGE: &str = "ulog_rrd.Message";
pub const MESSAGE_SEVERITY: &str = "severity";

/// The syslog severities of text messages, 0 (emergency) to 7 (debug), one instance each.
pub fn severity(severities: impl Iterator<Item = u8>) -> DynamicArchetype {
    DynamicArchetype::new(MESSAGE)
        .with_component_from_data(MESSAGE_SEVERITY, Arc::new(UInt8Array::from_iter_values(severities)))
}

/// The row for one key's values: an instance per value. A key's values share one type; one of another
/// type, which a log does not write, is left out.
pub fn row(values: &[InfoValue]) -> DynamicArchetype {
    DynamicArchetype::new(INFO).with_component_from_data(INFO_VALUE, array(values))
}

fn array(values: &[InfoValue]) -> ArrayRef {
    macro_rules! of {
        ($variant:ident, $array:ty) => {
            Arc::new(<$array>::from_iter_values(values.iter().filter_map(|v| match v {
                InfoValue::$variant(x) => Some(x.clone()),
                _ => None,
            })))
        };
    }
    match values.first() {
        Some(InfoValue::Text(_)) => Arc::new(StringArray::from_iter_values(values.iter().filter_map(|v| match v {
            InfoValue::Text(s) => Some(s.as_str()),
            _ => None,
        }))),
        Some(InfoValue::Bytes(_)) => Arc::new(BinaryArray::from_iter_values(values.iter().filter_map(|v| match v {
            InfoValue::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }))),
        Some(InfoValue::Bool(_)) => Arc::new(BooleanArray::from_iter(values.iter().filter_map(|v| match v {
            InfoValue::Bool(x) => Some(Some(*x)),
            _ => None,
        }))),
        Some(InfoValue::U8(_)) => of!(U8, UInt8Array),
        Some(InfoValue::U16(_)) => of!(U16, UInt16Array),
        Some(InfoValue::U32(_)) => of!(U32, UInt32Array),
        Some(InfoValue::U64(_)) => of!(U64, UInt64Array),
        Some(InfoValue::I8(_)) => of!(I8, Int8Array),
        Some(InfoValue::I16(_)) => of!(I16, Int16Array),
        Some(InfoValue::I32(_)) => of!(I32, Int32Array),
        Some(InfoValue::I64(_)) => of!(I64, Int64Array),
        Some(InfoValue::F32(_)) => of!(F32, Float32Array),
        Some(InfoValue::F64(_)) => of!(F64, Float64Array),
        None => Arc::new(UInt8Array::from_iter_values([])),
    }
}
