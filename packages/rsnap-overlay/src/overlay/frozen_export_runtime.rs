use image::{
	Rgba, RgbaImage,
	imageops::{self, FilterType},
};

use crate::overlay::session_state::FrozenBrushStyle;
use crate::overlay::{
	FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS, FrozenArrowAnnotation, FrozenBrushStroke,
	FrozenCaptureSource, FrozenCommittedOverlay, FrozenEditKind, FrozenExportTransform,
	FrozenSpotlightAnnotation, FrozenTextAnnotation, MonitorRect, OverlaySession, Pos2, RectPoints,
	WINDOW_CAPTURE_MATTE_DARK_RGBA, WINDOW_CAPTURE_MATTE_LIGHT_RGBA, WindowCaptureAlphaMode,
};

impl OverlaySession {
	pub(super) fn cropped_frozen_capture_image(&self) -> Option<RgbaImage> {
		if self.frozen_capture_source != FrozenCaptureSource::FullscreenFallback
			&& let Some(window_image) = self.frozen_window_image.as_ref()
		{
			match self.config.window_capture_alpha_mode {
				WindowCaptureAlphaMode::Background => {},
				WindowCaptureAlphaMode::MatteLight | WindowCaptureAlphaMode::MatteDark => {
					return Some(window_image.clone());
				},
			}
		}

		let frozen_image = self.state.frozen_image.as_ref()?;
		let Some(monitor) = self.state.monitor else {
			return Some(frozen_image.clone());
		};
		let capture_rect = self
			.state
			.frozen_capture_rect
			.unwrap_or_else(|| RectPoints::new(0, 0, monitor.width, monitor.height));
		let capture_rect = monitor.local_rect_to_pixels(capture_rect);
		let x = capture_rect.x.min(frozen_image.width());
		let y = capture_rect.y.min(frozen_image.height());
		let max_width = frozen_image.width().saturating_sub(x);
		let max_height = frozen_image.height().saturating_sub(y);
		let width = capture_rect.width.min(max_width);
		let height = capture_rect.height.min(max_height);

		if width == 0 || height == 0 {
			None
		} else {
			Some(imageops::crop_imm(frozen_image, x, y, width, height).to_image())
		}
	}

	fn render_frozen_committed_overlays_into_image(&self, image: &mut RgbaImage) {
		if self.scroll_capture.active {
			return;
		}

		let Some(export_transform) = self.frozen_export_transform_for_image(image) else {
			return;
		};
		let mut spotlight_annotations = Vec::new();

		Self::for_each_frozen_spotlight_annotation(
			&self.frozen_edit_undo_stack,
			&self.frozen_spotlight_annotations,
			|annotation| spotlight_annotations.push(annotation.clone()),
		);
		Self::apply_frozen_spotlight_annotations_to_image(
			image,
			export_transform,
			&spotlight_annotations,
		);

		let mut brush_coverage_mask = None;

		Self::for_each_frozen_committed_overlay(
			&self.frozen_edit_undo_stack,
			&self.frozen_brush.committed_strokes,
			&self.frozen_arrow_annotations,
			&self.frozen_text_annotations,
			|overlay| match overlay {
				FrozenCommittedOverlay::Brush(stroke) => {
					let coverage_mask = brush_coverage_mask.get_or_insert_with(|| {
						vec![0_u8; image.width() as usize * image.height() as usize]
					});

					Self::rasterize_frozen_brush_points_into_image(
						image,
						coverage_mask,
						export_transform,
						&stroke.points,
						stroke.style,
					);
				},
				FrozenCommittedOverlay::Arrow(annotation) => {
					let coverage_mask = brush_coverage_mask.get_or_insert_with(|| {
						vec![0_u8; image.width() as usize * image.height() as usize]
					});

					Self::render_frozen_arrow_annotation_into_image(
						image,
						coverage_mask,
						export_transform,
						annotation,
					);
				},
				FrozenCommittedOverlay::Text(annotation) => {
					Self::render_frozen_text_annotation_into_image(
						image,
						export_transform,
						annotation,
					);
				},
			},
		);

		if let Some(active_stroke) = &self.frozen_brush.active_stroke {
			let display_points = Self::active_frozen_brush_display_points(active_stroke);
			let coverage_mask = brush_coverage_mask.get_or_insert_with(|| {
				vec![0_u8; image.width() as usize * image.height() as usize]
			});

			Self::rasterize_frozen_brush_points_into_image(
				image,
				coverage_mask,
				export_transform,
				&display_points,
				active_stroke.style,
			);
		}
	}

