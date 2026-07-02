use std::f32::consts::FRAC_PI_2;
use std::f32::consts::PI;

use egui::Context;

use crate::overlay::rendering::{
	SelectionFlowGeometryCache, SelectionFlowGeometryCacheKey, WindowRenderer,
};
use crate::overlay::{
	Color32, HudTheme, LIVE_DRAG_START_THRESHOLD_PX, Mesh, Painter, Pos2, Rect, SelectionFlowStyle,
	Shape, Vec2,
};

const SELECTION_FLOW_CORNER_RADIUS_PX: f32 = 9.0;
const SELECTION_FLOW_MIN_SEGMENTS: usize = 160;
const SELECTION_FLOW_MAX_SEGMENTS: usize = 1_536;
const SELECTION_FLOW_SAMPLE_STEP_PX: f32 = 3.2;
const SELECTION_FLOW_SPEED: f32 = 0.24;
const SELECTION_FLOW_CORE_FLOW_WIDTH: f32 = 0.06;
const SELECTION_FLOW_FLOW_BOOST: f32 = 2.8;
const SELECTION_FLOW_PALETTE: [(u8, u8, u8); 3] =
	[(196, 226, 255), (228, 198, 255), (176, 244, 224)];
const SELECTION_FLOW_LIGHT_PALETTE: [(u8, u8, u8); 3] =
	[(0, 104, 226), (124, 54, 214), (0, 128, 104)];

impl WindowRenderer {
	pub(in crate::overlay) fn render_selection_flow_ring(
		painter: &Painter,
		rect: Rect,
		ctx: &Context,
		theme: HudTheme,
		style: SelectionFlowStyle,
		selection_flow_stroke_width_px: f32,
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
	) {
		if rect.width() < LIVE_DRAG_START_THRESHOLD_PX
			|| rect.height() < LIVE_DRAG_START_THRESHOLD_PX
		{
			return;
		}

		let corner_radius = Self::selection_flow_corner_radius(rect);
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let time = ctx.input(|i| i.time) as f32;
		let sample_count = Self::selection_flow_sample_count(perimeter);
		let seam_offset = if rect.width() > corner_radius * 2.0 {
			(rect.width() - corner_radius * 2.0) * 0.5
		} else {
			0.0
		};
		let (samples, normals) = Self::selection_flow_cached_geometry(
			selection_flow_geometry_cache,
			rect,
			corner_radius,
			sample_count,
			seam_offset,
		);
		let base_alpha_scale = 1.0;
		let stroke_width = selection_flow_stroke_width_px.clamp(1.0, 8.0);

		if samples.is_empty() {
			return;
		}

		let flow_time = time * SELECTION_FLOW_SPEED;
		let phase = flow_time * 1.28 + 0.72;

		match style {
			SelectionFlowStyle::Band => Self::selection_flow_draw_layer(
				painter,
				samples,
				normals,
				stroke_width,
				base_alpha_scale * 0.52,
				phase,
				SELECTION_FLOW_CORE_FLOW_WIDTH,
				theme,
			),
		}
	}

	pub(in crate::overlay) fn selection_flow_corner_radius(rect: Rect) -> f32 {
		SELECTION_FLOW_CORNER_RADIUS_PX
			.min(rect.width() / 2.0 - 0.25)
			.min(rect.height() / 2.0 - 0.25)
			.max(0.0)
	}

