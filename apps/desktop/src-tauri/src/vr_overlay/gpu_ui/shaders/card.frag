#version 450

layout(set = 0, binding = 2) uniform sampler2D scratch_tex;
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;

void main() {
    vec4 s = texture(scratch_tex, v_uv);
    if (s.a <= 0.0005) {
        discard;
    }
    out_color = s;
}
