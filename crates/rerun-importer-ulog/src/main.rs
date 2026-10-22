//! A Rerun external importer for PX4 ULog files.
//!
//! Rerun runs every `rerun-importer-*` executable on the `PATH` for a file it opens: the viewer for
//! `rerun flight.ulg`, and the SDK for `rr.log_file_from_path`. It passes the store to write to and
//! an entity path prefix, and reads RRD from stdout. Exit code 66 says "not my file".

use std::io::Write as _;

const INCOMPATIBLE: i32 = 66;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut opts = px4_rerun::Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" => {
                println!("rerun-importer-ulog {}", px4_rerun::VERSION);
                return;
            }
            "--application-id" => opts.application_id = args.next().unwrap_or_default(),
            "--recording-id" => opts.recording_id = args.next().unwrap_or_default(),
            "--entity-path-prefix" => opts.entity_path_prefix = args.next(),
            // px4-rerun's own flags, for a caller that runs the binary itself: `px4_rerun.log_ulog`.
            "--timeline" => opts.timeline = args.next().unwrap_or(opts.timeline),
            "--time-offset-us" => opts.time_offset_us = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            // Rerun's other flags: the opened store, static, and the caller's current time. A ULog
            // carries its own time, so these are read and dropped.
            "--opened-application-id" | "--opened-recording-id" | "--time_sequence" | "--time_duration_nanos"
            | "--time_timestamp_nanos" | "--sequence" | "--time" => {
                args.next();
            }
            "--static" | "--timeless" => {}
            other if !other.starts_with("--") => path = Some(other.to_owned()),
            _ => {}
        }
    }
    let Some(path) = path.filter(|p| p.ends_with(".ulg")) else {
        std::process::exit(INCOMPATIBLE);
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("rerun-importer-ulog: {path}: {e}");
            std::process::exit(1);
        }
    };
    match px4_rerun::convert(&bytes, &opts) {
        Ok(rrd) => {
            std::io::stdout().write_all(&rrd).expect("stdout");
        }
        Err(e) => {
            eprintln!("rerun-importer-ulog: {path}: {e}");
            std::process::exit(1);
        }
    }
}
