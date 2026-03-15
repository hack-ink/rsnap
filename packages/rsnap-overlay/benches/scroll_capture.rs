use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use rsnap_overlay::bench_support::{ScrollCaptureBenchHarness, ScrollCaptureBenchScenario};

fn bench_scroll_capture_fingerprint(c: &mut Criterion) {
	let mut group = c.benchmark_group("scroll_capture_fingerprint");

	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| black_box(harness.run_fingerprint()));
		});
	}

	group.finish();
}

fn bench_scroll_capture_overlap_match(c: &mut Criterion) {
	let mut group = c.benchmark_group("scroll_capture_overlap_match");

	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| black_box(harness.run_overlap_match()));
		});
	}

	group.finish();
}

fn bench_scroll_capture_session_commit(c: &mut Criterion) {
	let mut group = c.benchmark_group("scroll_capture_session_commit");

	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| black_box(harness.run_session_commit()));
		});
	}

	group.finish();
}

criterion_group!(
	benches,
	bench_scroll_capture_fingerprint,
	bench_scroll_capture_overlap_match,
	bench_scroll_capture_session_commit,
);
criterion_main!(benches);
