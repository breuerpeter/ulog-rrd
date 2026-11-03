//! cargo run --release -p ulog-rrd --example convert -- flight.ulg flight.rrd [--scalars] [--prefix px4_sitl/logs]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = ulog_rrd::Options::default();
    let mut paths = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--scalars" => opts.scalars = true,
            "--app" => {
                i += 1;
                opts.application_id = args.get(i).cloned().unwrap_or_default();
            }
            "--recording" => {
                i += 1;
                opts.recording_id = args.get(i).cloned();
            }
            "--prefix" => {
                i += 1;
                opts.entity_path_prefix = args.get(i).cloned();
            }
            p => paths.push(p.to_owned()),
        }
        i += 1;
    }
    let [input, output] = paths.as_slice() else {
        eprintln!("usage: convert <in.ulg> <out.rrd> [--scalars] [--prefix <path>]");
        std::process::exit(2);
    };
    let bytes = std::fs::read(input).expect("read ulog");
    let t = std::time::Instant::now();
    let rrd = ulog_rrd::convert(&bytes, &opts).expect("convert");
    eprintln!("{} bytes -> {} bytes in {:.2?}", bytes.len(), rrd.len(), t.elapsed());
    std::fs::write(output, rrd).expect("write rrd");
}
