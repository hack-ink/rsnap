#![allow(missing_docs)]

use std::hint;
use std::time::{Duration, Instant};

use color_eyre::eyre::{self, Result};

pub(crate) struct PerfCaseResult {
	name: String,
	iterations: u32,
	elapsed: Duration,
	budget: Duration,
	checksum: u64,
}
impl PerfCaseResult {
	pub(crate) fn print(&self) {
		println!(
			"[perf] {} iterations={} elapsed={} budget={} checksum={:#018x}",
			self.name,
			self.iterations,
			format_duration(self.elapsed),
			format_duration(self.budget),
			self.checksum
		);
	}

	pub(crate) fn require_budget(&self) -> Result<()> {
		eyre::ensure!(
			self.elapsed <= self.budget,
			"performance case {} exceeded budget: elapsed={} budget={}",
			self.name,
			format_duration(self.elapsed),
			format_duration(self.budget)
		);

		Ok(())
	}
}

pub(crate) fn time_case(
	name: impl Into<String>,
	iterations: u32,
	budget: Duration,
	mut run_once: impl FnMut() -> Result<u64>,
) -> Result<PerfCaseResult> {
	let started_at = Instant::now();
	let mut checksum = 0_u64;

	for _ in 0..iterations {
		checksum = checksum.wrapping_add(hint::black_box(run_once()?));
	}

	Ok(PerfCaseResult {
		name: name.into(),
		iterations,
		elapsed: started_at.elapsed(),
		budget,
		checksum,
	})
}

fn format_duration(duration: Duration) -> String {
	let micros = duration.as_micros();
	let millis = micros / 1_000;
	let fractional = micros % 1_000;

	format!("{millis}.{fractional:03}ms")
}
