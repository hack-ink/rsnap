struct VsOut {
	@builtin(position) pos: vec4<f32>,
	@location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
	// Fullscreen triangle.
	var pos = array<vec2<f32>, 3>(
		vec2<f32>(-1.0, -1.0),
		vec2<f32>( 3.0, -1.0),
		vec2<f32>(-1.0,  3.0),
	);
	var uv = array<vec2<f32>, 3>(
		vec2<f32>(0.0, 0.0),
		vec2<f32>(2.0, 0.0),
		vec2<f32>(0.0, 2.0),
	);

	var out: VsOut;
	out.pos = vec4<f32>(pos[vertex_index], 0.0, 1.0);
	out.uv = uv[vertex_index];
	return out;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
	return textureSample(src_tex, src_samp, in.uv);
}

@fragment
fn fs_main_macos_surface(in: VsOut) -> @location(0) vec4<f32> {
	let flipped_uv = vec2<f32>(in.uv.x, 1.0 - in.uv.y);

	return textureSample(src_tex, src_samp, flipped_uv);
}
