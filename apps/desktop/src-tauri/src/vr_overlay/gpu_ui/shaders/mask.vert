#version 450

layout(push_constant) uniform DrawParams {
    vec2 rt_size;
    vec2 inv_rt_size;
    vec4 h_row0;
    vec4 h_row1;
    vec4 h_row2;
    vec4 card_info;
} pc;

layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec4 a_color;
layout(location = 0) out vec4 v_color;

void main() {
    vec2 ndc = vec2(a_pos.x * pc.inv_rt_size.x * 2.0 - 1.0, a_pos.y * pc.inv_rt_size.y * 2.0 - 1.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_color = a_color;
}
