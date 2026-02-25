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

fn hash12(p: vec2<f32>) -> f32 {
	// Simple hash suitable for per-pixel noise/rotation.
	let h = dot(p, vec2<f32>(127.1, 311.7));
	return fract(sin(h) * 43758.5453123);
}

fn rot(v: vec2<f32>, a: f32) -> vec2<f32> {
	let s = sin(a);
	let c = cos(a);
	return vec2<f32>(c * v.x - s * v.y, s * v.x + c * v.y);
}

fn sample_blur(uv: vec2<f32>, delta: vec2<f32>, seed: vec2<f32>) -> vec3<f32> {
	// Poisson-ish disk taps + per-pixel rotation to avoid the obvious "ghost copies"
	// you get with grid kernels.
	let angle = hash12(seed) * 6.28318530718;
	var sum = vec3<f32>(0.0);
	var weight_sum = 0.0;

	let taps = array<vec2<f32>, 16>(
		vec2<f32>( 0.170019, -0.040254),
		vec2<f32>( 0.063326,  0.142369),
		vec2<f32>( 0.203528,  0.214331),
		vec2<f32>(-0.098422, -0.295755),
		vec2<f32>( 0.421003,  0.027070),
		vec2<f32>(-0.299417,  0.791925),
		vec2<f32>( 0.645680,  0.493210),
		vec2<f32>(-0.651784,  0.717887),
		vec2<f32>(-0.705374, -0.668203),
		vec2<f32>( 0.667531, -0.578772),
		vec2<f32>(-0.613392,  0.617481),
		vec2<f32>( 0.566637,  0.605213),
		vec2<f32>(-0.817194, -0.271096),
		vec2<f32>( 0.977050, -0.108615),
		vec2<f32>(-0.885922,  0.215369),
		vec2<f32>( 0.527837, -0.085868),
	);

	for (var i: u32 = 0u; i < 16u; i = i + 1u) {
		let o = rot(taps[i], angle);
		let d2 = dot(o, o);
		let w = exp(-3.0 * d2);
		sum += textureSample(bg_tex, bg_samp, uv + o * delta).rgb * w;
		weight_sum += w;
	}

	// Add the center tap with a higher weight for stability.
	let center_w = 1.25;
	sum += textureSample(bg_tex, bg_samp, uv).rgb * center_w;
	weight_sum += center_w;

	return sum / max(weight_sum, 1.0);
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
	let blurred = sample_blur(uv, delta, pos.xy);

	let tint = u.tint_rgba;
	let color = mix(blurred, tint.rgb, tint.a);

	return vec4<f32>(color * alpha, alpha);
}
