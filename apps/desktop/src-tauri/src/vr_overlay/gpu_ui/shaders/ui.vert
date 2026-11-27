#version 450

layout(push_constant) uniform DrawParams {
    vec2 rt_size;
    vec2 inv_rt_size;
    vec4 h_row0;
    vec4 h_row1;
    vec4 h_row2;
    vec4 card_info;
} pc;

layout(location = 0) in vec4 a_rect;
layout(location = 1) in vec4 a_uv;
layout(location = 2) in vec4 a_color;
layout(location = 3) in vec4 a_border_color;
layout(location = 4) in vec4 a_radii;
layout(location = 5) in vec4 a_clip_rect;
layout(location = 6) in vec2 a_clip_bw;
layout(location = 7) in uvec2 a_kind_flags;

layout(location = 0) out vec2 v_px;
layout(location = 1) out vec2 v_uv;
layout(location = 2) flat out vec4 v_rect;
layout(location = 3) flat out vec4 v_arc_uv;
layout(location = 4) flat out vec4 v_color;
layout(location = 5) flat out vec4 v_border_color;
layout(location = 6) flat out vec4 v_radii;
layout(location = 7) flat out vec4 v_clip_rect;
layout(location = 8) flat out vec2 v_clip_bw;
layout(location = 9) flat out uvec2 v_kind_flags;

vec2 quad_corner(uint vid) {
    uint i = vid % 6u;
    if (i == 0u || i == 3u) return vec2(0.0, 0.0);
    if (i == 1u) return vec2(1.0, 0.0);
    if (i == 2u || i == 4u) return vec2(1.0, 1.0);
    return vec2(0.0, 1.0);
}

void main() {
    vec2 c = quad_corner(uint(gl_VertexIndex));
    uint kind = a_kind_flags.x;

    vec4 qbox = a_rect;
    if (kind == 0u || kind == 3u) {
        qbox = vec4(a_rect.xy - 1.5, a_rect.zw + 3.0);
    } else if (kind == 4u) {
        vec2 p0 = a_rect.xy;
        vec2 p1 = a_rect.zw;
        float pad = a_clip_bw.y * 0.5 + 2.0;
        vec2 mn = min(p0, p1) - pad;
        vec2 mx = max(p0, p1) + pad;
        qbox = vec4(mn, mx - mn);
    }

    vec2 px = qbox.xy + c * qbox.zw;
    vec2 ndc = vec2(px.x * pc.inv_rt_size.x * 2.0 - 1.0, px.y * pc.inv_rt_size.y * 2.0 - 1.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_px = px;
    v_uv = mix(a_uv.xy, a_uv.zw, c);
    v_rect = a_rect;
    v_arc_uv = a_uv;
    v_color = a_color;
    v_border_color = a_border_color;
    v_radii = a_radii;
    v_clip_rect = a_clip_rect;
    v_clip_bw = a_clip_bw;
    v_kind_flags = a_kind_flags;
}
