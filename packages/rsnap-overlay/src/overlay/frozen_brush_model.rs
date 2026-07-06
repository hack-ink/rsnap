#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use crate::overlay::session_state::{FROZEN_BRUSH_STROKE_WIDTH_POINTS, FrozenBrushStyle};
use crate::overlay::{ActiveFrozenBrushStroke, FrozenBrushModelState, Pos2, Vec2};

pub(in crate::overlay) const FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN: f32 = 0.12;
pub(in crate::overlay) const FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MAX: f32 = 0.96;
#[cfg(test)]
pub(in crate::overlay) const FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS: f32 =
	1.0 / 120.0;
pub(in crate::overlay) const FROZEN_BRUSH_RENDER_SAMPLE_STEP_POINTS: f32 = 0.25;

const FROZEN_BRUSH_POINT_SPACING_MIN_POINTS: f32 = 0.25;
const FROZEN_BRUSH_PREVIEW_POINT_SPACING_MIN_POINTS: f32 = 0.1;
const FROZEN_BRUSH_MODELED_POINT_SPACING_MIN_POINTS: f32 = 0.25;
const FROZEN_BRUSH_MODEL_SPEED_FLOOR_POINTS_PER_SECOND: f32 = 12.0;
const FROZEN_BRUSH_MODEL_SPEED_CEILING_POINTS_PER_SECOND: f32 = 1_200.0;
const FROZEN_BRUSH_MODEL_OUTPUT_RATE_HZ: f32 = 180.0;
const FROZEN_BRUSH_MODEL_TIMESTEP_SECONDS: f32 = 1.0 / FROZEN_BRUSH_MODEL_OUTPUT_RATE_HZ;
const FROZEN_BRUSH_MODEL_SPRING_CONSTANT: f32 = 540.0;
const FROZEN_BRUSH_MODEL_DRAG_CONSTANT: f32 = 42.0;
const FROZEN_BRUSH_MODEL_CURVE_TURN_RADIANS: f32 = 0.2;
const FROZEN_BRUSH_MODEL_CURVE_AMPLITUDE_POINTS: f32 = FROZEN_BRUSH_STROKE_WIDTH_POINTS * 0.08;
const FROZEN_BRUSH_MODEL_CURVE_RESPONSE_BOOST: f32 = 0.34;
const FROZEN_BRUSH_MODEL_FEATURE_TURN_RADIANS: f32 = 0.78;
const FROZEN_BRUSH_MODEL_SHARP_TURN_RADIANS: f32 = 1.45;
const FROZEN_BRUSH_MODEL_FEATURE_AMPLITUDE_POINTS: f32 = FROZEN_BRUSH_STROKE_WIDTH_POINTS * 0.22;
const FROZEN_BRUSH_STREAMLINE_RESPONSE_MIN: f32 = 0.18;
const FROZEN_BRUSH_STREAMLINE_RESPONSE_MAX: f32 = 0.78;
const FROZEN_BRUSH_STREAMLINE_DISTANCE_CEILING_POINTS: f32 = 6.0;
const FROZEN_BRUSH_PREVIEW_ROUNDING_PASSES: usize = 1;
const FROZEN_BRUSH_COMMIT_ROUNDING_PASSES: usize = 2;

pub(in crate::overlay) fn new_active_stroke(
	point: Pos2,
	sampled_at: Instant,
	style: FrozenBrushStyle,
) -> ActiveFrozenBrushStroke {
	ActiveFrozenBrushStroke {
		raw_points: vec![point],
		points: vec![point],
		style,
		model_state: FrozenBrushModelState {
			filtered_input_point: point,
			modeled_point: point,
			modeled_velocity: Vec2::ZERO,
			modeled_elapsed_seconds: 0.0,
		},
		started_at: sampled_at,
		last_sample_at: sampled_at,
	}
}

