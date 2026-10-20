//! Separate load and teardown timing with preparation outside the measured intervals.
use std::time::{Duration, Instant};

pub const PHASES: [&str; 3] = ["load", "teardown", "total"];

#[derive(Default)]
pub struct Timings {
    pub load: Duration,
    pub teardown: Duration,
}

impl Timings {
    pub fn total(&self) -> Duration {
        self.load + self.teardown
    }

    pub fn phases(&self) -> [Duration; 3] {
        [self.load, self.teardown, self.total()]
    }
}

pub fn measure<Context, Loaded>(
    iterations: u64,
    mut prepare: impl FnMut() -> Context,
    mut load: impl FnMut(&mut Context) -> Loaded,
    mut teardown: impl FnMut(Context, Loaded),
) -> Timings {
    let mut timings = Timings::default();
    for _ in 0..iterations {
        let mut context = prepare();
        let start = Instant::now();
        let loaded = load(&mut context);
        let loaded_at = Instant::now();
        teardown(context, loaded);
        let finished_at = Instant::now();
        timings.load += loaded_at.duration_since(start);
        timings.teardown += finished_at.duration_since(loaded_at);
    }
    timings
}
