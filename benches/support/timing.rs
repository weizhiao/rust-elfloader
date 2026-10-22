//! Separate load and teardown timing with context creation and destruction outside the measured intervals.
use std::time::{Duration, Instant};

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
    mut teardown: impl FnMut(&mut Context, Loaded),
) -> Timings {
    let mut timings = Timings::default();
    for _ in 0..iterations {
        let mut context = prepare();
        let start = Instant::now();
        let loaded = load(&mut context);
        let loaded_at = Instant::now();
        teardown(&mut context, loaded);
        let finished_at = Instant::now();
        drop(context);
        timings.load += loaded_at.duration_since(start);
        timings.teardown += finished_at.duration_since(loaded_at);
    }
    timings
}