pub(in crate::overlay) fn append_raw_sample(
	active_stroke: &mut ActiveFrozenBrushStroke,
	point: Pos2,
	sampled_at: Instant,
) -> bool {
	if let Some(previous) = active_stroke.raw_points.last().copied()
		&& previous.distance(point) < FROZEN_BRUSH_POINT_SPACING_MIN_POINTS
	{
		return false;
	}

	let delta_seconds = sampled_at
		.saturating_duration_since(active_stroke.last_sample_at)
		.as_secs_f32()
		.max(1.0 / 240.0);

	active_stroke.raw_points.push(point);

	let snap_boost = feature_snap_boost(&active_stroke.raw_points);
	let response = input_response_with_feature_boost(
		&active_stroke.raw_points,
		delta_seconds,
		response_floor_boost(&active_stroke.raw_points, snap_boost),
	);

	active_stroke.model_state.filtered_input_point = Pos2::new(
		active_stroke.model_state.filtered_input_point.x
			+ ((point.x - active_stroke.model_state.filtered_input_point.x) * response),
		active_stroke.model_state.filtered_input_point.y
			+ ((point.y - active_stroke.model_state.filtered_input_point.y) * response),
	);

	let elapsed_seconds =
		sampled_at.saturating_duration_since(active_stroke.started_at).as_secs_f32();

	advance_model(&mut active_stroke.points, &mut active_stroke.model_state, elapsed_seconds);

	if snap_boost >= 1.0 {
		active_stroke.model_state.modeled_point = Pos2::new(
			active_stroke.model_state.modeled_point.x
				+ (point.x - active_stroke.model_state.modeled_point.x),
			active_stroke.model_state.modeled_point.y
				+ (point.y - active_stroke.model_state.modeled_point.y),
		);
		active_stroke.model_state.modeled_velocity *= 0.35;

		push_modeled_point(&mut active_stroke.points, active_stroke.model_state.modeled_point);
	} else if snap_boost > 0.0 {
		active_stroke.model_state.modeled_point = Pos2::new(
			active_stroke.model_state.modeled_point.x
				+ ((point.x - active_stroke.model_state.modeled_point.x) * 0.24),
			active_stroke.model_state.modeled_point.y
				+ ((point.y - active_stroke.model_state.modeled_point.y) * 0.24),
		);
		active_stroke.model_state.modeled_velocity *= 0.82;

		push_modeled_point(&mut active_stroke.points, active_stroke.model_state.modeled_point);
	}

	active_stroke.last_sample_at = sampled_at;

	true
}

#[cfg(test)]
pub(in crate::overlay) fn input_response(points: &[Pos2], delta_seconds: f32) -> f32 {
	let snap_boost = feature_snap_boost(points);

	input_response_with_feature_boost(
		points,
		delta_seconds,
		response_floor_boost(points, snap_boost),
	)
}

pub(in crate::overlay) fn finished_points(stroke: &ActiveFrozenBrushStroke) -> Vec<Pos2> {
	let source_points = if stroke.points.len() >= 2 { &stroke.points } else { &stroke.raw_points };

	processed_points(
		source_points,
		stroke.raw_points[0],
		stroke.raw_points[stroke.raw_points.len().saturating_sub(1)],
		FROZEN_BRUSH_COMMIT_ROUNDING_PASSES,
		true,
	)
}

pub(in crate::overlay) fn rendered_points(points: &[Pos2], sample_step: f32) -> Vec<Pos2> {
	match points {
		[] => Vec::new(),
		[first] => vec![*first],
		[first, second] => {
			let sample_step = sample_step.max(0.1);
			let mut rendered = vec![*first];

			append_linear_segment(&mut rendered, *first, *second, sample_step);

			rendered
		},
		_ => {
			let sample_step = sample_step.max(0.1);
			let mut rendered = Vec::with_capacity(points.len() * 6);

			rendered.push(points[0]);

			for index in 0..points.len().saturating_sub(1) {
				let previous = if index == 0 { points[0] } else { points[index - 1] };
				let start = points[index];
				let end = points[index + 1];
				let next = points
					.get(index + 2)
					.copied()
					.unwrap_or(points[points.len().saturating_sub(1)]);

				append_curve_segment(&mut rendered, previous, start, end, next, sample_step);
			}

			rendered
		},
	}
}

