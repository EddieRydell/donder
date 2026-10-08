struct Uniforms {
    screen_zoom_min_radius: vec4<f32>,
    pan: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) center_radius: vec4<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let local = corners[input.vertex_index];
    let screen = uniforms.screen_zoom_min_radius.xy;
    let zoom = uniforms.screen_zoom_min_radius.z;
    let min_radius = uniforms.screen_zoom_min_radius.w;
    let center = input.center_radius.xy;
    let radius = max(input.center_radius.z * zoom, min_radius);
    let pixel = vec2<f32>(
        screen.x * 0.5 + (center.x - uniforms.pan.x) * zoom,
        screen.y * 0.5 - (center.y - uniforms.pan.y) * zoom,
    ) + local * radius;

    var output: VertexOutput;
    output.position = vec4<f32>(
        pixel.x / screen.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.y * 2.0,
        0.0,
        1.0,
    );
    output.local = local;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (dot(input.local, input.local) > 1.0) {
        discard;
    }
    return input.color;
}
