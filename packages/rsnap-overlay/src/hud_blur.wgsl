struct VsOut {
	@builtin(position) pos: vec4<f32>,
	@location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
	// Fullscreen triangle.
	// This blur path is monitor-surface aligned, so macOS toolbar uses native window blur
	// instead of this shader for now.
	var pos = array<vec2<f32>, 3>(
		vec2<f32>(-1.0, -1.0),
		vec2<f32>( 3.0, -1.0),
		vec2<f32>(-1.0,  3.0),
	);
	var uv = array<vec2<f32>, 3>(
		// Match the fullscreen background path so blur samples the same orientation as what's shown.
		vec2<f32>(0.0,  1.0),
		vec2<f32>(2.0,  1.0),
		vec2<f32>(0.0, -1.0),
	);

	var out: VsOut;
	out.pos = vec4<f32>(pos[vertex_index], 0.0, 1.0);
	out.uv = uv[vertex_index];
	return out;
}

struct HudBlurUniform {
	// min.xy, size.xy in *physical pixels* (monitor surface coordinates).
	// Toolbar windows rendered with native macOS blur bypass this shader path today; if they are
	// later switched to shader blur, this struct needs a per-window source texture.
	rect_min_size: vec4<f32>,
	// radius_px, blur_radius_px, edge_softness_px, _pad
	radius_blur_soft: vec4<f32>,
	// surface_size_px.xy, _pad
	surface_size_px: vec4<f32>,
	// Reserved tint payload, not used by this pass.
	tint_rgba: vec4<f32>,
	// blur_amount, tint_amount, max_lod, _pad
	effects: vec4<f32>,
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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	let surface_size = u.surface_size_px.xy;
	if surface_size.x <= 0.0 || surface_size.y <= 0.0 {
		return vec4<f32>(0.0);
	}

	let pos = in.pos;
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

	let uv2 = in.uv;
	let blur_amount = clamp(u.effects.x, 0.0, 1.0);
	// Prevent over-blurring into a low-res "zoomed" look where the user can no longer correlate
	// what is behind the HUD.
	let max_lod = min(max(u.effects.z, 0.0), 7.0);
	// Stronger curve so mid values feel like "frosted glass" instead of a mild reflection.
	let lod = clamp(pow(blur_amount, 0.55) * max_lod, 0.0, max_lod);
	var blurred = textureSampleLevel(bg_tex, bg_samp, uv2, lod).rgb;
	let blur_radius_px = max(u.radius_blur_soft.y, 0.0);
	if blur_radius_px > 0.01 {
		let off = vec2<f32>(blur_radius_px / surface_size.x, blur_radius_px / surface_size.y);

		let c = textureSampleLevel(bg_tex, bg_samp, uv2, lod).rgb * 4.0
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>( off.x, 0.0), lod).rgb * 2.0
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>(-off.x, 0.0), lod).rgb * 2.0
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>(0.0,  off.y), lod).rgb * 2.0
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>(0.0, -off.y), lod).rgb * 2.0
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>( off.x,  off.y), lod).rgb
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>(-off.x,  off.y), lod).rgb
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>( off.x, -off.y), lod).rgb
			+ textureSampleLevel(bg_tex, bg_samp, uv2 + vec2<f32>(-off.x, -off.y), lod).rgb;

		blurred = c / 16.0;
	}

	return vec4<f32>(blurred * alpha, alpha);
}
