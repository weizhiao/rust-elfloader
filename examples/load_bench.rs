//! Relink-only Linux/glibc load/unload runner for timing and profiling.
#[path = "../benches/support/libraries.rs"]
mod libraries;

use elf_loader::{Linker, Loader, input::PathBuf};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

const USAGE: &str =
    "load_bench --workload SONAME [--sample-ms N] [--warmup-ms N] [--rounds N] [--iterations N]";

fn main() {
    let mut workload = None;
    let mut iterations = None;
    let mut rounds = 9usize;
    let mut warmup_ms = 1000u64;
    let mut sample_ms = 1000u64;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!(
                "{USAGE}\nTimes complete Relink load/unload cycles; setup is excluded. Defaults: 1000 ms warmup, auto-calibrated iterations for ~1000 ms per round, 9 rounds. --iterations overrides calibration."
            );
            return;
        }
        let value = args
            .next()
            .unwrap_or_else(|| usage_error(&format!("missing value for {arg}")));
        match arg.as_str() {
            "--workload" => workload = Some(value),
            "--iterations" => {
                iterations = Some(
                    value
                        .parse::<usize>()
                        .unwrap_or_else(|_| usage_error("invalid iterations")),
                )
            }
            "--rounds" => {
                rounds = value
                    .parse()
                    .unwrap_or_else(|_| usage_error("invalid rounds"))
            }
            "--warmup-ms" => {
                warmup_ms = value
                    .parse()
                    .unwrap_or_else(|_| usage_error("invalid warmup duration"))
            }
            "--sample-ms" => {
                sample_ms = value
                    .parse()
                    .unwrap_or_else(|_| usage_error("invalid sample duration"))
            }
            _ => usage_error(&format!("unknown option {arg}")),
        }
    }
    if rounds == 0 || sample_ms == 0 || iterations == Some(0) {
        usage_error("rounds, sample-ms and iterations must be positive");
    }
    let name = workload.unwrap_or_else(|| usage_error("missing --workload"));
    if name.is_empty() || name.contains(['/', ':']) || name.chars().any(char::is_control) {
        usage_error("expected SONAME");
    }
    let cache = libraries::LibraryCache::new(false);
    let hosts = libraries::HostLibraries::new(&cache);
    let path = libraries::root_path(&cache, &name);
    let linker = Linker::new()
        .with_loader(Loader::new())
        .with_resolver(cache.resolver());
    let mut context = hosts.context();
    let mut measure = |iterations| {
        let start = Instant::now();
        for _ in 0..iterations {
            let loaded = linker
                .run()
                .load(
                    &mut context,
                    PathBuf::from(black_box(path.to_str().unwrap())),
                )
                .unwrap();
            drop(loaded.release(&mut context).unwrap());
        }
        let elapsed = start.elapsed();
        // Exclude validation from timing, and reject resident roots before reporting samples.
        assert_eq!(
            context.load_order().count(),
            hosts.len(),
            "workload retained modules after unload"
        );
        elapsed
    };
    let warmup_start = Instant::now();
    let mut batch = 1usize;
    while warmup_start.elapsed() < Duration::from_millis(warmup_ms) {
        let elapsed = measure(batch);
        if elapsed < Duration::from_millis(50) {
            batch = batch.saturating_mul(2);
        }
    }
    let iterations =
        iterations.unwrap_or_else(|| calibrate(&mut measure, Duration::from_millis(sample_ms)));
    println!("workload\tround\titerations\telapsed_ms\tns_per_load_unload");
    let mut times = Vec::with_capacity(rounds);
    for round in 0..rounds {
        let elapsed = measure(iterations);
        let time = elapsed.as_nanos() as f64 / iterations as f64;
        times.push(time);
        println!(
            "{name}\t{}\t{iterations}\t{:.1}\t{time:.1}",
            round + 1,
            elapsed.as_secs_f64() * 1000.0
        );
    }
    times.sort_by(f64::total_cmp);
    for (label, fraction) in [("p10", 0.1), ("median", 0.5), ("p90", 0.9)] {
        println!("{name}\t{label}\t-\t-\t{:.1}", percentile(&times, fraction));
    }
}

fn usage_error(message: &str) -> ! {
    eprintln!("{message}\nUsage: {USAGE}");
    std::process::exit(2);
}

fn calibrate(measure: &mut impl FnMut(usize) -> Duration, target: Duration) -> usize {
    let minimum = target.min(Duration::from_millis(100));
    let mut iterations = 1usize;
    loop {
        let elapsed = measure(iterations);
        if elapsed >= minimum {
            return ((iterations as f64 * target.as_secs_f64() / elapsed.as_secs_f64()).ceil()
                as usize)
                .max(1);
        }
        iterations = iterations
            .checked_mul(2)
            .expect("calibration iteration count overflowed");
    }
}

// Interpolated percentiles of round averages, not per-operation tail latencies or confidence bounds.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let index = (sorted.len() - 1) as f64 * fraction;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    sorted[lower] + (sorted[upper] - sorted[lower]) * index.fract()
}