pub(in crate::overlay) fn active_display_points(
	active_stroke: &ActiveFrozenBrushStroke,
) -> Vec<Pos2> {
	let source_points = if active_stroke.points.len() >= 2 {
		&active_stroke.points
	} else {
		&active_stroke.raw_points
	};
	let rounding_passes = preview_rounding_passes(&active_stroke.raw_points);

	processed_points(
		source_points,
		active_stroke.raw_points[0],
		active_stroke.raw_points[active_stroke.raw_points.len().saturating_sub(1)],
		rounding_passes,
		false,
	)
}

pub(in crate::overlay) fn preview_points(active_stroke: &ActiveFrozenBrushStroke) -> Vec<Pos2> {
	active_display_points(active_stroke)
}

#[cfg(test)]
pub(in crate::overlay) fn corrected_points(points: &[Pos2]) -> Vec<Pos2> {
	if points.len() <= 2 {
		return points.to_vec();
	}

	let started_at = Instant::now();
	let mut stroke = new_active_stroke(points[0], started_at, FrozenBrushStyle::default());

	for (index, point) in points.iter().copied().enumerate().skip(1) {
		let sampled_at = started_at
			+ Duration::from_secs_f32(
				index as f32 * FROZEN_BRUSH_MODEL_SYNTHETIC_SAMPLE_INTERVAL_SECONDS,
			);

		append_raw_sample(&mut stroke, point, sampled_at);
	}

	finished_points(&stroke)
}

pub(in crate::overlay) fn turn_angle(previous: Pos2, current: Pos2, next: Pos2) -> f32 {
	let first = current - previous;
	let second = next - current;
	let first_length = first.length();
	let second_length = second.length();

	if first_length <= f32::EPSILON || second_length <= f32::EPSILON {
		return 0.0;
	}

	let cosine = (first.dot(second) / (first_length * second_length)).clamp(-1.0, 1.0);

	cosine.acos()
}

fn input_response_with_feature_boost(
	points: &[Pos2],
	delta_seconds: f32,
	feature_boost: f32,
) -> f32 {
	let speed_points_per_second = points
		.windows(2)
		.last()
		.map_or(0.0, |window| window[0].distance(window[1]) / delta_seconds);
	let normalized_speed = ((speed_points_per_second
		- FROZEN_BRUSH_MODEL_SPEED_FLOOR_POINTS_PER_SECOND)
		/ (FROZEN_BRUSH_MODEL_SPEED_CEILING_POINTS_PER_SECOND
			- FROZEN_BRUSH_MODEL_SPEED_FLOOR_POINTS_PER_SECOND))
		.clamp(0.0, 1.0);
	let base_response = FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN
		+ ((FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MAX - FROZEN_BRUSH_MODEL_INPUT_RESPONSE_MIN)
			* normalized_speed);

	base_response.max(feature_boost)
}

fn response_floor_boost(points: &[Pos2], snap_boost: f32) -> f32 {
	curve_response_boost(points).max(snap_boost)
}

fn preview_rounding_passes(points: &[Pos2]) -> usize {
	if has_sustained_curve_context(points) {
		FROZEN_BRUSH_PREVIEW_ROUNDING_PASSES + 1
	} else {
		FROZEN_BRUSH_PREVIEW_ROUNDING_PASSES
	}
}

fn curve_response_boost(points: &[Pos2]) -> f32 {
	if has_sustained_curve_context(points) { FROZEN_BRUSH_MODEL_CURVE_RESPONSE_BOOST } else { 0.0 }
}

