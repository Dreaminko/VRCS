#version 450

layout(push_constant) uniform DrawParams {
    vec2 rt_size;
    vec2 inv_rt_size;
    vec4 h_row0;
    vec4 h_row1;
    vec4 h_row2;
    vec4 card_info;
} pc;

layout(location = 0) out vec2 v_uv;

vec2 quad_corner(uint vid) {
    uint i = vid % 6u;
    if (i == 0u || i == 3u) return vec2(0.0, 0.0);
    if (i == 1u) return vec2(1.0, 0.0);
    if (i == 2u || i == 4u) return vec2(1.0, 1.0);
    return vec2(0.0, 1.0);
}

void main() {
    vec2 c = quad_corner(uint(gl_VertexIndex));
    vec2 local = c * pc.card_info.xy;
    float hx = dot(pc.h_row0.xyz, vec3(local, 1.0));
    float hy = dot(pc.h_row1.xyz, vec3(local, 1.0));
    float hw = dot(pc.h_row2.xyz, vec3(local, 1.0));
    float ndc_x = hx * pc.inv_rt_size.x * 2.0 - hw;
    float ndc_y = hy * pc.inv_rt_size.y * 2.0 - hw;
    gl_Position = vec4(ndc_x, ndc_y, 0.0, hw);
    v_uv = c * pc.card_info.zw;
}
