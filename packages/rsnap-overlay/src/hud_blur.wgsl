struct VsOut {
	@builtin(position) pos: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
	// Fullscreen triangle.
	var pos = array<vec2<f32>, 3>(
		vec2<f32>(-1.0, -1.0),
		vec2<f32>( 3.0, -1.0),
		vec2<f32>(-1.0,  3.0),
	);

	var out: VsOut;
	out.pos = vec4<f32>(pos[vertex_index], 0.0, 1.0);
	return out;
}

struct HudBlurUniform {
	// min.xy, size.xy in *physical pixels* (surface coordinates).
	rect_min_size: vec4<f32>,
	// radius_px, blur_radius_px, edge_softness_px, _pad
	radius_blur_soft: vec4<f32>,
	// surface_size_px.xy, _pad
	surface_size_px: vec4<f32>,
	// tint_rgb (linear), tint_alpha
	tint_rgba: vec4<f32>,
}

@group(0) @binding(0) var bg_tex: texture_2d<f32>;
@group(0) @binding(1) var bg_samp: sampler;
@group(0) @binding(2) var<uniform> u: HudBlurUniform;

fn sd_rounded_rect(p: vec2<f32>, center: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
	// Standard rounded-rect SDF.
	let r = min(radius, min(half_size.x, half_size.y));
	let q = abs(p - center) - (half_size - vec2<f32>(r));
	let outside = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
	return outside;
}

fn sample_blur(uv: vec2<f32>, delta: vec2<f32>) -> vec3<f32> {
	// 13 taps (center + 3 rings in axial directions).
	let c0 = textureSample(bg_tex, bg_samp, uv).rgb;

	let c1x1 = textureSample(bg_tex, bg_samp, uv + vec2<f32>( delta.x, 0.0)).rgb;
	let c1x2 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(-delta.x, 0.0)).rgb;
	let c1y1 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(0.0,  delta.y)).rgb;
	let c1y2 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(0.0, -delta.y)).rgb;

	let c2x1 = textureSample(bg_tex, bg_samp, uv + vec2<f32>( 2.0 * delta.x, 0.0)).rgb;
	let c2x2 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(-2.0 * delta.x, 0.0)).rgb;
	let c2y1 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(0.0,  2.0 * delta.y)).rgb;
	let c2y2 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(0.0, -2.0 * delta.y)).rgb;

	let c3x1 = textureSample(bg_tex, bg_samp, uv + vec2<f32>( 3.0 * delta.x, 0.0)).rgb;
	let c3x2 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(-3.0 * delta.x, 0.0)).rgb;
	let c3y1 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(0.0,  3.0 * delta.y)).rgb;
	let c3y2 = textureSample(bg_tex, bg_samp, uv + vec2<f32>(0.0, -3.0 * delta.y)).rgb;

	let w0 = 0.30;
	let w1 = 0.14;
	let w2 = 0.09;
	let w3 = 0.06;
	let sum =
		(w0 * c0) +
		(w1 * (c1x1 + c1x2 + c1y1 + c1y2)) +
		(w2 * (c2x1 + c2x2 + c2y1 + c2y2)) +
		(w3 * (c3x1 + c3x2 + c3y1 + c3y2));
	let norm = w0 + 4.0 * (w1 + w2 + w3);
	return sum / norm;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
	let surface_size = u.surface_size_px.xy;
	if surface_size.x <= 0.0 || surface_size.y <= 0.0 {
		return vec4<f32>(0.0);
	}

	let rect_min = u.rect_min_size.xy;
	let rect_size = max(u.rect_min_size.zw, vec2<f32>(0.0));
	let center = rect_min + (rect_size * 0.5);
	let half_size = rect_size * 0.5;
	let radius_px = max(u.radius_blur_soft.x, 0.0);
	let softness_px = max(u.radius_blur_soft.z, 0.0);
	let sd = sd_rounded_rect(pos.xy, center, half_size, radius_px);
	let alpha = 1.0 - smoothstep(0.0, softness_px, sd);
	if alpha <= 0.0 {
		return vec4<f32>(0.0);
	}

	let uv = pos.xy / surface_size;
	let blur_radius_px = max(u.radius_blur_soft.y, 0.0);
	let delta = vec2<f32>(blur_radius_px) / surface_size;
	let blurred = sample_blur(uv, delta);

	let tint = u.tint_rgba;
	let color = mix(blurred, tint.rgb, tint.a);

	return vec4<f32>(color * alpha, alpha);
}
