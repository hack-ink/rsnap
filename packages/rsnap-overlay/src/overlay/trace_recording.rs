mod model;

pub(crate) use self::model::{
	ScrollCaptureLiveTraceEntry, ScrollCaptureLiveTraceManifest, ScrollCaptureTraceDirection,
	ScrollCaptureTraceFrameEntry, ScrollCaptureTraceFrameSource, ScrollCaptureTraceInputEntry,
	ScrollCaptureTraceRecordedOutcome, ScrollCaptureTraceSessionSnapshot,
};

#[cfg(any(target_os = "macos", test))]
use std::{
	env, process,
	time::{SystemTime, UNIX_EPOCH},
};
use std::{
	fs,
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

use color_eyre::eyre::{self, Result, WrapErr};
#[cfg(target_os = "macos")]
use directories::ProjectDirs;
use image::RgbaImage;

use crate::overlay::ScrollCaptureFrameSource;
#[cfg(any(target_os = "macos", test))]
use crate::overlay::{MonitorRect, RectPoints};
use crate::scroll_capture;
use crate::{
	png,
	scroll_capture::{ScrollObserveOutcome, ScrollSession},
};

#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_TRACE_ENV: &str = "RSNAP_SCROLL_CAPTURE_TRACE";
#[cfg(target_os = "macos")]
const SCROLL_CAPTURE_TRACE_DIR_ENV: &str = "RSNAP_SCROLL_CAPTURE_TRACE_DIR";
#[cfg(any(target_os = "macos", test))]
const SCROLL_CAPTURE_TRACE_SCHEMA: &str = "scroll_capture_live_trace/1";
const SCROLL_CAPTURE_TRACE_MANIFEST_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) struct ScrollCaptureTraceRecorder {
	trace_dir: PathBuf,
	manifest_path: PathBuf,
	started_at: Instant,
	last_manifest_flush_at: Instant,
	next_frame_index: u64,
	last_recorded_frame_fingerprint: Option<Vec<u8>>,
	last_recorded_frame_path: Option<String>,
	manifest: ScrollCaptureLiveTraceManifest,
}
impl ScrollCaptureTraceRecorder {
	#[cfg(target_os = "macos")]
	pub(crate) fn from_env(
		monitor: MonitorRect,
		capture_rect_pixels: RectPoints,
		preview_width_px: u32,
		base_frame: &RgbaImage,
	) -> Option<Self> {
		let trace_root = resolve_trace_root_dir()?;

		match Self::new_for_root_dir(
			trace_root,
			monitor,
			capture_rect_pixels,
			preview_width_px,
			base_frame,
		) {
			Ok(recorder) => Some(recorder),
			Err(err) => {
				tracing::warn!(
					op = "scroll_capture.trace_init_failed",
					error = %err,
					"Failed to initialize scroll-capture live trace recorder."
				);

				None
			},
		}
	}

	#[cfg(any(target_os = "macos", test))]
	pub(crate) fn record_replayed_input(&mut self, record: ScrollCaptureTraceInputRecord) {
		self.manifest.entries.push(ScrollCaptureLiveTraceEntry::Input(
			ScrollCaptureTraceInputEntry {
				applied_at_ms: self.relative_ms(record.applied_at),
				seq: record.seq,
				cursor_global_x: record.cursor_global.0,
				cursor_global_y: record.cursor_global.1,
				delta_y: record.delta_y,
				gesture_active: record.gesture_active,
				gesture_ended: record.gesture_ended,
				recorded_age_ms: duration_to_ms(record.recorded_age),
				snapshot_after: record.snapshot_after,
			},
		));
		self.flush_manifest_if_due_best_effort("record_input");
	}

