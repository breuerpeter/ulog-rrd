//! A recording back into the PX4 logs it holds: each log's topics, text messages and info.

use std::collections::BTreeMap;

use crate::{Error, InfoValue};

/// One PX4 log in a recording, found under the prefix it was logged with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Log {
    /// The entity path prefix the log sits under; `None` at the root.
    pub prefix: Option<String>,
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

/// The PX4 logs in the recording `rrd`, one per prefix.
pub fn read(_rrd: &[u8]) -> Result<Vec<Log>, Error> {
    Ok(Vec::new())
}
