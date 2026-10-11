// Flat-coloured blocks: the face shading is already baked into the vertex colours.

struct Uniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.position = uniforms.view_proj * vec4<f32>(position, 1.0);
    out.color = color;
    return out;
}

// Vertex colours are sRGB values: written as they are to a non-sRGB target (what egui prefers)...
@fragment
fn fs_gamma_framebuffer(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// ...and decoded first for an sRGB target, which encodes them again.
@fragment
fn fs_linear_framebuffer(in: VertexOutput) -> @location(0) vec4<f32> {
    let c = in.color.rgb;
    let linear = select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
    return vec4<f32>(linear, in.color.a);
}