	pub(crate) fn record_frame_observation(&mut self, record: ScrollCaptureTraceFrameRecord<'_>) {
		let frame_fingerprint = scroll_capture::scroll_capture_fingerprint(record.frame);
		let frame_path = if self
			.last_recorded_frame_fingerprint
			.as_ref()
			.is_some_and(|previous| previous == &frame_fingerprint)
		{
			self.last_recorded_frame_path.clone().unwrap_or_else(|| {
				let frame_index = self.next_frame_index;

				self.next_frame_index = self.next_frame_index.saturating_add(1);

				format!("frames/frame-{frame_index:06}.png")
			})
		} else {
			let frame_index = self.next_frame_index;

			self.next_frame_index = self.next_frame_index.saturating_add(1);

			let frame_path = format!("frames/frame-{frame_index:06}.png");

			if let Err(err) = self.write_frame(record.frame, &frame_path) {
				tracing::warn!(
					op = "scroll_capture.trace_write_frame_failed",
					error = %err,
					frame_index,
					"Failed to persist scroll-capture trace frame."
				);
			}

			self.last_recorded_frame_fingerprint = Some(frame_fingerprint);
			self.last_recorded_frame_path = Some(frame_path.clone());

			frame_path
		};
		let outcome = match record.outcome {
			Ok(value) => ScrollCaptureTraceRecordedOutcome::from(*value),
			Err(err) => ScrollCaptureTraceRecordedOutcome::Error { message: format!("{err:#}") },
		};

		self.manifest.entries.push(ScrollCaptureLiveTraceEntry::Frame(
			ScrollCaptureTraceFrameEntry {
				observed_at_ms: self.relative_ms(record.observed_at),
				allow_stale_input: record.allow_stale_input,
				prior_block_reason: record.prior_block_reason.map(str::to_owned),
				frame_path,
				frame_source: record.source.into(),
				frame_dimensions: [record.frame.width(), record.frame.height()],
				snapshot_after: record.snapshot_after,
				outcome,
			},
		));
		self.flush_manifest_if_due_best_effort("record_frame");
	}

	pub(crate) fn record_error(&mut self, message: &str) {
		self.manifest.final_error = Some(message.to_owned());

		self.flush_manifest_best_effort("record_error");
	}

	pub(crate) fn finalize_session(
		&mut self,
		session: &ScrollSession,
		final_preview_image: &RgbaImage,
		final_snapshot: ScrollCaptureTraceSessionSnapshot,
	) {
		let final_preview_path = String::from("frames/final-preview.png");
		let final_export_path = String::from("frames/final-export.png");

		if let Err(err) = self.write_frame(final_preview_image, &final_preview_path) {
			tracing::warn!(
				op = "scroll_capture.trace_write_final_preview_failed",
				error = %err,
				manifest_path = %self.manifest_path.display(),
				"Failed to persist final scroll-capture preview trace frame."
			);
		} else {
			self.manifest.final_preview_path = Some(final_preview_path);
		}
		if let Err(err) = self.write_frame(session.export_image(), &final_export_path) {
			tracing::warn!(
				op = "scroll_capture.trace_write_final_export_failed",
				error = %err,
				manifest_path = %self.manifest_path.display(),
				"Failed to persist final scroll-capture export trace frame."
			);
		} else {
			self.manifest.final_export_path = Some(final_export_path);
		}

		self.manifest.final_snapshot = Some(final_snapshot);

		self.flush_manifest_best_effort("finalize_session");
	}

	#[cfg(any(target_os = "macos", test))]
	pub(crate) fn manifest_path(&self) -> &Path {
		&self.manifest_path
	}

