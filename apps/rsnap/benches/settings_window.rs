use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use rsnap::settings_window::bench_support::{SettingsUiBenchHarness, SettingsUiBenchScenario};

fn bench_settings_layout(c: &mut Criterion) {
	let mut group = c.benchmark_group("settings_window_layout");

	for scenario in SettingsUiBenchScenario::ALL {
		let mut harness = SettingsUiBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| black_box(harness.run_layout()));
		});
	}

	group.finish();
}

fn bench_settings_frame(c: &mut Criterion) {
	let mut group = c.benchmark_group("settings_window_frame");

	for scenario in SettingsUiBenchScenario::ALL {
		let mut harness = SettingsUiBenchHarness::new(scenario);

		group.bench_function(scenario.as_str(), |b| {
			b.iter(|| black_box(harness.run_frame()));
		});
	}

	group.finish();
}

criterion_group!(benches, bench_settings_layout, bench_settings_frame);
criterion_main!(benches);