	pub(super) fn rasterize_frozen_brush_points_into_image(
		export_image: &mut RgbaImage,
		coverage_mask: &mut [u8],
		export_transform: FrozenExportTransform,
		points: &[Pos2],
		style: FrozenBrushStyle,
	) {
		if export_image.width() == 0 || export_image.height() == 0 {
			return;
		}
		if coverage_mask.len() != export_image.width() as usize * export_image.height() as usize {
			return;
		}

		let radius = (style.stroke_width_points * export_transform.scalar_scale() * 0.5).max(1.0);
		let color = Rgba(style.color.export_rgba());

		coverage_mask.fill(0);

		Self::rasterize_frozen_brush_points(
			coverage_mask,
			export_image.width(),
			export_image.height(),
			points,
			export_transform,
			radius,
		);
		Self::blend_frozen_brush_coverage_mask(export_image, coverage_mask, color);
	}

	pub(super) fn for_each_frozen_committed_overlay(
		edit_history: &[FrozenEditKind],
		brush_strokes: &[FrozenBrushStroke],
		arrow_annotations: &[FrozenArrowAnnotation],
		text_annotations: &[FrozenTextAnnotation],
		mut f: impl FnMut(FrozenCommittedOverlay<'_>),
	) {
		let mut brush_index = 0;
		let mut arrow_index = 0;
		let mut text_index = 0;

		for edit_kind in edit_history {
			match edit_kind {
				FrozenEditKind::BrushStroke => {
					let Some(stroke) = brush_strokes.get(brush_index) else {
						continue;
					};

					brush_index += 1;

					f(FrozenCommittedOverlay::Brush(stroke));
				},
				FrozenEditKind::ArrowAnnotation => {
					let Some(annotation) = arrow_annotations.get(arrow_index) else {
						continue;
					};

					arrow_index += 1;

					f(FrozenCommittedOverlay::Arrow(annotation));
				},
				FrozenEditKind::TextAnnotation => {
					let Some(annotation) = text_annotations.get(text_index) else {
						continue;
					};

					text_index += 1;

					f(FrozenCommittedOverlay::Text(annotation));
				},
				FrozenEditKind::MosaicEdit | FrozenEditKind::SpotlightAnnotation => {},
			}
		}
	}

	pub(super) fn for_each_frozen_spotlight_annotation(
		edit_history: &[FrozenEditKind],
		spotlight_annotations: &[FrozenSpotlightAnnotation],
		mut f: impl FnMut(&FrozenSpotlightAnnotation),
	) {
		let mut spotlight_index = 0;

		for edit_kind in edit_history {
			if *edit_kind != FrozenEditKind::SpotlightAnnotation {
				continue;
			}

			let Some(annotation) = spotlight_annotations.get(spotlight_index) else {
				continue;
			};

			spotlight_index += 1;

			f(annotation);
		}
	}

	fn export_rect_points(
		export_transform: FrozenExportTransform,
		rect: RectPoints,
		export_width: u32,
		export_height: u32,
	) -> Option<RectPoints> {
		let min = export_transform.point_to_pixels(Pos2::new(rect.x as f32, rect.y as f32));
		let max = export_transform.point_to_pixels(Pos2::new(
			rect.x.saturating_add(rect.width) as f32,
			rect.y.saturating_add(rect.height) as f32,
		));
		let left = min.x.floor().max(0.0) as u32;
		let top = min.y.floor().max(0.0) as u32;
		let right = max.x.ceil().min(export_width as f32) as u32;
		let bottom = max.y.ceil().min(export_height as f32) as u32;

		(right > left && bottom > top).then(|| {
			RectPoints::new(left, top, right.saturating_sub(left), bottom.saturating_sub(top))
		})
	}

	fn apply_frozen_spotlight_annotations_to_image(
		image: &mut RgbaImage,
		export_transform: FrozenExportTransform,
		annotations: &[FrozenSpotlightAnnotation],
	) {
		let capture_rect = RectPoints::new(0, 0, image.width(), image.height());
		let spotlight_rects = annotations
			.iter()
			.filter_map(|annotation| {
				Self::export_rect_points(
					export_transform,
					annotation.rect,
					image.width(),
					image.height(),
				)
			})
			.collect::<Vec<_>>();
		let spotlight_rects = Self::clipped_frozen_spotlight_rects(capture_rect, spotlight_rects);

		if spotlight_rects.is_empty() {
			return;
		}

		for dim_rect in Self::frozen_spotlight_scrim_rects(capture_rect, &spotlight_rects) {
			let right = dim_rect.x.saturating_add(dim_rect.width);
			let bottom = dim_rect.y.saturating_add(dim_rect.height);

			for y in dim_rect.y..bottom {
				for x in dim_rect.x..right {
					let pixel = image.get_pixel_mut(x, y);

					for channel in 0..3 {
						pixel[channel] = ((u16::from(pixel[channel])
							* Self::frozen_spotlight_outside_brightness_numerator())
							/ 255) as u8;
					}
				}
			}
		}
	}

	fn render_frozen_arrow_annotation_into_image(
		export_image: &mut RgbaImage,
		coverage_mask: &mut [u8],
		export_transform: FrozenExportTransform,
		annotation: &FrozenArrowAnnotation,
	) {
		let Some(geometry) = Self::frozen_arrow_geometry(annotation) else {
			return;
		};
		let outline_width =
			Self::frozen_arrow_outline_width_points(annotation.style.stroke_width_points)
				* export_transform.scalar_scale();
		let mut shaft_style = annotation.style;

		shaft_style.stroke_width_points =
			Self::frozen_arrow_stroke_width_points(annotation.style.stroke_width_points);

		let outline_style = FrozenBrushStyle {
			stroke_width_points: Self::frozen_arrow_outline_stroke_width_points(
				annotation.style.stroke_width_points,
			),
			..shaft_style
		};
		let outline_radius =
			(outline_style.stroke_width_points * export_transform.scalar_scale() * 0.5).max(1.0);

		coverage_mask.fill(0);

		Self::rasterize_frozen_brush_points(
			coverage_mask,
			export_image.width(),
			export_image.height(),
			&[annotation.start, geometry.shaft_end],
			export_transform,
			outline_radius,
		);
		Self::blend_frozen_brush_coverage_mask(
			export_image,
			coverage_mask,
			Rgba([255, 255, 255, 208]),
		);

		coverage_mask.fill(0);

		Self::rasterize_frozen_brush_points_into_image(
			export_image,
			coverage_mask,
			export_transform,
			&[annotation.start, geometry.shaft_end],
			shaft_style,
		);

		coverage_mask.fill(0);

		let (outline_tip, outline_left, outline_right) = Self::expanded_triangle(
			export_transform.point_to_pixels(geometry.tip),
			export_transform.point_to_pixels(geometry.head_left),
			export_transform.point_to_pixels(geometry.head_right),
			outline_width,
		);

		Self::rasterize_frozen_triangle(
			coverage_mask,
			export_image.width(),
			export_image.height(),
			outline_tip,
			outline_left,
			outline_right,
		);
		Self::blend_frozen_brush_coverage_mask(
			export_image,
			coverage_mask,
			Rgba([255, 255, 255, 208]),
		);

		coverage_mask.fill(0);

		Self::rasterize_frozen_triangle(
			coverage_mask,
			export_image.width(),
			export_image.height(),
			export_transform.point_to_pixels(geometry.tip),
			export_transform.point_to_pixels(geometry.head_left),
			export_transform.point_to_pixels(geometry.head_right),
		);
		Self::blend_frozen_brush_coverage_mask(
			export_image,
			coverage_mask,
			Rgba(annotation.style.color.export_rgba()),
		);
	}

	fn expanded_triangle(a: Pos2, b: Pos2, c: Pos2, amount: f32) -> (Pos2, Pos2, Pos2) {
		Self::frozen_arrow_expanded_triangle(a, b, c, amount)
	}

	fn rasterize_frozen_triangle(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		a: Pos2,
		b: Pos2,
		c: Pos2,
	) {
		if export_width == 0 || export_height == 0 {
			return;
		}

		let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as u32;
		let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as u32;
		let max_x = a.x.max(b.x).max(c.x).ceil().min(export_width.saturating_sub(1) as f32) as u32;
		let max_y = a.y.max(b.y).max(c.y).ceil().min(export_height.saturating_sub(1) as f32) as u32;

		for y in min_y..=max_y {
			for x in min_x..=max_x {
				let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);

				if Self::point_in_triangle(sample, a, b, c) {
					let index = y as usize * export_width as usize + x as usize;

					coverage_mask[index] = 255;
				}
			}
		}
	}

