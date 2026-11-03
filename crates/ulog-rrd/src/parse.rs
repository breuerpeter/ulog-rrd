//! The ULog as columns: one table per topic instance, every numeric field a column of `f64`.

use std::collections::HashMap;
use std::io::Cursor;

use yule_log::model::inst::{self, FieldValue};
use yule_log::model::msg::UlogMessage;
use yule_log::parser::ULogParser;

use crate::{err, Error};

/// One topic instance's numeric fields, as columns. A field an earlier message lacked reads NaN there.
#[derive(Default)]
pub struct Table {
    pub timestamps: Vec<u64>,
    /// In the order the message definition lists them, arrays and nested types flattened.
    pub columns: Vec<(String, Vec<f64>)>,
}

impl Table {
    pub fn get(&self, field: &str) -> Option<&[f64]> {
        self.columns.iter().find(|(name, _)| name == field).map(|(_, v)| v.as_slice())
    }
}

/// A text message PX4 logged.
pub struct Message {
    pub timestamp: u64,
    /// The syslog severity, 0 (emergency) to 7 (debug).
    pub severity: u8,
    pub text: String,
}

pub struct Ulog {
    /// By topic name, `<name>_<multi_id>` for an instance after the first.
    pub tables: HashMap<String, Table>,
    pub messages: Vec<Message>,
    /// The span of the log in ULog time: the header's start, and the median over the topics of the
    /// last timestamp each logged, so one topic with a stray timestamp does not stretch it.
    pub start_us: u64,
    pub end_us: u64,
}

impl Ulog {
    pub fn table(&self, topic: &str) -> Option<&Table> {
        self.tables.get(topic)
    }
}

/// The columns of one message instance, in definition order: a scalar is one column, an array one
/// per element as `name[i]`, and a nested type its own columns as `name.field`. Characters are text,
/// not columns.
fn column_names(format: &inst::Format, prefix: &str, out: &mut Vec<String>) {
    for field in &format.fields {
        let name = format!("{prefix}{}", field.name);
        match &field.value {
            FieldValue::ScalarChar(_) | FieldValue::ArrayChar(_) => {}
            FieldValue::ScalarOther(nested) => column_names(nested, &format!("{name}."), out),
            FieldValue::ArrayOther(items) => {
                for (i, nested) in items.iter().enumerate() {
                    column_names(nested, &format!("{name}[{i}]."), out);
                }
            }
            value if field.r#type.is_array() => {
                let n = value.to_scalars().map_or(0, |v| v.len());
                out.extend((0..n).map(|i| format!("{name}[{i}]")));
            }
            _ => out.push(name),
        }
    }
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

/// The values of one message instance, in the order of [`column_names`].
fn values(format: &inst::Format, out: &mut Vec<f64>) {
    for field in &format.fields {
        match &field.value {
            FieldValue::ScalarChar(_) | FieldValue::ArrayChar(_) => {}
            FieldValue::ScalarOther(nested) => values(nested, out),
            FieldValue::ArrayOther(items) => items.iter().for_each(|nested| values(nested, out)),
            FieldValue::ArrayU8(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayU16(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayU32(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayU64(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayI8(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayI16(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayI32(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayI64(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayF32(v) => out.extend(v.iter().map(|x| *x as f64)),
            FieldValue::ArrayF64(v) => out.extend(v.iter().copied()),
            FieldValue::ArrayBool(v) => out.extend(v.iter().map(|x| *x as u8 as f64)),
            value => out.extend(scalar(value)),
        }
    }
}

pub fn parse(bytes: &[u8]) -> Result<Ulog, Error> {
    let mut tables: HashMap<String, Table> = HashMap::new();
    let mut messages = Vec::new();
    let mut start_us = 0;
    let mut row = Vec::new();
    // The parser drops the `timestamp` and `_padding` fields; `data.timestamp` keeps the time.
    let parser = ULogParser::new(Cursor::new(bytes)).map_err(err)?;
    for msg in parser {
        match msg.map_err(err)? {
            UlogMessage::Header(header) => start_us = header.timestamp,
            UlogMessage::LoggedData(data) => {
                let topic = match data.data.multi_id_index {
                    Some(i) if i > 0 => format!("{}_{i}", data.data.name),
                    _ => data.data.name.to_string(),
                };
                let table = tables.entry(topic).or_insert_with(|| {
                    let mut names = Vec::new();
                    column_names(&data.data, "", &mut names);
                    Table { timestamps: Vec::new(), columns: names.into_iter().map(|n| (n, Vec::new())).collect() }
                });
                row.clear();
                values(&data.data, &mut row);
                if row.len() != table.columns.len() {
                    // A message whose shape differs from its definition: the file is damaged here.
                    continue;
                }
                table.timestamps.push(data.timestamp);
                for ((_, column), value) in table.columns.iter_mut().zip(&row) {
                    column.push(*value);
                }
            }
            UlogMessage::LoggedString(s) | UlogMessage::TaggedLoggedString(s) => {
                messages.push(Message {
                    timestamp: s.timestamp,
                    severity: (s.level as u8).saturating_sub(b'0'),
                    text: s.msg,
                });
            }
            _ => {}
        }
    }
    let mut ends: Vec<u64> = tables.values().filter_map(|t| t.timestamps.iter().copied().max()).collect();
    ends.sort_unstable();
    let end_us = ends.get(ends.len() / 2).copied().unwrap_or(start_us);
    Ok(Ulog { tables, messages, start_us, end_us })
}