fn feature_snap_boost(points: &[Pos2]) -> f32 {
	let len = points.len();

	if len < 3 {
		return 0.0;
	}

	let previous = points[len - 2];
	let current = points[len - 1];
	let anchor = points[len - 3];
	let current_turn_angle = turn_angle(anchor, previous, current);
	let current_amplitude = point_to_segment_distance(previous, anchor, current);

	if current_turn_angle < FROZEN_BRUSH_MODEL_FEATURE_TURN_RADIANS
		|| current_amplitude < FROZEN_BRUSH_MODEL_FEATURE_AMPLITUDE_POINTS
	{
		return 0.0;
	}
	if has_sustained_curve_context(points) {
		return 0.0;
	}
	if current_turn_angle >= FROZEN_BRUSH_MODEL_SHARP_TURN_RADIANS {
		return 1.0;
	}
	if len < 4 {
		return 0.0;
	}

	let support = points[len - 4];
	let previous_turn = signed_turn(support, anchor, previous);
	let current_turn = signed_turn(anchor, previous, current);
	let previous_turn_angle = turn_angle(support, anchor, previous);
	let previous_amplitude = point_to_segment_distance(anchor, support, previous);

	if previous_turn * current_turn < 0.0
		&& previous_turn_angle >= FROZEN_BRUSH_MODEL_FEATURE_TURN_RADIANS
		&& previous_amplitude >= FROZEN_BRUSH_MODEL_FEATURE_AMPLITUDE_POINTS
	{
		return 0.94;
	}

	0.0
}

fn has_sustained_curve_context(points: &[Pos2]) -> bool {
	let len = points.len();

	if len < 5 {
		return false;
	}

	let recent_points = &points[len - 5..];
	let mut turn_sign: f32 = 0.0;

	for window in recent_points.windows(3) {
		let signed_turn = signed_turn(window[0], window[1], window[2]);
		let turn_angle = turn_angle(window[0], window[1], window[2]);
		let amplitude = point_to_segment_distance(window[1], window[0], window[2]);

		if turn_angle < FROZEN_BRUSH_MODEL_CURVE_TURN_RADIANS
			|| amplitude < FROZEN_BRUSH_MODEL_CURVE_AMPLITUDE_POINTS
		{
			return false;
		}
		if turn_sign.abs() <= f32::EPSILON {
			turn_sign = signed_turn.signum();
		} else if signed_turn.signum() != turn_sign {
			return false;
		}
	}

	true
}

fn advance_model(
	points: &mut Vec<Pos2>,
	model_state: &mut FrozenBrushModelState,
	target_elapsed_seconds: f32,
) {
	if target_elapsed_seconds <= model_state.modeled_elapsed_seconds {
		return;
	}

	while model_state.modeled_elapsed_seconds + FROZEN_BRUSH_MODEL_TIMESTEP_SECONDS
		< target_elapsed_seconds
	{
		step_model(points, model_state, FROZEN_BRUSH_MODEL_TIMESTEP_SECONDS);
	}

	let remainder = target_elapsed_seconds - model_state.modeled_elapsed_seconds;

	if remainder > f32::EPSILON {
		step_model(points, model_state, remainder);
	}
}

fn step_model(points: &mut Vec<Pos2>, model_state: &mut FrozenBrushModelState, delta_seconds: f32) {
	let displacement = model_state.filtered_input_point - model_state.modeled_point;
	let acceleration = (displacement * FROZEN_BRUSH_MODEL_SPRING_CONSTANT)
		- (model_state.modeled_velocity * FROZEN_BRUSH_MODEL_DRAG_CONSTANT);

	model_state.modeled_velocity += acceleration * delta_seconds;
	model_state.modeled_point += model_state.modeled_velocity * delta_seconds;
	model_state.modeled_elapsed_seconds += delta_seconds;

	push_modeled_point(points, model_state.modeled_point);
}

fn processed_points(
	source_points: &[Pos2],
	start_point: Pos2,
	end_point: Pos2,
	rounding_passes: usize,
	streamline: bool,
) -> Vec<Pos2> {
	match source_points {
		[] => Vec::new(),
		[_] | [_, _] => vec![start_point, end_point],
		_ => {
			let streamlined =
				if streamline { streamlined_points(source_points) } else { source_points.to_vec() };
			let mut rounded = rounded_open_points(&streamlined, rounding_passes);

			if rounded.len() < 2 {
				return vec![start_point, end_point];
			}

			rounded[0] = start_point;

			if let Some(last) = rounded.last_mut() {
				*last = end_point;
			}

			rounded
		},
	}
}