	fn point_in_triangle(sample: Pos2, a: Pos2, b: Pos2, c: Pos2) -> bool {
		fn edge(p0: Pos2, p1: Pos2, p2: Pos2) -> f32 {
			(p2.x - p0.x) * (p1.y - p0.y) - (p2.y - p0.y) * (p1.x - p0.x)
		}

		let ab = edge(a, b, sample);
		let bc = edge(b, c, sample);
		let ca = edge(c, a, sample);

		(ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)
	}

	fn rasterize_frozen_brush_points(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		points: &[Pos2],
		export_transform: FrozenExportTransform,
		radius: f32,
	) {
		let rendered_points =
			Self::rendered_frozen_brush_points(points, FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS);
		let Some(first) = rendered_points.first().copied() else {
			return;
		};
		let mut previous = export_transform.point_to_pixels(first);

		Self::rasterize_frozen_brush_circle(
			coverage_mask,
			export_width,
			export_height,
			previous,
			radius,
		);

		for point in rendered_points.into_iter().skip(1) {
			let current = export_transform.point_to_pixels(point);

			Self::rasterize_frozen_brush_segment(
				coverage_mask,
				export_width,
				export_height,
				previous,
				current,
				radius,
			);

			previous = current;
		}
	}

	fn frozen_export_capture_rect(&self) -> Option<RectPoints> {
		self.state.frozen_capture_rect.or_else(|| {
			self.state.monitor.map(|monitor| RectPoints::new(0, 0, monitor.width, monitor.height))
		})
	}