	#[cfg(any(target_os = "macos", test))]
	pub(crate) fn new_for_root_dir(
		trace_root: PathBuf,
		monitor: MonitorRect,
		capture_rect_pixels: RectPoints,
		preview_width_px: u32,
		base_frame: &RgbaImage,
	) -> Result<Self> {
		let started_at = Instant::now();
		let started_unix_ms = now_unix_ms()?;
		let trace_id = format!("scroll-capture-{}-pid{}", started_unix_ms, process::id());
		let trace_dir = trace_root.join(&trace_id);
		let frames_dir = trace_dir.join("frames");
		let manifest_path = trace_dir.join("manifest.json");

		fs::create_dir_all(&frames_dir).wrap_err_with(|| {
			format!("failed to create trace directory {}", trace_dir.display())
		})?;

		let manifest = ScrollCaptureLiveTraceManifest {
			schema: SCROLL_CAPTURE_TRACE_SCHEMA.to_owned(),
			trace_id,
			started_unix_ms,
			preview_width_px,
			monitor: monitor.into(),
			capture_rect_pixels: capture_rect_pixels.into(),
			base_frame_path: "frames/base.png".to_owned(),
			entries: Vec::new(),
			final_preview_path: None,
			final_export_path: None,
			final_snapshot: None,
			final_error: None,
			finalized: false,
		};
		let mut recorder = Self {
			trace_dir,
			manifest_path,
			started_at,
			last_manifest_flush_at: started_at,
			next_frame_index: 0,
			last_recorded_frame_fingerprint: None,
			last_recorded_frame_path: None,
			manifest,
		};

		recorder.write_frame(base_frame, &recorder.manifest.base_frame_path)?;

		recorder.last_recorded_frame_fingerprint =
			Some(scroll_capture::scroll_capture_fingerprint(base_frame));
		recorder.last_recorded_frame_path = Some(recorder.manifest.base_frame_path.clone());

		recorder.flush_manifest_best_effort("init");

		Ok(recorder)
	}

	fn relative_ms(&self, at: Instant) -> u64 {
		duration_to_ms(at.saturating_duration_since(self.started_at))
	}

	fn write_frame(&self, frame: &RgbaImage, relative_path: &str) -> Result<()> {
		let target_path = self.trace_dir.join(relative_path);
		let png_bytes = png::rgba_image_to_png_bytes(frame)
			.wrap_err("failed to encode scroll-capture trace frame")?;

		fs::write(&target_path, png_bytes)
			.wrap_err_with(|| format!("failed to write trace frame {}", target_path.display()))
	}

	fn flush_manifest_best_effort(&self, op: &'static str) {
		if let Err(err) = self.flush_manifest() {
			tracing::warn!(
				op = "scroll_capture.trace_flush_failed",
				stage = op,
				error = %err,
				manifest_path = %self.manifest_path.display(),
				"Failed to flush scroll-capture trace manifest."
			);
		}
	}

	fn flush_manifest_if_due_best_effort(&mut self, op: &'static str) {
		let now = Instant::now();

		if now.saturating_duration_since(self.last_manifest_flush_at)
			< SCROLL_CAPTURE_TRACE_MANIFEST_FLUSH_INTERVAL
		{
			return;
		}

		self.last_manifest_flush_at = now;

		self.flush_manifest_best_effort(op);
	}

	fn flush_manifest(&self) -> Result<()> {
		let bytes = serde_json::to_vec_pretty(&self.manifest)
			.wrap_err("failed to serialize scroll-capture trace manifest")?;
		let tmp_path = self.manifest_path.with_extension("json.tmp");

		fs::write(&tmp_path, bytes).wrap_err_with(|| {
			format!("failed to write temporary trace manifest {}", tmp_path.display())
		})?;

		fs::rename(&tmp_path, &self.manifest_path).wrap_err_with(|| {
			format!(
				"failed to publish scroll-capture trace manifest {}",
				self.manifest_path.display()
			)
		})
	}
}

impl Drop for ScrollCaptureTraceRecorder {
	fn drop(&mut self) {
		self.manifest.finalized = true;

		self.flush_manifest_best_effort("drop");
	}
}

#[cfg(any(target_os = "macos", test))]
pub(crate) struct ScrollCaptureTraceInputRecord {
	pub(crate) seq: u64,
	pub(crate) cursor_global: (f64, f64),
	pub(crate) delta_y: f64,
	pub(crate) gesture_active: bool,
	pub(crate) gesture_ended: bool,
	pub(crate) recorded_age: Duration,
	pub(crate) applied_at: Instant,
	pub(crate) snapshot_after: ScrollCaptureTraceSessionSnapshot,
}

