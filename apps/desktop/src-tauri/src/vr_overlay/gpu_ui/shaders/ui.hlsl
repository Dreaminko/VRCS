cbuffer DrawParams : register(b0) {
    float2 rt_size;
    float2 inv_rt_size;
    float4 h_row0;
    float4 h_row1;
    float4 h_row2;
    float4 card_info; // [card_w, card_h, card_w / scratch_w, card_h / scratch_h]
};

Texture2D<float> r8_atlas : register(t0);
Texture2D<float4> rgba_atlas : register(t1);
Texture2D<float4> scratch_tex : register(t2);
SamplerState linear_clamp : register(s0);
SamplerState card_sampler : register(s1);

struct UiVsIn {
    uint vertex_id : SV_VertexID;
    float4 rect : RECT;
    float4 uv : UV;
    float4 color : COLOR;
    float4 border_color : BORDER_COLOR;
    float4 radii : RADII;
    float4 clip_rect : CLIP_RECT;
    float2 clip_bw : CLIP_BW;
    uint2 kind_flags : KIND_FLAGS;
};

struct UiPsIn {
    float4 pos : SV_Position;
    float2 px : TEXCOORD0;
    float2 uv : TEXCOORD1;
    nointerpolation float4 rect : TEXCOORD2;
    nointerpolation float4 arc_uv : TEXCOORD3;
    nointerpolation float4 color : COLOR0;
    nointerpolation float4 border_color : COLOR1;
    nointerpolation float4 radii : TEXCOORD4;
    nointerpolation float4 clip_rect : TEXCOORD5;
    nointerpolation float2 clip_bw : TEXCOORD6;
    nointerpolation uint2 kind_flags : TEXCOORD7;
};

float2 quad_corner(uint vid) {
    // Two triangles: (0,0), (1,0), (1,1), (0,0), (1,1), (0,1)
    uint i = vid % 6;
    if (i == 0 || i == 3) return float2(0.0, 0.0);
    if (i == 1) return float2(1.0, 0.0);
    if (i == 2 || i == 4) return float2(1.0, 1.0);
    return float2(0.0, 1.0);
}

UiPsIn UiVS(UiVsIn input) {
    UiPsIn o;
    float2 c = quad_corner(input.vertex_id);
    uint kind = input.kind_flags.x;

    float4 qbox = input.rect;
    if (kind == 0 || kind == 3) {
        qbox = float4(input.rect.xy - 1.5, input.rect.zw + 3.0);
    } else if (kind == 4) {
        float2 p0 = input.rect.xy;
        float2 p1 = input.rect.zw;
        float pad = input.clip_bw.y * 0.5 + 2.0;
        float2 mn = min(p0, p1) - pad;
        float2 mx = max(p0, p1) + pad;
        qbox = float4(mn, mx - mn);
    }

    float2 px = qbox.xy + c * qbox.zw;
    float2 ndc = float2(px.x * inv_rt_size.x * 2.0 - 1.0, 1.0 - px.y * inv_rt_size.y * 2.0);
    o.pos = float4(ndc, 0.0, 1.0);
    o.px = px;
    o.uv = lerp(input.uv.xy, input.uv.zw, c);
    o.rect = input.rect;
    o.arc_uv = input.uv;
    o.color = input.color;
    o.border_color = input.border_color;
    o.radii = input.radii;
    o.clip_rect = input.clip_rect;
    o.clip_bw = input.clip_bw;
    o.kind_flags = input.kind_flags;
    return o;
}

