//! Rust-native GPU UI layout and rasterization for all VR Overlay surfaces.

pub mod caption;
pub mod draw;
pub mod icons;
pub mod layout;
pub mod markdown;
pub mod ocr_scene;
#[cfg(windows)]
pub mod painter_d3d11;
#[cfg(all(linux_vr, target_arch = "x86_64"))]
pub mod painter_vulkan;
pub mod text;
pub mod theme;
pub mod views;

use std::collections::HashMap;
use std::time::Instant;

use self::draw::DrawList;
use self::text::TextEngine;
use crate::vr_overlay::surface_protocol::{
    ReaderIdentity, SurfaceId, SurfaceKey, SurfaceLayout, SurfaceState, SurfaceView,
    reader_identity,
};

#[derive(Debug, Default, Clone)]
pub struct SurfaceRuntimeState {
    pub scroll_y: f32,
    pub scroll_height: f32,
    pub client_height: f32,
    pub dialogue_following: bool,
    pub reader_needs_page_sync: bool,
    pub last_scroll_key: Option<SurfaceKey>,
    pub saved_reader_scroll: HashMap<u64, f32>,
    pub anim_start: Option<Instant>,
}

pub struct GpuUiEngine {
    pub text: TextEngine,
}

impl Default for GpuUiEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuUiEngine {
    pub fn new() -> Self {
        Self {
            text: TextEngine::new(),
        }
    }

    pub fn render_surface(
        &mut self,
        state: &SurfaceState,
        runtime: &mut SurfaceRuntimeState,
        now: Instant,
    ) -> (SurfaceLayout, DrawList) {
        if let SurfaceView::Dashboard { dashboard, .. } = &state.view {
            self.text.set_locale(&dashboard.locale);
        }
        if runtime.last_scroll_key != Some(state.scroll_key) {
            runtime.last_scroll_key = Some(state.scroll_key);
            runtime.dialogue_following = true;
            if let Some(id) = reader_identity(&state.view) {
                let hash = hash_reader_identity(&id);
                if let Some(&saved) = runtime.saved_reader_scroll.get(&hash) {
                    runtime.scroll_y = saved;
                    runtime.reader_needs_page_sync = false;
                } else {
                    runtime.scroll_y = 0.0;
                    runtime.reader_needs_page_sync = true;
                }
            } else {
                runtime.scroll_y = 0.0;
                runtime.reader_needs_page_sync = false;
            }
        }
        views::render_surface(&mut self.text, state, runtime, now)
    }

    pub fn apply_scroll(
        surface: SurfaceId,
        delta: f32,
        view: Option<&SurfaceView>,
        runtime: &mut SurfaceRuntimeState,
    ) -> bool {
        if !delta.is_finite() || delta == 0.0 {
            return false;
        }
        let client_h = runtime.client_height;
        let max_scroll = (runtime.scroll_height - client_h).max(0.0);
        if client_h <= 0.0 || max_scroll <= 0.0 {
            return false;
        }
        let next = (runtime.scroll_y + delta * client_h).clamp(0.0, max_scroll);
        if (next - runtime.scroll_y).abs() > 0.01 {
            runtime.scroll_y = next;
            if surface == SurfaceId::Dialogue {
                runtime.dialogue_following = delta >= 0.0 && next >= max_scroll - 2.0;
            } else if surface == SurfaceId::Reader {
                runtime.reader_needs_page_sync = false;
                if let Some(view) = view {
                    Self::save_reader_scroll(view, runtime);
                }
            }
            return true;
        }
        false
    }

    pub fn save_reader_scroll(view: &SurfaceView, runtime: &mut SurfaceRuntimeState) {
        if let Some(id) = reader_identity(view) {
            let hash = hash_reader_identity(&id);
            runtime.saved_reader_scroll.insert(hash, runtime.scroll_y);
        }
    }
}

fn hash_reader_identity(id: &ReaderIdentity) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}