pub(crate) struct ScrollCaptureTraceFrameRecord<'a> {
	pub(crate) frame: &'a RgbaImage,
	pub(crate) source: ScrollCaptureFrameSource,
	pub(crate) allow_stale_input: bool,
	pub(crate) prior_block_reason: Option<&'static str>,
	pub(crate) observed_at: Instant,
	pub(crate) snapshot_after: ScrollCaptureTraceSessionSnapshot,
	pub(crate) outcome: &'a Result<ScrollObserveOutcome>,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedScrollCaptureLiveTrace {
	pub(crate) manifest_path: PathBuf,
	pub(crate) manifest: ScrollCaptureLiveTraceManifest,
	pub(crate) base_frame: RgbaImage,
}
impl LoadedScrollCaptureLiveTrace {
	pub(crate) fn load(manifest_path: impl AsRef<Path>) -> Result<Self> {
		let manifest_path = manifest_path.as_ref().to_path_buf();
		let manifest_bytes = fs::read(&manifest_path).wrap_err_with(|| {
			format!("failed to read scroll-capture trace manifest {}", manifest_path.display())
		})?;
		let manifest: ScrollCaptureLiveTraceManifest = serde_json::from_slice(&manifest_bytes)
			.wrap_err("failed to decode scroll-capture trace manifest")?;
		let base_dir = manifest_path.parent().ok_or_else(|| {
			eyre::eyre!("trace manifest path {} has no parent directory", manifest_path.display())
		})?;
		let base_frame_path = base_dir.join(&manifest.base_frame_path);
		let base_frame = image::open(&base_frame_path)
			.wrap_err_with(|| {
				format!(
					"failed to open scroll-capture trace base frame {}",
					base_frame_path.display()
				)
			})?
			.into_rgba8();

		Ok(Self { manifest_path, manifest, base_frame })
	}

	pub(crate) fn base_dir(&self) -> &Path {
		self.manifest_path.parent().expect("trace manifest path should have a parent directory")
	}

	pub(crate) fn resolve_frame_path(&self, relative_path: &str) -> PathBuf {
		self.base_dir().join(relative_path)
	}
}

#[cfg(target_os = "macos")]
fn resolve_trace_root_dir() -> Option<PathBuf> {
	let override_dir = env::var_os(SCROLL_CAPTURE_TRACE_DIR_ENV).and_then(|value| {
		let trimmed = value.to_string_lossy().trim().to_owned();

		if trimmed.is_empty() { None } else { Some(PathBuf::from(trimmed)) }
	});

	if let Some(dir) = override_dir {
		return Some(dir);
	}

	let enabled = env::var_os(SCROLL_CAPTURE_TRACE_ENV)
		.map(|value| parse_truthy_flag(&value.to_string_lossy()))
		.unwrap_or(false);

	if !enabled {
		return None;
	}

	ProjectDirs::from("ink", "hack", "rsnap")
		.map(|dirs| dirs.data_dir().join("scroll-capture-traces"))
}

#[cfg(target_os = "macos")]
fn parse_truthy_flag(value: &str) -> bool {
	let normalized = value.trim().to_ascii_lowercase();

	!matches!(normalized.as_str(), "" | "0" | "false" | "no" | "off")
}

fn duration_to_ms(duration: Duration) -> u64 {
	u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(any(target_os = "macos", test))]
