use crate::overlay::{Duration, OverlayMode, SlowOperationLogger, WindowId, WindowRendererPath};

#[derive(Debug, Default)]
pub(in crate::overlay) struct WindowRendererPhaseTimings {
	pub(in crate::overlay::rendering) prepare_input: Duration,
	pub(in crate::overlay::rendering) sync_hud_bg: Duration,
	pub(in crate::overlay::rendering) run_egui: Duration,
	pub(in crate::overlay::rendering) update_hud_blur_uniform: Duration,
	pub(in crate::overlay::rendering) sync_egui_textures: Duration,
	pub(in crate::overlay::rendering) tessellate: Duration,
	pub(in crate::overlay::rendering) acquire_frame: Duration,
	pub(in crate::overlay::rendering) render_frame: Duration,
	pub(in crate::overlay::rendering) total: Duration,
}
impl WindowRendererPhaseTimings {
	pub(in crate::overlay::rendering) fn trace(
		&self,
		path: WindowRendererPath,
		window_id: WindowId,
		monitor_id: u32,
		mode: OverlayMode,
		toolbar_active: bool,
		paint_jobs: usize,
	) {
		tracing::trace!(
			op = "overlay.window_renderer_phase_timing",
			path = path.as_str(),
			window_id = ?window_id,
			monitor_id,
			mode = ?mode,
			toolbar_active,
			paint_jobs,
			total_us = self.total.as_micros(),
			prepare_input_us = self.prepare_input.as_micros(),
			sync_hud_bg_us = self.sync_hud_bg.as_micros(),
			run_egui_us = self.run_egui.as_micros(),
			update_hud_blur_uniform_us = self.update_hud_blur_uniform.as_micros(),
			sync_egui_textures_us = self.sync_egui_textures.as_micros(),
			tessellate_us = self.tessellate.as_micros(),
			acquire_frame_us = self.acquire_frame.as_micros(),
			render_frame_us = self.render_frame.as_micros(),
			"Overlay window renderer phase timing."
		);
	}

	pub(in crate::overlay::rendering) fn warn_if_substeps_slow(
		&self,
		slow_op_logger: &mut SlowOperationLogger,
		path: WindowRendererPath,
		window_id: WindowId,
		monitor_id: u32,
		mode: OverlayMode,
		paint_jobs: usize,
	) {
		let context = || {
			format!(
				"path={} window_id={window_id:?} monitor_id={monitor_id} mode={mode:?} paint_jobs={paint_jobs}",
				path.as_str()
			)
		};

		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.prepare_input",
			self.prepare_input,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.sync_hud_bg",
			self.sync_hud_bg,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.run_egui",
			self.run_egui,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.update_hud_blur_uniform",
			self.update_hud_blur_uniform,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.sync_egui_textures",
			self.sync_egui_textures,
			&context,
		);
		self.warn_phase_if_slow(
			slow_op_logger,
			"overlay.window_renderer.tessellate",
			self.tessellate,
			&context,
		);
	}

	fn warn_phase_if_slow<F>(
		&self,
		slow_op_logger: &mut SlowOperationLogger,
		op: &'static str,
		elapsed: Duration,
		describe: &F,
	) where
		F: Fn() -> String,
	{
		if elapsed.is_zero() {
			return;
		}

		slow_op_logger.warn_if_redraw_substep_slow(op, elapsed, self.total, describe);
	}
}
