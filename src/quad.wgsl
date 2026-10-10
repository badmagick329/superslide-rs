// The one quad every image is drawn with. `Transform` in view.rs documents the coordinates.

struct Uniforms {
    // Quad-local coordinates, -0.5 to 0.5 on each axis, to window pixels.
    matrix: mat2x2<f32>,
    // Window pixel of the image centre.
    offset: vec2<f32>,
    // Window size in pixels.
    viewport: vec2<f32>,
};

// Group 0 is per frame (the sampler carries the magnification filter); group 1 is per image.
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var image_sampler: sampler;
@group(1) @binding(0) var image: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Four vertices as a triangle strip; no vertex buffer.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let pixel = u.matrix * (uv - 0.5) + u.offset;
    // Window pixels have y down; clip space has y up.
    let clip = vec2<f32>(pixel.x / u.viewport.x * 2.0 - 1.0, 1.0 - pixel.y / u.viewport.y * 2.0);
    return VertexOut(vec4<f32>(clip, 0.0, 1.0), uv);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, in.uv);
}