fn now_unix_ms() -> Result<u64> {
	Ok(u64::try_from(
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.wrap_err("system clock is before unix epoch")?
			.as_millis(),
	)
	.unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
	use std::process;
	use std::sync::atomic::{AtomicU64, Ordering};

	use crate::overlay::trace_recording;
	use crate::overlay::trace_recording::SCROLL_CAPTURE_TRACE_SCHEMA;
	use crate::overlay::trace_recording::{
		Duration, Instant, LoadedScrollCaptureLiveTrace, MonitorRect, PathBuf, RectPoints,
		RgbaImage, ScrollCaptureLiveTraceEntry, ScrollCaptureTraceFrameRecord,
		ScrollCaptureTraceInputRecord, ScrollCaptureTraceRecorder,
		ScrollCaptureTraceSessionSnapshot, env, fs,
	};
	use crate::overlay::{OverlaySession, ScrollCaptureFrameSource};
	use crate::scroll_capture::{ScrollDirection, ScrollObserveOutcome, ScrollSession};
	use crate::state::GlobalPoint;

	static TRACE_TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

	fn test_monitor() -> MonitorRect {
		MonitorRect {
			id: 1,
			origin: GlobalPoint::new(0, 0),
			width: 1_000,
			height: 800,
			scale_factor_x1000: 1_000,
		}
	}

	fn test_rect() -> RectPoints {
		RectPoints::new(100, 120, 3, 5)
	}

	fn make_window(rows: &[[u8; 4]], start: usize) -> RgbaImage {
		let mut image = RgbaImage::new(3, 5);

		for (y, row) in rows[start..start + 5].iter().enumerate() {
			for x in 0..3 {
				image.put_pixel(x, y as u32, image::Rgba(*row));
			}
		}

		image
	}

	fn temp_trace_root() -> PathBuf {
		let counter = TRACE_TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
		let root = env::temp_dir().join(format!(
			"rsnap-scroll-trace-test-{}-{}-{}",
			trace_recording::now_unix_ms().unwrap_or(0),
			process::id(),
			counter
		));
		let _ = fs::remove_dir_all(&root);

		root
	}

	#[test]
	fn trace_recorder_round_trips_manifest_and_frames() {
		let rows = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let base_frame = make_window(&rows, 0);
		let next_frame = make_window(&rows, 1);
		let root = temp_trace_root();
		let start = Instant::now();
		let mut recorder = ScrollCaptureTraceRecorder::new_for_root_dir(
			root,
			test_monitor(),
			test_rect(),
			320,
			&base_frame,
		)
		.unwrap();
		let mut session = OverlaySession::new();

		session.scroll_capture.active = true;
		session.scroll_capture.monitor = Some(test_monitor());
		session.scroll_capture.capture_rect_pixels = Some(test_rect());
		session.scroll_capture.session = Some(ScrollSession::new(base_frame.clone(), 320).unwrap());

		recorder.record_replayed_input(ScrollCaptureTraceInputRecord {
			seq: 1,
			cursor_global: (150.0, 160.0),
			delta_y: 4.0,
			gesture_active: true,
			gesture_ended: false,
			recorded_age: Duration::from_millis(3),
			applied_at: start + Duration::from_millis(10),
			snapshot_after: ScrollCaptureTraceSessionSnapshot::capture(
				session.scroll_capture.session.as_ref(),
				session
					.scroll_capture
					.session
					.as_ref()
					.map(ScrollSession::preview_display_image)
					.map(|image| [image.width(), image.height()]),
				Some(ScrollDirection::Down),
				true,
				4.0,
				Some(3),
			),
		});
		recorder.record_frame_observation(ScrollCaptureTraceFrameRecord {
			frame: &next_frame,
			source: ScrollCaptureFrameSource::LiveStream { frame_seq: 7 },
			allow_stale_input: false,
			prior_block_reason: None,
			observed_at: start + Duration::from_millis(20),
			snapshot_after: ScrollCaptureTraceSessionSnapshot::capture(
				session.scroll_capture.session.as_ref(),
				session
					.scroll_capture
					.session
					.as_ref()
					.map(ScrollSession::preview_display_image)
					.map(|image| [image.width(), image.height()]),
				Some(ScrollDirection::Down),
				true,
				4.0,
				Some(1),
			),
			outcome: &Ok(ScrollObserveOutcome::PreviewUpdated),
		});

		let manifest_path = recorder.manifest_path().to_path_buf();

		drop(recorder);

		let loaded = LoadedScrollCaptureLiveTrace::load(&manifest_path).unwrap();

		assert_eq!(loaded.manifest.schema, SCROLL_CAPTURE_TRACE_SCHEMA);
		assert_eq!(loaded.manifest.entries.len(), 2);
		assert!(loaded.manifest.finalized);
		assert_eq!(loaded.base_frame.dimensions(), base_frame.dimensions());
		assert!(loaded.resolve_frame_path("frames/frame-000000.png").exists());
	}

	#[test]
	fn trace_recorder_persists_final_preview_export_artifacts() {
		let rows = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
		];
		let base_frame = make_window(&rows, 0);
		let root = temp_trace_root();
		let mut recorder = ScrollCaptureTraceRecorder::new_for_root_dir(
			root,
			test_monitor(),
			test_rect(),
			320,
			&base_frame,
		)
		.unwrap();
		let manifest_path = recorder.manifest_path().to_path_buf();
		let session = ScrollSession::new(base_frame.clone(), 320).unwrap();
		let final_snapshot = ScrollCaptureTraceSessionSnapshot::capture(
			Some(&session),
			Some({
				let image = session.preview_display_image();

				[image.width(), image.height()]
			}),
			Some(ScrollDirection::Down),
			false,
			0.0,
			Some(0),
		);
		let final_preview_image = session.preview_display_image();

		recorder.finalize_session(&session, &final_preview_image, final_snapshot);

		drop(recorder);

		let loaded = LoadedScrollCaptureLiveTrace::load(&manifest_path).unwrap();

		assert_eq!(loaded.manifest.final_preview_path.as_deref(), Some("frames/final-preview.png"));
		assert_eq!(loaded.manifest.final_export_path.as_deref(), Some("frames/final-export.png"));
		assert!(loaded.resolve_frame_path("frames/final-preview.png").exists());
		assert!(loaded.resolve_frame_path("frames/final-export.png").exists());
		assert!(loaded.manifest.final_snapshot.is_some());
	}

	#[test]
	fn trace_recorder_reuses_png_path_for_consecutive_identical_frames() {
		let rows = [
			[10, 0, 0, 255],
			[20, 0, 0, 255],
			[30, 0, 0, 255],
			[40, 0, 0, 255],
			[50, 0, 0, 255],
			[60, 0, 0, 255],
			[70, 0, 0, 255],
		];
		let base_frame = make_window(&rows, 0);
		let next_frame = make_window(&rows, 1);
		let root = temp_trace_root();
		let mut recorder = ScrollCaptureTraceRecorder::new_for_root_dir(
			root,
			test_monitor(),
			test_rect(),
			320,
			&base_frame,
		)
		.unwrap();
		let start = Instant::now();
		let manifest_path = recorder.manifest_path().to_path_buf();
		let snapshot = ScrollCaptureTraceSessionSnapshot::capture(
			None,
			Some([next_frame.width(), next_frame.height()]),
			Some(ScrollDirection::Down),
			true,
			32.0,
			Some(1),
		);

		recorder.record_frame_observation(ScrollCaptureTraceFrameRecord {
			frame: &next_frame,
			source: ScrollCaptureFrameSource::Worker { request_id: 1 },
			allow_stale_input: false,
			prior_block_reason: None,
			observed_at: start + Duration::from_millis(10),
			snapshot_after: snapshot.clone(),
			outcome: &Ok(ScrollObserveOutcome::PreviewUpdated),
		});
		recorder.record_frame_observation(ScrollCaptureTraceFrameRecord {
			frame: &next_frame,
			source: ScrollCaptureFrameSource::Worker { request_id: 2 },
			allow_stale_input: false,
			prior_block_reason: Some("frame_matches_last_committed_frame"),
			observed_at: start + Duration::from_millis(20),
			snapshot_after: snapshot,
			outcome: &Ok(ScrollObserveOutcome::NoChange),
		});

		drop(recorder);

		let loaded = LoadedScrollCaptureLiveTrace::load(&manifest_path).unwrap();
		let entries = loaded
			.manifest
			.entries
			.iter()
			.filter_map(|entry| match entry {
				ScrollCaptureLiveTraceEntry::Frame(frame) => Some(frame),
				ScrollCaptureLiveTraceEntry::Input(_) => None,
			})
			.collect::<Vec<_>>();

		assert_eq!(entries.len(), 2);
		assert_eq!(entries[0].frame_path, entries[1].frame_path);
		assert!(loaded.resolve_frame_path(&entries[0].frame_path).exists());
		assert!(!loaded.resolve_frame_path("frames/frame-000001.png").exists());
	}
}