#[cfg(all(test, any(windows, linux_vr)))]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use super::*;
    use crate::vr_overlay::caption_status::CaptionLabels;
    use crate::vr_overlay::control_protocol::ControlSnapshot;
    use crate::vr_overlay::dashboard::{CaptionVisibility, DashboardUi, DashboardViewModel};
    use crate::vr_overlay::placement::Mode;
    use crate::vr_overlay::player_audio_ui::{Prompt, Section};
    use crate::vr_overlay::presentation::{CaptionOriginal, MessageSide, WristMessage};
    use crate::vr_overlay::surface_protocol::{SurfaceId, SurfaceKey, SurfaceState};

    fn sample_state(view: SurfaceView) -> SurfaceState {
        let key = SurfaceKey {
            generation: 1,
            revision: 1,
        };
        SurfaceState {
            key,
            scroll_key: key,
            view,
            hovered: None,
            pressed: None,
            pointer: None,
        }
    }

    fn sample_messages() -> Vec<WristMessage> {
        vec![
            WristMessage {
                speaker: Some("Alice".into()),
                text: "Hello VRCS!".into(),
                side: MessageSide::Left,
                original: Some(CaptionOriginal {
                    source: "whisper".into(),
                    track_id: Some("p1".into()),
                    text: "こんにちは".into(),
                    language: Some("ja".into()),
                    detected_language: Some("ja".into()),
                    translation_sources: vec![],
                    live: false,
                }),
            },
            WristMessage {
                speaker: Some("Me".into()),
                text: "Testing Rust GPU overlay rendering.\nRust GPU 渲染测试。".into(),
                side: MessageSide::Right,
                original: Some(CaptionOriginal {
                    source: "self".into(),
                    track_id: Some("self".into()),
                    text: "Testing Rust GPU overlay rendering.".into(),
                    language: Some("en".into()),
                    detected_language: Some("en".into()),
                    translation_sources: vec![],
                    live: true,
                }),
            },
        ]
    }

    fn sample_views() -> Vec<(SurfaceId, SurfaceView)> {
        let dashboard: DashboardViewModel =
            serde_json::from_str(include_str!("../preview-fixture.json")).expect("valid fixture");
        let dark = false;
        let mut views = vec![
            (
                SurfaceId::Menu,
                SurfaceView::Menu {
                    players: Default::default(),
                    ui: Default::default(),
                    controls: ControlSnapshot {
                        dark,
                        ..Default::default()
                    },
                    hold_progress: Some(0.65),
                },
            ),
            (
                SurfaceId::Dialogue,
                SurfaceView::Dialogue {
                    visibility: CaptionVisibility::default(),
                    max_visible_players: None,
                    max_entries: 12,
                    controls: ControlSnapshot {
                        dark,
                        ..Default::default()
                    },
                    messages: sample_messages(),
                    notice: Some("Synced".into()),
                    notice_error: false,
                    labels: CaptionLabels::default(),
                    mode: Mode::Wrist,
                    follow_view: true,
                    busy: false,
                    error: None,
                    font_size: 26,
                    opacity: 0.94,
                },
            ),
            (
                SurfaceId::Headset,
                SurfaceView::Headset {
                    visibility: CaptionVisibility::default(),
                    max_visible_players: None,
                    dark,
                    messages: sample_messages(),
                    notice: None,
                    notice_error: false,
                    labels: CaptionLabels::default(),
                    lines_per_language: 2,
                    font_size: 28,
                    opacity: 0.92,
                },
            ),
            (
                SurfaceId::Dashboard,
                SurfaceView::Dashboard {
                    dark,
                    dashboard: Arc::new(dashboard),
                    ui: DashboardUi::default(),
                },
            ),
            (
                SurfaceId::PlayerAudio,
                SurfaceView::PlayerAudio {
                    dark,
                    notice: Prompt {
                        request_id: 1,
                        title: "Player audio consent".into(),
                        intro: "Review audio consent terms.".into(),
                        sections: vec![Section {
                            title: "Scope".into(),
                            paragraphs: vec!["Only selected players are processed.".into()],
                        }],
                        remaining_seconds: 0,
                        reading_status: "Ready".into(),
                        confirm: "Confirm".into(),
                        cancel: "Cancel".into(),
                        previous: "Prev".into(),
                        next: "Next".into(),
                        scroll_hint: "Scroll".into(),
                    },
                },
            ),
        ];
        #[cfg(windows)]
        {
            use crate::vr_overlay::ocr_plane::{OcrScene, OcrSceneBlock};
            use crate::vr_overlay::ocr_progress::{ProgressTask, ProgressView, Stage};
            use crate::vr_overlay::ocr_wrist::{View as ReaderView, ViewMode};
            use crate::vr_overlay::wrist_labels::WristLabels;

            views.push((
                SurfaceId::Reader,
                SurfaceView::Reader {
                    dark,
                    labels: WristLabels::default(),
                    reader: ReaderView {
                        mode: ViewMode::Vision,
                        source: "What does this menu say?".into(),
                        translation: "### Settings\n\n- **Audio**: `Enabled`\n\n| Key | Value |\n| --- | --- |\n| Mode | Live |".into(),
                        translation_label: "Answer".into(),
                        title: "Vision".into(),
                        status: "complete".into(),
                        detail: String::new(),
                        block_index: 0,
                        block_count: 2,
                        page: 0,
                        pinned: false,
                        source_only: false,
                        language_count: 1,
                        hovered: None,
                    },
                    font_size: 26,
                    opacity: 0.94,
                },
            ));
            views.push((
                SurfaceId::OcrProgress,
                SurfaceView::OcrProgress {
                    dark,
                    progress: Some(ProgressView {
                        task: ProgressTask::Ocr,
                        scan_id: 1,
                        status: "Translating".into(),
                        detail: "2 / 4 blocks".into(),
                        elapsed_seconds: 1,
                        stages: [
                            Stage::Complete,
                            Stage::Complete,
                            Stage::Active,
                            Stage::Pending,
                        ],
                        stage_labels: [
                            "Capture".into(),
                            "Recognize".into(),
                            "Translate".into(),
                            "Display".into(),
                        ],
                        translation_fraction: Some((2, 4)),
                    }),
                },
            ));
            views.push((SurfaceId::OcrSelection, SurfaceView::OcrSelection { dark }));
            views.push((
                SurfaceId::OcrResult,
                SurfaceView::OcrResult {
                    dark,
                    scene: Arc::new(OcrScene {
                        aspect: 1.6,
                        opacity: 0.94,
                        blocks: vec![OcrSceneBlock {
                            id: 0,
                            text: "Open Settings · 打开设置".into(),
                            quad: [[0.12, 0.18], [0.52, 0.20], [0.50, 0.34], [0.11, 0.32]],
                            masks: vec![[[0.12, 0.18], [0.52, 0.20], [0.50, 0.34], [0.11, 0.32]]],
                        }],
                        excluded: vec![],
                    }),
                },
            ));
        }
        views
    }

    #[test]
    fn caption_filters_and_slices_headset_and_dialogue() {
        let msgs = sample_messages();
        let vis = CaptionVisibility {
            hidden_tracks: vec!["p1".into()],
            hidden_source_languages: vec![],
        };
        let out = caption::visible_caption_messages(true, &msgs, &vis, None, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].speaker.as_deref(), Some("Me"));
    }

    #[test]
    fn markdown_parses_blocks_and_inline_styles() {
        let blocks = markdown::parse_markdown(
            "# Title\n\n- [x] Done with **bold** and `code`\n\n| A | B |\n| - | - |\n| 1 | 2 |",
        );
        assert!(blocks.len() >= 3);
    }

    #[cfg(windows)]
    #[test]
    fn ocr_scene_places_cards_and_valid_homographies() {
        use crate::vr_overlay::ocr_plane::{OcrScene, OcrSceneBlock};

        let mut engine = TextEngine::new();
        let scene = OcrScene {
            aspect: 1.6,
            opacity: 0.94,
            blocks: vec![
                OcrSceneBlock {
                    id: 0,
                    text: "Open Settings".into(),
                    quad: [[0.10, 0.15], [0.45, 0.16], [0.44, 0.28], [0.10, 0.27]],
                    masks: vec![[[0.10, 0.15], [0.45, 0.16], [0.44, 0.28], [0.10, 0.27]]],
                },
                OcrSceneBlock {
                    id: 1,
                    text: "Short label".into(),
                    quad: [[0.55, 0.55], [0.72, 0.55], [0.72, 0.62], [0.55, 0.62]],
                    masks: vec![[[0.55, 0.55], [0.72, 0.55], [0.72, 0.62], [0.55, 0.62]]],
                },
            ],
            excluded: vec![],
        };
        let laid =
            ocr_scene::layout_ocr_scene(&scene, &mut |s, sz| engine.measure_line_width(s, sz, 600));
        assert_eq!(laid.cards.len(), 2);
        for card in &laid.cards {
            assert!(ocr_scene::quad_homography(&card.quad, card.width, card.height).is_some());
        }
    }

    #[test]
    fn all_surfaces_render_valid_layouts() {
        let mut engine = GpuUiEngine::new();
        let now = Instant::now();
        for (surface, view) in sample_views() {
            let mut rt = SurfaceRuntimeState::default();
            let (layout, draw) = engine.render_surface(&sample_state(view), &mut rt, now);
            layout
                .validate(surface)
                .unwrap_or_else(|e| panic!("{} failed validation: {e}", surface.name()));
            let has_geometry = !draw.instances.is_empty()
                || !draw.mask_vertices.is_empty()
                || !draw.ocr_cards.is_empty();
            assert!(has_geometry, "{} emitted no geometry", surface.name());
        }
    }

    #[cfg(windows)]
    #[test]
    fn d3d11_renders_visible_pixels_for_surfaces() {
        use super::painter_d3d11::{D3d11Painter, D3d11SurfaceTarget};

        let Ok(mut painter) = D3d11Painter::new_for_test() else {
            return;
        };
        let mut engine = GpuUiEngine::new();
        let now = Instant::now();
        let dump_dir = std::env::var_os("VRCS_GPU_UI_DUMP_DIR").map(std::path::PathBuf::from);
        if let Some(dir) = &dump_dir {
            let _ = std::fs::create_dir_all(dir);
        }

        for (surface, view) in sample_views() {
            let mut rt = SurfaceRuntimeState::default();
            let (_layout, draw) = engine.render_surface(&sample_state(view), &mut rt, now);
            let mut target = D3d11SurfaceTarget::new(draw.width, draw.height);
            let (handle, bounds) = painter
                .render(&mut target, &engine.text.atlas, &draw)
                .unwrap_or_else(|e| panic!("D3D11 render {} failed: {e}", surface.name()));
            let rgba = painter
                .readback_rgba(&target, handle, bounds)
                .unwrap_or_else(|e| panic!("D3D11 readback {} failed: {e}", surface.name()));
            assert!(
                rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0),
                "{} produced completely transparent output",
                surface.name()
            );
            if let Some(dir) = &dump_dir {
                let rw = bounds[2] - bounds[0];
                let rh = bounds[3] - bounds[1];
                let file = std::fs::File::create(dir.join(format!("{}.png", surface.name())))
                    .expect("create png");
                let mut enc = png::Encoder::new(file, rw, rh);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let mut writer = enc.write_header().expect("png header");
                writer.write_image_data(&rgba).expect("png write");
            }
        }
    }

    #[test]
    fn hover_change_hits_shaping_cache_without_new_atlas_writes() {
        let mut engine = GpuUiEngine::new();
        let dashboard: DashboardViewModel =
            serde_json::from_str(include_str!("../preview-fixture.json")).expect("valid fixture");
        let mut state = sample_state(SurfaceView::Dashboard {
            dark: false,
            dashboard: Arc::new(dashboard),
            ui: DashboardUi::default(),
        });
        let mut rt = SurfaceRuntimeState::default();
        let now = Instant::now();
        let _ = engine.render_surface(&state, &mut rt, now);
        let r8_dirty_after_warmup = engine.text.atlas.r8_dirty.len();
        let rgba_dirty_after_warmup = engine.text.atlas.rgba_dirty.len();

        state.hovered = Some(2);
        let (layout, draw) = engine.render_surface(&state, &mut rt, now);
        layout.validate(SurfaceId::Dashboard).unwrap();
        assert!(!draw.instances.is_empty());
        assert_eq!(engine.text.atlas.r8_dirty.len(), r8_dirty_after_warmup);
        assert_eq!(engine.text.atlas.rgba_dirty.len(), rgba_dirty_after_warmup);
    }

    #[test]
    fn menu_hover_tooltip_glyphs_stay_centered_inside_tooltip_box() {
        use super::draw::InstanceKind;

        let mut engine = GpuUiEngine::new();
        let now = Instant::now();
        let mut rt = SurfaceRuntimeState::default();

        for (hovered_id, tip_label) in [
            (0u8, "字幕"),
            (3u8, "Capture speech and translate it immediately"),
        ] {
            let mut controls = ControlSnapshot::default();
            controls.items[0].available = true;
            controls.items[0].label = tip_label.into();
            controls.items[3].available = true;
            controls.items[3].label = tip_label.into();
            let mut state = sample_state(SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
                controls,
                hold_progress: None,
            });
            state.hovered = Some(hovered_id);

            let (layout, draw) = engine.render_surface(&state, &mut rt, now);
            layout.validate(SurfaceId::Menu).unwrap();

            // Tooltips are emitted last: the final Rect is the tooltip background, followed by its glyphs.
            let tip_idx = draw
                .instances
                .iter()
                .rposition(|inst| inst.kind == InstanceKind::Rect as u32)
                .expect("tooltip background rect");
            let tip_rect = draw.instances[tip_idx].rect;
            assert!(
                tip_rect[2] <= 230.5,
                "menu tooltip exceeds max_width: {:?}",
                tip_rect
            );

            let glyphs = &draw.instances[tip_idx + 1..];
            assert!(!glyphs.is_empty(), "tooltip emitted no glyph instances");

            let mut min_x = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            for g in glyphs {
                assert_eq!(g.kind, InstanceKind::MaskR8 as u32);
                assert!(
                    g.rect[0] >= tip_rect[0] + 10.0
                        && g.rect[0] + g.rect[2] <= tip_rect[0] + tip_rect[2] - 10.0,
                    "tooltip glyph {:?} overflowed rect {:?}",
                    g.rect,
                    tip_rect
                );
                assert!(
                    g.rect[1] >= tip_rect[1] && g.rect[1] + g.rect[3] <= tip_rect[1] + tip_rect[3],
                    "tooltip glyph {:?} vertically outside rect {:?}",
                    g.rect,
                    tip_rect
                );
                min_x = min_x.min(g.rect[0]);
                max_x = max_x.max(g.rect[0] + g.rect[2]);
            }
            let rect_cx = tip_rect[0] + tip_rect[2] * 0.5;
            let text_cx = (min_x + max_x) * 0.5;
            assert!(
                (text_cx - rect_cx).abs() <= 4.0,
                "tooltip text center {text_cx} shifted vs rect center {rect_cx} (rect={tip_rect:?})"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn reader_right_aligned_header_stays_within_surface_bounds() {
        use super::draw::InstanceKind;
        use crate::vr_overlay::ocr_wrist::{View as ReaderView, ViewMode};
        use crate::vr_overlay::wrist_labels::WristLabels;

        let mut engine = GpuUiEngine::new();
        let mut rt = SurfaceRuntimeState::default();
        let now = Instant::now();
        let state = sample_state(SurfaceView::Reader {
            dark: false,
            labels: WristLabels::default(),
            reader: ReaderView {
                mode: ViewMode::Ocr,
                source: "Source".into(),
                translation: "Translated".into(),
                translation_label: "Translation".into(),
                title: "Reader".into(),
                status: "complete".into(),
                detail: "Done".into(),
                block_index: 0,
                block_count: 3,
                page: 0,
                pinned: false,
                source_only: false,
                language_count: 1,
                hovered: None,
            },
            font_size: 26,
            opacity: 0.94,
        });
        let (layout, draw) = engine.render_surface(&state, &mut rt, now);
        layout.validate(SurfaceId::Reader).unwrap();
        let mask_r8 = InstanceKind::MaskR8 as u32;
        for inst in &draw.instances {
            if inst.kind == mask_r8 {
                assert!(
                    inst.rect[0] >= 0.0 && inst.rect[0] + inst.rect[2] <= draw.width as f32,
                    "glyph out of reader surface bounds: {:?} (w={})",
                    inst.rect,
                    draw.width
                );
            }
        }
    }

    #[test]
    fn thumbstick_scroll_scales_by_viewport_and_does_not_snap_back() {
        let mut engine = GpuUiEngine::new();
        let now = Instant::now();

        // 1. Dialogue: auto-follows bottom initially, then gentle upward scroll unlocks following.
        let mut rt = SurfaceRuntimeState::default();
        let long_messages: Vec<WristMessage> = (0..10)
            .map(|i| WristMessage {
                speaker: Some(format!("Speaker {i}")),
                text: "Overflowing VRCS dialogue subtitle line to make the viewport scrollable.\n第二行测试文本。".into(),
                side: if i % 2 == 0 {
                    MessageSide::Left
                } else {
                    MessageSide::Right
                },
                original: None,
            })
            .collect();
        let view = SurfaceView::Dialogue {
            visibility: CaptionVisibility::default(),
            max_visible_players: None,
            max_entries: 12,
            controls: ControlSnapshot::default(),
            messages: long_messages,
            notice: None,
            notice_error: false,
            labels: CaptionLabels::default(),
            mode: Mode::Wrist,
            follow_view: true,
            busy: false,
            error: None,
            font_size: 26,
            opacity: 0.94,
        };
        let state = sample_state(view.clone());
        let (_, draw_bottom) = engine.render_surface(&state, &mut rt, now);
        assert!(
            rt.scroll_height > rt.client_height,
            "dialogue did not overflow"
        );
        let max_scroll = rt.scroll_height - rt.client_height;
        assert!((rt.scroll_y - max_scroll).abs() < 0.5);
        assert!(rt.dialogue_following);

        // Even a gentle <2px upward delta must scroll and disengage auto-follow.
        assert!(GpuUiEngine::apply_scroll(
            SurfaceId::Dialogue,
            -0.001,
            Some(&view),
            &mut rt
        ));
        assert!(!rt.dialogue_following);
        let expected_after_gentle = (max_scroll - 0.001 * rt.client_height).max(0.0);
        assert!((rt.scroll_y - expected_after_gentle).abs() < 0.1);

        // Typical 90Hz thumbstick upward tick (~1.5 viewport heights/sec * 1/90s ≈ -0.016).
        assert!(GpuUiEngine::apply_scroll(
            SurfaceId::Dialogue,
            -0.016,
            Some(&view),
            &mut rt
        ));
        let scrolled_y = rt.scroll_y;
        assert!(scrolled_y < max_scroll - 5.0);
        let (_, draw_scrolled) = engine.render_surface(&state, &mut rt, now);
        assert!(
            (rt.scroll_y - scrolled_y).abs() < 0.1,
            "dialogue snapped back on re-render: {} vs {}",
            rt.scroll_y,
            scrolled_y
        );
        assert_ne!(
            draw_bottom
                .instances
                .iter()
                .map(|i| i.rect[1].to_bits())
                .collect::<Vec<_>>(),
            draw_scrolled
                .instances
                .iter()
                .map(|i| i.rect[1].to_bits())
                .collect::<Vec<_>>()
        );

        // Scrolling down to bottom re-engages auto-follow.
        assert!(GpuUiEngine::apply_scroll(
            SurfaceId::Dialogue,
            1.0,
            Some(&view),
            &mut rt
        ));
        assert!((rt.scroll_y - max_scroll).abs() < 0.5);
        assert!(rt.dialogue_following);

        // 2. PlayerAudio: Prev/Next & thumbstick scale by client_height.
        let mut pa_rt = SurfaceRuntimeState::default();
        let pa_state = sample_state(SurfaceView::PlayerAudio {
            dark: false,
            notice: Prompt {
                request_id: 1,
                title: "Audio consent".into(),
                intro: "Intro paragraph.".into(),
                sections: (0..8)
                    .map(|i| Section {
                        title: format!("Section {i}"),
                        paragraphs: vec![
                            "Long explanation paragraph that fills the consent viewport vertically so scrolling is required.".into(),
                            "Second paragraph in section.".into(),
                        ],
                    })
                    .collect(),
                remaining_seconds: 0,
                reading_status: "Ready".into(),
                confirm: "Confirm".into(),
                cancel: "Cancel".into(),
                previous: "Prev".into(),
                next: "Next".into(),
                scroll_hint: "Scroll".into(),
            },
        });
        let _ = engine.render_surface(&pa_state, &mut pa_rt, now);
        assert!(
            pa_rt.scroll_height > pa_rt.client_height,
            "player audio did not overflow"
        );
        assert!(GpuUiEngine::apply_scroll(
            SurfaceId::PlayerAudio,
            0.25,
            Some(&pa_state.view),
            &mut pa_rt,
        ));
        let expected_pa =
            (0.25 * pa_rt.client_height).min(pa_rt.scroll_height - pa_rt.client_height);
        assert!((pa_rt.scroll_y - expected_pa).abs() < 0.5);

        // 3. Reader on page 1: initial page sync applies geom.offset, scrolling back to 0 stays at 0.
        #[cfg(windows)]
        {
            use crate::vr_overlay::ocr_wrist::{View as ReaderView, ViewMode};
            use crate::vr_overlay::wrist_labels::WristLabels;

            let mut r_rt = SurfaceRuntimeState::default();
            let long_translation = (0..30)
                .map(|i| format!("Line {i}: Translated OCR text block for scrolling check."))
                .collect::<Vec<_>>()
                .join("\n");
            let r_state = sample_state(SurfaceView::Reader {
                dark: false,
                labels: WristLabels::default(),
                reader: ReaderView {
                    mode: ViewMode::Ocr,
                    source: "Source text".into(),
                    translation: long_translation,
                    translation_label: "Translation".into(),
                    title: "Reader".into(),
                    status: "complete".into(),
                    detail: String::new(),
                    block_index: 0,
                    block_count: 1,
                    page: 1,
                    pinned: false,
                    source_only: false,
                    language_count: 1,
                    hovered: None,
                },
                font_size: 28,
                opacity: 0.94,
            });
            let _ = engine.render_surface(&r_state, &mut r_rt, now);
            assert!(
                r_rt.scroll_height > r_rt.client_height,
                "reader did not overflow"
            );
            assert!(
                r_rt.scroll_y > 10.0,
                "expected initial page 1 offset > 10px, got {}",
                r_rt.scroll_y
            );

            // Scroll all the way back to top via thumbstick and re-render.
            assert!(GpuUiEngine::apply_scroll(
                SurfaceId::Reader,
                -2.0,
                Some(&r_state.view),
                &mut r_rt,
            ));
            assert!((r_rt.scroll_y - 0.0).abs() < 0.01);
            let _ = engine.render_surface(&r_state, &mut r_rt, now);
            assert!(
                r_rt.scroll_y <= 0.5,
                "reader snapped back to page offset after manual scroll to top: {}",
                r_rt.scroll_y
            );
        }
    }

    #[test]
    fn language_preset_buttons_have_gaps() {
        use crate::vr_overlay::dashboard::{
            DashboardAction, DashboardCommand, DashboardPage, DashboardPicker,
        };

        let mut engine = GpuUiEngine::new();
        let mut rt = SurfaceRuntimeState::default();
        let dashboard: DashboardViewModel =
            serde_json::from_str(include_str!("../preview-fixture.json")).expect("valid fixture");
        let state = sample_state(SurfaceView::Dashboard {
            dark: false,
            dashboard: Arc::new(dashboard),
            ui: DashboardUi {
                page: DashboardPage::Language,
                ..Default::default()
            },
        });
        let (layout, _) = engine.render_surface(&state, &mut rt, Instant::now());
        layout.validate(SurfaceId::Dashboard).unwrap();

        let w = SurfaceId::Dashboard.size().0 as f32;
        let find_x = |pred: &dyn Fn(&DashboardCommand) -> bool| -> [f32; 2] {
            let r = layout
                .regions
                .iter()
                .find(|r| r.dashboard.as_ref().is_some_and(pred))
                .expect("preset button region missing");
            [r.bounds[0] * w, r.bounds[2] * w]
        };
        let apply =
            find_x(&|c| matches!(c, DashboardCommand::Picker(DashboardPicker::ApplyPreset)));
        let save = find_x(&|c| {
            matches!(
                c,
                DashboardCommand::Action(DashboardAction::SaveLanguagePreset(_))
            )
        });
        let delete =
            find_x(&|c| matches!(c, DashboardCommand::Picker(DashboardPicker::DeletePreset)));

        let gap_ab = save[0] - apply[1];
        let gap_bc = delete[0] - save[1];
        assert!(
            (gap_ab - 12.0).abs() <= 1.5,
            "expected ~12px gap between apply and save, got {gap_ab} ({apply:?} -> {save:?})"
        );
        assert!(
            (gap_bc - 12.0).abs() <= 1.5,
            "expected ~12px gap between save and delete, got {gap_bc} ({save:?} -> {delete:?})"
        );
    }

    #[test]
    fn dashboard_display_includes_wrist_menu_toggle_within_bounds() {
        use crate::vr_overlay::dashboard::{DashboardAction, DashboardCommand, DashboardPage};

        let mut engine = GpuUiEngine::new();
        let mut rt = SurfaceRuntimeState::default();
        let base: DashboardViewModel =
            serde_json::from_str(include_str!("../preview-fixture.json")).expect("valid fixture");

        for master_enabled in [true, false] {
            let mut dashboard = base.clone();
            dashboard.enabled = master_enabled;
            let state = sample_state(SurfaceView::Dashboard {
                dark: false,
                dashboard: Arc::new(dashboard),
                ui: DashboardUi {
                    page: DashboardPage::Display,
                    ..Default::default()
                },
            });
            let (layout, _) = engine.render_surface(&state, &mut rt, Instant::now());
            layout.validate(SurfaceId::Dashboard).unwrap();

            assert!(
                layout.regions.iter().any(|r| matches!(
                    r.dashboard,
                    Some(DashboardCommand::Action(DashboardAction::ToggleWristMenu))
                )),
                "missing ToggleWristMenu region when master_enabled={master_enabled}"
            );
            for region in &layout.regions {
                assert!(
                    region.bounds[1] >= 0.0 && region.bounds[3] <= 1.0,
                    "region vertically out of bounds when master_enabled={master_enabled}: {:?}",
                    region.bounds
                );
            }
        }
    }
}
