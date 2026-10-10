// Draws one mip level from the level above. Both are `Rgba8UnormSrgb`, so the sampler averages in
// linear light and the result is re-encoded: 1 px black-and-white stripes shrink to sRGB 188, not
// the too-dark 128 that averaging the encoded values would give.

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle that covers the target: uv (0, 0), (2, 0), (0, 2).
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return VertexOut(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

// Each target pixel centre lands on the corner shared by its 2 × 2 source pixels, so the linear
// filter is their exact average on even sizes.
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, source_sampler, in.uv, 0.0);
}
