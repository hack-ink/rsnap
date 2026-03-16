use std::hint;

use criterion::{self, Criterion};

use rsnap_overlay::bench_support::{ScrollCaptureBenchHarness, ScrollCaptureBenchScenario};

criterion::criterion_group!(
	benches,
	bench_scroll_capture_fingerprint,
	bench_scroll_capture_overlap_match,
	bench_scroll_capture_session_commit,
);

criterion::criterion_main!(benches);

fn bench_scroll_capture_fingerprint(c: &mut Criterion) {
	let mut group = c.benchmark_group("scroll_capture_fingerprint");

	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| hint::black_box(harness.run_fingerprint()));
		});
	}

	group.finish();
}

fn bench_scroll_capture_overlap_match(c: &mut Criterion) {
	let mut group = c.benchmark_group("scroll_capture_overlap_match");

	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| hint::black_box(harness.run_overlap_match()));
		});
	}

	group.finish();
}

fn bench_scroll_capture_session_commit(c: &mut Criterion) {
	let mut group = c.benchmark_group("scroll_capture_session_commit");

	for scenario in ScrollCaptureBenchScenario::ALL {
		let harness = ScrollCaptureBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| hint::black_box(harness.run_session_commit()));
		});
	}

	group.finish();
}