fn streamlined_points(raw_points: &[Pos2]) -> Vec<Pos2> {
	let Some(first) = raw_points.first().copied() else {
		return Vec::new();
	};
	let mut streamlined = vec![first];
	let mut filtered = first;

	for window in raw_points.windows(2) {
		let previous_raw = window[0];
		let current_raw = window[1];
		let normalized_distance = ((previous_raw.distance(current_raw)
			- FROZEN_BRUSH_POINT_SPACING_MIN_POINTS)
			/ (FROZEN_BRUSH_STREAMLINE_DISTANCE_CEILING_POINTS
				- FROZEN_BRUSH_POINT_SPACING_MIN_POINTS))
			.clamp(0.0, 1.0);
		let response = FROZEN_BRUSH_STREAMLINE_RESPONSE_MIN
			+ ((FROZEN_BRUSH_STREAMLINE_RESPONSE_MAX - FROZEN_BRUSH_STREAMLINE_RESPONSE_MIN)
				* normalized_distance);

		filtered = Pos2::new(
			filtered.x + ((current_raw.x - filtered.x) * response),
			filtered.y + ((current_raw.y - filtered.y) * response),
		);

		push_processed_point(&mut streamlined, filtered);
	}

	streamlined[0] = raw_points[0];

	let last_raw = raw_points[raw_points.len().saturating_sub(1)];

	if let Some(last) = streamlined.last_mut() {
		*last = last_raw;
	}

	streamlined
}

fn rounded_open_points(points: &[Pos2], passes: usize) -> Vec<Pos2> {
	if points.len() <= 2 || passes == 0 {
		return points.to_vec();
	}

	let mut rounded = points.to_vec();

	for _ in 0..passes {
		if rounded.len() <= 2 {
			break;
		}

		let mut next = Vec::with_capacity((rounded.len() * 2).saturating_sub(2));

		next.push(rounded[0]);

		for window in rounded.windows(2) {
			let start = window[0];
			let end = window[1];
			let quarter =
				Pos2::new((start.x * 0.75) + (end.x * 0.25), (start.y * 0.75) + (end.y * 0.25));
			let three_quarters =
				Pos2::new((start.x * 0.25) + (end.x * 0.75), (start.y * 0.25) + (end.y * 0.75));

			push_processed_point(&mut next, quarter);
			push_processed_point(&mut next, three_quarters);
		}

		let last = rounded[rounded.len().saturating_sub(1)];

		if next.last().is_none_or(|point| {
			point.distance(last) > FROZEN_BRUSH_PREVIEW_POINT_SPACING_MIN_POINTS
		}) {
			next.push(last);
		} else if let Some(last_point) = next.last_mut() {
			*last_point = last;
		}

		rounded = next;
	}

	rounded
}

fn append_curve_segment(
	points: &mut Vec<Pos2>,
	previous: Pos2,
	start: Pos2,
	end: Pos2,
	next: Pos2,
	sample_step: f32,
) {
	let approximate_length = start.distance(end);
	let steps = ((approximate_length / sample_step).ceil().max(1.0)) as usize;

	for step in 1..=steps {
		let t = step as f32 / steps as f32;
		let point = catmull_rom_point(previous, start, end, next, t);

		push_sample_point(points, point);
	}
}

fn append_linear_segment(points: &mut Vec<Pos2>, start: Pos2, end: Pos2, sample_step: f32) {
	let approximate_length = start.distance(end);
	let steps = ((approximate_length / sample_step).ceil().max(1.0)) as usize;

	for step in 1..=steps {
		let t = step as f32 / steps as f32;
		let point = Pos2::new(start.x + (end.x - start.x) * t, start.y + (end.y - start.y) * t);

		push_sample_point(points, point);
	}
}