	pub(in crate::overlay) fn selection_flow_palette(
		theme: HudTheme,
	) -> &'static [(u8, u8, u8); 3] {
		match theme {
			HudTheme::Dark => &SELECTION_FLOW_PALETTE,
			HudTheme::Light => &SELECTION_FLOW_LIGHT_PALETTE,
		}
	}

	pub(in crate::overlay) fn selection_flow_cached_geometry(
		selection_flow_geometry_cache: &mut SelectionFlowGeometryCache,
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		seam_offset: f32,
	) -> (&[(Pos2, f32)], &[Vec2]) {
		let key =
			SelectionFlowGeometryCacheKey::new(rect, corner_radius, seam_offset, sample_count);

		if selection_flow_geometry_cache.key == Some(key)
			&& !selection_flow_geometry_cache.samples.is_empty()
		{
			return (
				&selection_flow_geometry_cache.samples,
				&selection_flow_geometry_cache.normals,
			);
		}

		let samples =
			Self::selection_flow_path_samples(rect, corner_radius, sample_count, seam_offset);
		let normals = Self::selection_flow_compute_normals(&samples);

		selection_flow_geometry_cache.key = Some(key);
		selection_flow_geometry_cache.samples = samples;
		selection_flow_geometry_cache.normals = normals;

		(&selection_flow_geometry_cache.samples, &selection_flow_geometry_cache.normals)
	}

	pub(in crate::overlay) fn selection_flow_compute_normals(samples: &[(Pos2, f32)]) -> Vec<Vec2> {
		let n = samples.len();

		if n == 0 {
			return Vec::new();
		}

		let mut normals = Vec::with_capacity(n);
		let mut first_non_zero = None;

		for i in 0..n {
			let (current_point, _) = samples[i];
			let (prev_point, _) = samples[(i + n - 1) % n];
			let (next_point, _) = samples[(i + 1) % n];
			let prev_tangent = current_point - prev_point;
			let next_tangent = next_point - current_point;
			let mut normal = Vec2::ZERO;

			if prev_tangent.length_sq() > f32::EPSILON {
				let prev_len = prev_tangent.length();

				normal += Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
			}
			if next_tangent.length_sq() > f32::EPSILON {
				let next_len = next_tangent.length();

				normal += Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
			}
			if normal.length_sq() <= f32::EPSILON {
				if next_tangent.length_sq() > f32::EPSILON {
					let next_len = next_tangent.length();

					normal = Vec2::new(-next_tangent.y / next_len, next_tangent.x / next_len);
				} else if prev_tangent.length_sq() > f32::EPSILON {
					let prev_len = prev_tangent.length();

					normal = Vec2::new(-prev_tangent.y / prev_len, prev_tangent.x / prev_len);
				}
			}

			let normal = if normal.length_sq() > f32::EPSILON {
				let normalized = normal / normal.length();

				if first_non_zero.is_none() && normalized.length_sq() > f32::EPSILON {
					first_non_zero = Some(i);
				}

				normalized
			} else {
				Vec2::ZERO
			};

			normals.push(normal);
		}

		if let Some(first_idx) = first_non_zero {
			let mut previous = normals[first_idx];

			for normal in normals.iter_mut().skip(first_idx + 1) {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}
			for normal in normals.iter_mut().take(first_idx).rev() {
				if normal.length_sq() > f32::EPSILON && normal.dot(previous) < 0.0 {
					*normal = -*normal;
				}
				if normal.length_sq() > f32::EPSILON {
					previous = *normal;
				}
			}

			if normals[first_idx].length_sq() > f32::EPSILON
				&& normals[(first_idx + n - 1) % n].length_sq() > f32::EPSILON
				&& normals[first_idx].dot(normals[(first_idx + n - 1) % n]) < 0.0
			{
				for normal in &mut normals {
					*normal = -*normal;
				}
			}
		}

		normals
	}

	#[allow(clippy::too_many_arguments)]
	pub(in crate::overlay) fn selection_flow_draw_layer(
		painter: &Painter,
		samples: &[(Pos2, f32)],
		normals: &[Vec2],
		line_width: f32,
		alpha_scale: f32,
		phase: f32,
		flow_band_width: f32,
		theme: HudTheme,
	) {
		if samples.is_empty() || normals.is_empty() || samples.len() != normals.len() {
			return;
		}

		let half = (line_width * 0.5).max(0.1);
		let n = samples.len();
		let mut mesh = Mesh::default();

		for i in 0..n {
			let (current_point, t) = samples[i];
			let movement = Self::selection_flow_flow_band(t, phase, flow_band_width);
			let intensity = SELECTION_FLOW_FLOW_BOOST * movement;
			let color = Self::selection_flow_color(t + phase, theme, alpha_scale, intensity);
			let normal = normals[i] * half;

			mesh.colored_vertex(current_point + normal, color);
			mesh.colored_vertex(current_point - normal, color);
		}
		for i in 0..n {
			let i0 = (i * 2) as u32;
			let i1 = ((i * 2) + 1) as u32;
			let n0 = (((i + 1) % n) * 2) as u32;
			let n1 = (((i + 1) % n) * 2 + 1) as u32;

			mesh.add_triangle(i0, i1, n0);
			mesh.add_triangle(i1, n1, n0);
		}

		painter.add(Shape::Mesh(mesh.into()));
	}

	pub(in crate::overlay) fn selection_flow_flow_band(
		progress: f32,
		phase: f32,
		band_width: f32,
	) -> f32 {
		let width = band_width.clamp(0.001, 0.5);
		let distance = (progress - phase).rem_euclid(1.0);
		let distance = distance.min(1.0 - distance);
		let normalized = (distance / width).min(1.0);

		(1.0 - normalized).powf(2.0)
	}

	pub(in crate::overlay) fn selection_flow_sample_count(perimeter: f32) -> usize {
		if perimeter <= 0.0 || !perimeter.is_finite() {
			return SELECTION_FLOW_MIN_SEGMENTS;
		}

		let by_step = (perimeter / SELECTION_FLOW_SAMPLE_STEP_PX).ceil() as usize;

		by_step.clamp(SELECTION_FLOW_MIN_SEGMENTS, SELECTION_FLOW_MAX_SEGMENTS)
	}

	pub(in crate::overlay) fn selection_flow_path_samples(
		rect: Rect,
		corner_radius: f32,
		sample_count: usize,
		start_offset: f32,
	) -> Vec<(Pos2, f32)> {
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);

		if perimeter <= 0.0 {
			return Vec::new();
		}

		let start = (start_offset / perimeter).rem_euclid(1.0);

		(0..sample_count)
			.map(|index| {
				let t = (index as f32 + 0.5) / sample_count as f32;
				let progress = (t + start).rem_euclid(1.0);

				(
					Self::selection_flow_sample_at_distance(
						rect,
						corner_radius,
						perimeter * progress,
					),
					t,
				)
			})
			.collect()
	}

	pub(in crate::overlay) fn selection_flow_sample_at_distance(
		rect: Rect,
		corner_radius: f32,
		distance: f32,
	) -> Pos2 {
		if corner_radius <= f32::EPSILON {
			let perimeter = Self::selection_flow_perimeter(rect, 0.0);
			let keep = distance.rem_euclid(perimeter);
			let edge_top = rect.width();
			let edge_right = rect.height();

			if keep < edge_top {
				return Pos2::new(rect.min.x + keep, rect.min.y);
			}
			if keep < edge_top + edge_right {
				return Pos2::new(rect.max.x, rect.min.y + (keep - edge_top));
			}
			if keep < edge_top * 2.0 + edge_right {
				return Pos2::new(rect.max.x - (keep - edge_top - edge_right), rect.max.y);
			}

			return Pos2::new(rect.min.x, rect.max.y - (keep - edge_top * 2.0 - edge_right));
		}

		let x0 = rect.min.x;
		let x1 = rect.max.x;
		let y0 = rect.min.y;
		let y1 = rect.max.y;
		let perimeter = Self::selection_flow_perimeter(rect, corner_radius);
		let remain = distance.rem_euclid(perimeter);
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = FRAC_PI_2 * corner_radius;

		if remain < edge_top_len {
			return Pos2::new(x0 + corner_radius + remain, y0);
		}

		let mut offset = remain - edge_top_len;

		if offset < corner_len {
			let angle = -FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x1, y0 + corner_radius + offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = offset / corner_radius;

			return Pos2::new(
				x1 - corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_top_len {
			return Pos2::new(x1 - corner_radius - offset, y1);
		}

		offset -= edge_top_len;

		if offset < corner_len {
			let angle = FRAC_PI_2 + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y1 - corner_radius + corner_radius * angle.sin(),
			);
		}

		offset -= corner_len;

		if offset < edge_right_len {
			return Pos2::new(x0, y1 - corner_radius - offset);
		}

		offset -= edge_right_len;

		if offset < corner_len {
			let angle = PI + offset / corner_radius;

			return Pos2::new(
				x0 + corner_radius + corner_radius * angle.cos(),
				y0 + corner_radius + corner_radius * angle.sin(),
			);
		}

		Pos2::new(x0 + corner_radius, y0)
	}

	pub(in crate::overlay) fn selection_flow_perimeter(rect: Rect, corner_radius: f32) -> f32 {
		let edge_top_len = (rect.width() - corner_radius * 2.0).max(0.0);
		let edge_right_len = (rect.height() - corner_radius * 2.0).max(0.0);
		let corner_len = FRAC_PI_2 * corner_radius;

		2.0 * (edge_top_len + edge_right_len) + 4.0 * corner_len
	}

	pub(in crate::overlay) fn selection_flow_color(
		progress: f32,
		theme: HudTheme,
		alpha_scale: f32,
		intensity: f32,
	) -> Color32 {
		let palette = Self::selection_flow_palette(theme);
		let normalized = progress.rem_euclid(1.0);
		let band_position = normalized * palette.len() as f32;
		let band = band_position.floor() as usize % palette.len();
		let local = band_position - band as f32;
		let (r0, g0, b0) = palette[band];
		let (r1, g1, b1) = palette[(band + 1) % palette.len()];
		let blend = |a: u8, b: u8, ratio: f32| -> u8 {
			(a as f32 + (b as f32 - a as f32) * ratio).clamp(0.0, 255.0).round() as u8
		};
		let theme_alpha = 1.0;
		let alpha = (255.0 * alpha_scale * intensity * theme_alpha).clamp(0.0, 255.0);

		Color32::from_rgba_unmultiplied(
			blend(r0, r1, local),
			blend(g0, g1, local),
			blend(b0, b1, local),
			alpha as u8,
		)
	}
}