	fn frozen_export_transform_for_image(
		&self,
		image: &RgbaImage,
	) -> Option<FrozenExportTransform> {
		FrozenExportTransform::new(
			self.frozen_export_capture_rect()?,
			image.width(),
			image.height(),
		)
	}

	fn rasterize_frozen_brush_segment(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		start: Pos2,
		end: Pos2,
		radius: f32,
	) {
		let delta = end - start;
		let delta_len_sq = delta.length_sq();

		if delta_len_sq <= f32::EPSILON {
			Self::rasterize_frozen_brush_circle(
				coverage_mask,
				export_width,
				export_height,
				start,
				radius,
			);

			return;
		}

		let min_x = ((start.x.min(end.x) - radius - 0.5).floor().max(0.0)) as u32;
		let min_y = ((start.y.min(end.y) - radius - 0.5).floor().max(0.0)) as u32;
		let max_x = ((start.x.max(end.x) + radius + 0.5)
			.ceil()
			.min((export_width.saturating_sub(1)) as f32)) as u32;
		let max_y = ((start.y.max(end.y) + radius + 0.5)
			.ceil()
			.min((export_height.saturating_sub(1)) as f32)) as u32;

		for y in min_y..=max_y {
			for x in min_x..=max_x {
				let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
				let projection = ((sample - start).dot(delta) / delta_len_sq).clamp(0.0, 1.0);
				let nearest = start + delta * projection;
				let coverage = Self::frozen_brush_coverage(sample.distance(nearest), radius);

				Self::update_frozen_brush_coverage_mask(
					coverage_mask,
					export_width,
					x,
					y,
					coverage,
				);
			}
		}
	}

	fn rasterize_frozen_brush_circle(
		coverage_mask: &mut [u8],
		export_width: u32,
		export_height: u32,
		center: Pos2,
		radius: f32,
	) {
		if export_width == 0 || export_height == 0 {
			return;
		}

		let min_x = ((center.x - radius - 0.5).floor().max(0.0)) as u32;
		let min_y = ((center.y - radius - 0.5).floor().max(0.0)) as u32;
		let max_x =
			(center.x + radius + 0.5).ceil().min((export_width.saturating_sub(1)) as f32) as u32;
		let max_y =
			(center.y + radius + 0.5).ceil().min((export_height.saturating_sub(1)) as f32) as u32;

		if min_x > max_x || min_y > max_y {
			return;
		}

		for y in min_y..=max_y {
			for x in min_x..=max_x {
				let sample = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
				let coverage = Self::frozen_brush_coverage(sample.distance(center), radius);

				Self::update_frozen_brush_coverage_mask(
					coverage_mask,
					export_width,
					x,
					y,
					coverage,
				);
			}
		}
	}