fn catmull_rom_point(previous: Pos2, start: Pos2, end: Pos2, next: Pos2, t: f32) -> Pos2 {
	const CENTRIPETAL_ALPHA: f32 = 0.5;
	const MIN_PARAMETER_STEP: f32 = 1.0e-3;

	let t0 = 0.0;
	let t1 = t0 + previous.distance(start).powf(CENTRIPETAL_ALPHA).max(MIN_PARAMETER_STEP);
	let t2 = t1 + start.distance(end).powf(CENTRIPETAL_ALPHA).max(MIN_PARAMETER_STEP);
	let t3 = t2 + end.distance(next).powf(CENTRIPETAL_ALPHA).max(MIN_PARAMETER_STEP);
	let sample_t = t1 + ((t2 - t1) * t.clamp(0.0, 1.0));
	let a1 = centripetal_catmull_rom_lerp(previous, start, t0, t1, sample_t);
	let a2 = centripetal_catmull_rom_lerp(start, end, t1, t2, sample_t);
	let a3 = centripetal_catmull_rom_lerp(end, next, t2, t3, sample_t);
	let b1 = centripetal_catmull_rom_lerp(a1, a2, t0, t2, sample_t);
	let b2 = centripetal_catmull_rom_lerp(a2, a3, t1, t3, sample_t);

	centripetal_catmull_rom_lerp(b1, b2, t1, t2, sample_t)
}

fn centripetal_catmull_rom_lerp(
	start: Pos2,
	end: Pos2,
	start_t: f32,
	end_t: f32,
	sample_t: f32,
) -> Pos2 {
	let span = (end_t - start_t).max(f32::EPSILON);
	let start_weight = (end_t - sample_t) / span;
	let end_weight = (sample_t - start_t) / span;

	Pos2::new(
		(start.x * start_weight) + (end.x * end_weight),
		(start.y * start_weight) + (end.y * end_weight),
	)
}

fn push_sample_point(points: &mut Vec<Pos2>, point: Pos2) {
	if points.last().is_none_or(|previous| previous.distance(point) > f32::EPSILON) {
		points.push(point);
	}
}

fn push_modeled_point(points: &mut Vec<Pos2>, point: Pos2) {
	let Some(previous) = points.last().copied() else {
		points.push(point);

		return;
	};

	if previous.distance(point) < f32::EPSILON {
		return;
	}
	if previous.distance(point) < FROZEN_BRUSH_MODELED_POINT_SPACING_MIN_POINTS && points.len() > 1
	{
		if let Some(last) = points.last_mut() {
			*last = point;
		}

		return;
	}

	points.push(point);
}

fn push_processed_point(points: &mut Vec<Pos2>, point: Pos2) {
	let Some(previous) = points.last().copied() else {
		points.push(point);

		return;
	};

	if previous.distance(point) < FROZEN_BRUSH_PREVIEW_POINT_SPACING_MIN_POINTS {
		if let Some(last) = points.last_mut() {
			*last = point;
		}

		return;
	}

	points.push(point);
}

fn point_to_segment_distance(point: Pos2, start: Pos2, end: Pos2) -> f32 {
	let segment_x = end.x - start.x;
	let segment_y = end.y - start.y;
	let segment_length_sq = (segment_x * segment_x) + (segment_y * segment_y);

	if segment_length_sq <= f32::EPSILON {
		return point.distance(start);
	}

	let t =
		(((point.x - start.x) * segment_x) + ((point.y - start.y) * segment_y)) / segment_length_sq;
	let t = t.clamp(0.0, 1.0);
	let projection = Pos2::new(start.x + (segment_x * t), start.y + (segment_y * t));

	point.distance(projection)
}

fn signed_turn(previous: Pos2, current: Pos2, next: Pos2) -> f32 {
	let first = current - previous;
	let second = next - current;

	(first.x * second.y) - (first.y * second.x)
}
