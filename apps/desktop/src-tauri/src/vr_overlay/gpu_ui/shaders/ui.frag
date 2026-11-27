#version 450

layout(set = 0, binding = 0) uniform sampler2D r8_atlas;
layout(set = 0, binding = 1) uniform sampler2D rgba_atlas;

layout(location = 0) in vec2 v_px;
layout(location = 1) in vec2 v_uv;
layout(location = 2) flat in vec4 v_rect;
layout(location = 3) flat in vec4 v_arc_uv;
layout(location = 4) flat in vec4 v_color;
layout(location = 5) flat in vec4 v_border_color;
layout(location = 6) flat in vec4 v_radii;
layout(location = 7) flat in vec4 v_clip_rect;
layout(location = 8) flat in vec2 v_clip_bw;
layout(location = 9) flat in uvec2 v_kind_flags;

layout(location = 0) out vec4 out_color;

float sd_rounded_box(vec2 p, vec2 half_size, vec4 radii) {
    vec2 rr = (p.x >= 0.0) ? vec2(radii.y, radii.z) : vec2(radii.x, radii.w);
    float r = (p.y < 0.0) ? rr.x : rr.y;
    r = min(r, min(half_size.x, half_size.y));
    vec2 q = abs(p) - half_size + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

void main() {
    vec2 c_half = (v_clip_rect.zw - v_clip_rect.xy) * 0.5;
    vec2 c_center = (v_clip_rect.xy + v_clip_rect.zw) * 0.5;
    float cr = min(v_clip_bw.x, min(c_half.x, c_half.y));
    float d_clip = sd_rounded_box(v_px - c_center, c_half, vec4(cr));
    float clip_alpha = clamp(0.5 - d_clip, 0.0, 1.0);
    if (clip_alpha <= 0.001) {
        discard;
    }

    uint kind = v_kind_flags.x;
    uint flags = v_kind_flags.y;
    vec4 out_pm = vec4(0.0);

    if (kind == 0u) {
        vec2 half_size = v_rect.zw * 0.5;
        vec2 p = v_px - (v_rect.xy + half_size);
        float d = sd_rounded_box(p, half_size, v_radii);
        float outer_cov = clamp(0.5 - d, 0.0, 1.0);
        if (outer_cov <= 0.001) {
            discard;
        }
        vec4 fill_pm = v_color * outer_cov;
        float bw = v_clip_bw.y;
        if (bw > 0.0 && v_border_color.a > 0.0) {
            float inner_cov = clamp(0.5 - (d + bw), 0.0, 1.0);
            float ring = clamp(outer_cov - inner_cov, 0.0, 1.0);
            float pat = 1.0;
            if ((flags & 3u) != 0u) {
                float coord = (abs(p.x) * half_size.y > abs(p.y) * half_size.x) ? p.y : p.x;
                if ((flags & 1u) != 0u) {
                    float m = mod(abs(coord) + 1024.0, 8.0);
                    pat = clamp(1.0 - max(m - 4.0, 0.0), 0.0, 1.0);
                } else {
                    float m = mod(abs(coord) + 1024.0, 6.0);
                    pat = clamp(1.0 - max(m - 2.5, 0.0), 0.0, 1.0);
                }
            }
            vec4 border_pm = v_border_color * (ring * pat);
            out_pm = border_pm + fill_pm * (1.0 - border_pm.a);
        } else {
            out_pm = fill_pm;
        }
    } else if (kind == 1u) {
        float cov = texture(r8_atlas, v_uv).r;
        out_pm = v_color * cov;
    } else if (kind == 2u) {
        vec4 tex = texture(rgba_atlas, v_uv);
        out_pm = tex * v_color;
    } else if (kind == 3u) {
        vec2 half_size = v_rect.zw * 0.5;
        vec2 dp = v_px - (v_rect.xy + half_size);
        float start_rad = v_arc_uv.x;
        float sweep_rad = v_arc_uv.y;
        float stroke_px = v_arc_uv.z;
        if (sweep_rad <= 0.001) {
            discard;
        }
        float radius = min(half_size.x, half_size.y) - stroke_px * 0.5;
        float d_rad = abs(length(dp) - radius) - stroke_px * 0.5;
        float d = d_rad;
        if (sweep_rad < 6.2831) {
            float angle = atan(dp.y, dp.x);
            float rel = mod(angle - start_rad, 6.28318530718);
            if (rel < 0.0) rel += 6.28318530718;
            if (rel > sweep_rad) {
                vec2 c0 = radius * vec2(cos(start_rad), sin(start_rad));
                vec2 c1 = radius * vec2(cos(start_rad + sweep_rad), sin(start_rad + sweep_rad));
                d = min(length(dp - c0), length(dp - c1)) - stroke_px * 0.5;
            }
        }
        out_pm = v_color * clamp(0.5 - d, 0.0, 1.0);
    } else if (kind == 4u) {
        vec2 p0 = v_rect.xy;
        vec2 p1 = v_rect.zw;
        vec2 pa = v_px - p0;
        vec2 ba = p1 - p0;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        float d = length(pa - ba * h) - v_clip_bw.y * 0.5;
        out_pm = v_color * clamp(0.5 - d, 0.0, 1.0);
    }

    out_pm *= clip_alpha;
    if (out_pm.a <= 0.0005) {
        discard;
    }
    out_color = out_pm;
}