	fn frozen_brush_coverage(distance: f32, radius: f32) -> u8 {
		((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0).round() as u8
	}

	fn update_frozen_brush_coverage_mask(
		coverage_mask: &mut [u8],
		export_width: u32,
		x: u32,
		y: u32,
		coverage: u8,
	) {
		if coverage == 0 {
			return;
		}

		let index = y as usize * export_width as usize + x as usize;

		coverage_mask[index] = coverage_mask[index].max(coverage);
	}

	fn blend_frozen_brush_coverage_mask(
		export_image: &mut RgbaImage,
		coverage_mask: &[u8],
		color: Rgba<u8>,
	) {
		let source_alpha = color[3] as f32 / 255.0;

		for (index, pixel) in export_image.pixels_mut().enumerate() {
			let mask_alpha = coverage_mask[index];

			if mask_alpha == 0 {
				continue;
			}

			let src_a = (mask_alpha as f32 / 255.0) * source_alpha;
			let dst_a = pixel[3] as f32 / 255.0;
			let out_a = src_a + dst_a * (1.0 - src_a);

			if out_a <= f32::EPSILON {
				continue;
			}

			for channel in 0..3 {
				let src = color[channel] as f32 / 255.0;
				let dst = pixel[channel] as f32 / 255.0;
				let out = (src * src_a + dst * dst_a * (1.0 - src_a)) / out_a;

				pixel[channel] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
			}

			pixel[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
		}
	}

	pub(super) fn current_export_image(&self) -> Option<RgbaImage> {
		if self.scroll_capture.active {
			return self
				.scroll_capture
				.session
				.as_ref()
				.map(|session| session.export_image().clone());
		}

		let mut export_image =
			self.cropped_frozen_capture_image().or_else(|| self.state.frozen_image.clone())?;

		self.render_frozen_committed_overlays_into_image(&mut export_image);

		Some(export_image)
	}

	fn flatten_window_image_with_matte(image: &RgbaImage, matte: Rgba<u8>) -> RgbaImage {
		let mut out = image.clone();

		for pixel in out.pixels_mut() {
			let alpha = u16::from(pixel[3]);
			let inv_alpha = 255_u16.saturating_sub(alpha);

			for channel in 0..3 {
				let src = u16::from(pixel[channel]);
				let bg = u16::from(matte[channel]);
				let blended =
					(src.saturating_mul(alpha) + bg.saturating_mul(inv_alpha) + 127) / 255;

				pixel[channel] = blended as u8;
			}

			pixel[3] = 255;
		}

		out
	}

	pub(super) fn compose_window_preview_layer(
		window_image: &RgbaImage,
		alpha_mode: WindowCaptureAlphaMode,
	) -> RgbaImage {
		match alpha_mode {
			WindowCaptureAlphaMode::Background => window_image.clone(),
			WindowCaptureAlphaMode::MatteLight => {
				Self::flatten_window_image_with_matte(window_image, WINDOW_CAPTURE_MATTE_LIGHT_RGBA)
			},
			WindowCaptureAlphaMode::MatteDark => {
				Self::flatten_window_image_with_matte(window_image, WINDOW_CAPTURE_MATTE_DARK_RGBA)
			},
		}
	}

	pub(super) fn composite_window_capture_preview(
		mut monitor_image: RgbaImage,
		window_image: &RgbaImage,
		monitor: MonitorRect,
		capture_rect_points: RectPoints,
		alpha_mode: WindowCaptureAlphaMode,
	) -> RgbaImage {
		let capture_rect_px = monitor.local_rect_to_pixels(capture_rect_points);

		if capture_rect_px.width == 0 || capture_rect_px.height == 0 {
			return monitor_image;
		}

		let window_overlay = if window_image.width() == capture_rect_px.width
			&& window_image.height() == capture_rect_px.height
		{
			window_image.clone()
		} else {
			imageops::resize(
				window_image,
				capture_rect_px.width,
				capture_rect_px.height,
				FilterType::Triangle,
			)
		};
		let preview_layer = Self::compose_window_preview_layer(&window_overlay, alpha_mode);

		imageops::overlay(
			&mut monitor_image,
			&preview_layer,
			i64::from(capture_rect_px.x),
			i64::from(capture_rect_px.y),
		);

		monitor_image
	}
}