float sd_rounded_box(float2 p, float2 half_size, float4 radii) {
    float2 rr = (p.x >= 0.0) ? float2(radii.y, radii.z) : float2(radii.x, radii.w);
    float r = (p.y < 0.0) ? rr.x : rr.y;
    r = min(r, min(half_size.x, half_size.y));
    float2 q = abs(p) - half_size + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

float4 UiPS(UiPsIn input) : SV_Target {
    // Scissor rounded clip
    float2 c_half = (input.clip_rect.zw - input.clip_rect.xy) * 0.5;
    float2 c_center = (input.clip_rect.xy + input.clip_rect.zw) * 0.5;
    float cr = min(input.clip_bw.x, min(c_half.x, c_half.y));
    float d_clip = sd_rounded_box(input.px - c_center, c_half, float4(cr, cr, cr, cr));
    float clip_alpha = saturate(0.5 - d_clip);
    if (clip_alpha <= 0.001) {
        discard;
    }

    uint kind = input.kind_flags.x;
    uint flags = input.kind_flags.y;
    float4 out_pm = float4(0.0, 0.0, 0.0, 0.0);

    if (kind == 0) {
        // SDF rounded rect with border
        float2 half_size = input.rect.zw * 0.5;
        float2 p = input.px - (input.rect.xy + half_size);
        float d = sd_rounded_box(p, half_size, input.radii);
        float outer_cov = saturate(0.5 - d);
        if (outer_cov <= 0.001) {
            discard;
        }
        float4 fill_pm = input.color * outer_cov;
        float bw = input.clip_bw.y;
        if (bw > 0.0 && input.border_color.a > 0.0) {
            float inner_cov = saturate(0.5 - (d + bw));
            float ring = saturate(outer_cov - inner_cov);
            float pat = 1.0;
            if ((flags & 3u) != 0u) {
                float coord = (abs(p.x) * half_size.y > abs(p.y) * half_size.x) ? p.y : p.x;
                if ((flags & 1u) != 0u) {
                    float m = fmod(abs(coord) + 1024.0, 8.0);
                    pat = saturate(1.0 - max(m - 4.0, 0.0));
                } else {
                    float m = fmod(abs(coord) + 1024.0, 6.0);
                    pat = saturate(1.0 - max(m - 2.5, 0.0));
                }
            }
            float4 border_pm = input.border_color * (ring * pat);
            out_pm = border_pm + fill_pm * (1.0 - border_pm.a);
        } else {
            out_pm = fill_pm;
        }
    } else if (kind == 1) {
        // R8 atlas mask
        float cov = r8_atlas.Sample(linear_clamp, input.uv);
        out_pm = input.color * cov;
    } else if (kind == 2) {
        // RGBA8 premultiplied sprite
        float4 tex = rgba_atlas.Sample(linear_clamp, input.uv);
        out_pm = tex * input.color;
    } else if (kind == 3) {
        // Circular progress arc with round end caps
        float2 half_size = input.rect.zw * 0.5;
        float2 dp = input.px - (input.rect.xy + half_size);
        float start_rad = input.arc_uv.x;
        float sweep_rad = input.arc_uv.y;
        float stroke_px = input.arc_uv.z;
        if (sweep_rad <= 0.001) {
            discard;
        }
        float radius = min(half_size.x, half_size.y) - stroke_px * 0.5;
        float d_rad = abs(length(dp) - radius) - stroke_px * 0.5;
        float d = d_rad;
        if (sweep_rad < 6.2831) {
            float angle = atan2(dp.y, dp.x);
            float rel = fmod(angle - start_rad, 6.28318530718);
            if (rel < 0.0) rel += 6.28318530718;
            if (rel > sweep_rad) {
                float2 c0 = radius * float2(cos(start_rad), sin(start_rad));
                float2 c1 = radius * float2(cos(start_rad + sweep_rad), sin(start_rad + sweep_rad));
                d = min(length(dp - c0), length(dp - c1)) - stroke_px * 0.5;
            }
        }
        out_pm = input.color * saturate(0.5 - d);
    } else if (kind == 4) {
        // Anti-aliased round-cap line
        float2 p0 = input.rect.xy;
        float2 p1 = input.rect.zw;
        float2 pa = input.px - p0;
        float2 ba = p1 - p0;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        float d = length(pa - ba * h) - input.clip_bw.y * 0.5;
        out_pm = input.color * saturate(0.5 - d);
    }

    out_pm *= clip_alpha;
    if (out_pm.a <= 0.0005) {
        discard;
    }
    return out_pm;
}

struct MaskVsIn {
    float2 pos : POSITION;
    float4 color : COLOR;
};

struct MaskPsIn {
    float4 pos : SV_Position;
    float4 color : COLOR;
};

MaskPsIn MaskVS(MaskVsIn input) {
    MaskPsIn o;
    float2 ndc = float2(input.pos.x * inv_rt_size.x * 2.0 - 1.0, 1.0 - input.pos.y * inv_rt_size.y * 2.0);
    o.pos = float4(ndc, 0.0, 1.0);
    o.color = input.color;
    return o;
}

float4 MaskPS(MaskPsIn input) : SV_Target {
    return input.color;
}

struct CardPsIn {
    float4 pos : SV_Position;
    float2 uv : TEXCOORD0;
};

CardPsIn CardQuadVS(uint vertex_id : SV_VertexID) {
    CardPsIn o;
    float2 c = quad_corner(vertex_id);
    float2 local = c * card_info.xy;
    float hx = dot(h_row0.xyz, float3(local, 1.0));
    float hy = dot(h_row1.xyz, float3(local, 1.0));
    float hw = dot(h_row2.xyz, float3(local, 1.0));
    float ndc_x = hx * inv_rt_size.x * 2.0 - hw;
    float ndc_y = hw - hy * inv_rt_size.y * 2.0;
    o.pos = float4(ndc_x, ndc_y, 0.0, hw);
    o.uv = c * card_info.zw;
    return o;
}

float4 CardQuadPS(CardPsIn input) : SV_Target {
    float4 s = scratch_tex.Sample(card_sampler, input.uv);
    if (s.a <= 0.0005) {
        discard;
    }
    return s;
}
