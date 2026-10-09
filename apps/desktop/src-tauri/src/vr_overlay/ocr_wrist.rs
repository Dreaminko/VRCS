use vrcs_core::ocr::TranslatedBlock;

use super::ocr_status::OcrState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    PreviousBlock,
    NextBlock,
    PreviousPage,
    NextPage,
    NextLanguage,
    TogglePin,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct View {
    pub source: String,
    pub translation: String,
    pub translation_label: String,
    pub title: String,
    pub status: String,
    pub block_index: usize,
    pub block_count: usize,
    pub page: usize,
    pub pinned: bool,
    pub source_only: bool,
    pub language_count: usize,
    pub hovered: Option<Action>,
}

#[derive(Default)]
pub struct Reader {
    scan_id: u64,
    blocks: Vec<TranslatedBlock>,
    current_id: Option<usize>,
    page: usize,
    language: usize,
    pub pinned: bool,
}

impl Reader {
    pub fn sync(&mut self, scan_id: u64, blocks: &[TranslatedBlock]) {
        if self.scan_id != scan_id {
            *self = Self {
                scan_id,
                ..Self::default()
            };
        }
        self.blocks = blocks
            .iter()
            .filter(|block| !block.source.text.trim().is_empty())
            .cloned()
            .collect();
        self.blocks.sort_by_key(|block| block.source.id);
        if !self
            .blocks
            .iter()
            .any(|block| Some(block.source.id) == self.current_id)
        {
            self.current_id = self.blocks.first().map(|block| block.source.id);
            self.page = 0;
        }
    }

    fn index(&self) -> usize {
        self.blocks
            .iter()
            .position(|block| Some(block.source.id) == self.current_id)
            .unwrap_or(0)
    }

    pub fn view(&self, state: OcrState, source_only: bool, fallback: bool) -> View {
        let block = self.blocks.get(self.index());
        let translation = block.and_then(|block| {
            block
                .translations
                .get(self.language % block.translations.len().max(1))
        });
        View {
            source: block
                .map(|block| block.source.text.trim().to_owned())
                .unwrap_or_default(),
            translation: if source_only || block.is_none() {
                String::new()
            } else {
                translation
                    .and_then(|item| item.text.as_deref())
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| text.trim().to_owned())
                    .unwrap_or_else(|| {
                        let error = translation.and_then(|item| item.error_code.as_deref());
                        if error == Some("translation.not_configured") {
                            "Translation is not configured"
                        } else if state == OcrState::Translating || state == OcrState::Recognized {
                            "Translating…"
                        } else if state == OcrState::TimedOut
                            || error == Some("translation.timeout")
                        {
                            "Translation timed out"
                        } else {
                            "Translation unavailable"
                        }
                        .into()
                    })
            },
            translation_label: translation
                .map(|item| item.target_language.clone())
                .unwrap_or_default(),
            title: if fallback {
                "OCR · Additional text"
            } else {
                "OCR"
            }
            .into(),
            status: state_label(state).into(),
            block_index: self.index(),
            block_count: self.blocks.len(),
            page: self.page,
            pinned: self.pinned,
            source_only,
            language_count: block.map(|block| block.translations.len()).unwrap_or(0),
            hovered: None,
        }
    }

    pub fn act(&mut self, action: Action, page_count: usize) {
        let index = self.index();
        match action {
            Action::PreviousBlock | Action::NextBlock => {
                let next = if action == Action::PreviousBlock {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(self.blocks.len().saturating_sub(1))
                };
                self.current_id = self.blocks.get(next).map(|block| block.source.id);
                if next != index {
                    self.page = 0;
                }
            }
            Action::PreviousPage => self.page = self.page.saturating_sub(1),
            Action::NextPage => self.page = (self.page + 1).min(page_count.saturating_sub(1)),
            Action::NextLanguage => {
                let count = self
                    .blocks
                    .get(index)
                    .map(|block| block.translations.len())
                    .unwrap_or(0);
                self.language = (self.language + 1) % count.max(1);
                self.page = 0;
            }
            Action::TogglePin => self.pinned = !self.pinned,
            Action::Close => {}
        }
    }

    pub fn clamp_page(&mut self, page_count: usize) {
        self.page = self.page.min(page_count.saturating_sub(1));
    }
}

pub fn state_label(state: OcrState) -> &'static str {
    match state {
        OcrState::Capturing => "Capturing…",
        OcrState::Submitting => "Uploading image…",
        OcrState::Pending => "Waiting for cloud OCR…",
        OcrState::Downloading => "Fetching OCR results…",
        OcrState::LoadingModel => "Loading local models…",
        OcrState::Running | OcrState::Recognizing => "Recognizing…",
        OcrState::Recognized | OcrState::Translating => "Translating…",
        OcrState::NoText => "No text detected",
        OcrState::LowConfidence => "No readable text detected",
        OcrState::PartialVisible => "Some translations are unavailable",
        OcrState::TimedOut => "Processing timed out",
        OcrState::TranslationFailed => "Translation failed",
        OcrState::Error => "OCR unavailable",
        OcrState::SourceVisible => "Original text",
        _ => "Ready",
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LaserEvent {
    Move { device: u32, point: [f32; 2] },
    Down { device: u32, point: [f32; 2] },
    Up { device: u32, point: [f32; 2] },
    Leave,
}

impl LaserEvent {
    pub fn point(self) -> Option<[f32; 2]> {
        match self {
            Self::Move { point, .. } | Self::Down { point, .. } | Self::Up { point, .. } => {
                Some(point)
            }
            Self::Leave => None,
        }
    }
}

pub fn laser_event(event: &openvr_sys::VREvent_t, wrist_device: u32) -> Option<LaserEvent> {
    let kind = event.eventType as i32;
    if matches!(
        kind,
        openvr_sys::EVREventType_VREvent_FocusLeave
            | openvr_sys::EVREventType_VREvent_OverlayHidden
            | openvr_sys::EVREventType_VREvent_OverlayShown
    ) {
        return Some(LaserEvent::Leave);
    }
    if !matches!(
        kind,
        openvr_sys::EVREventType_VREvent_MouseMove
            | openvr_sys::EVREventType_VREvent_MouseButtonDown
            | openvr_sys::EVREventType_VREvent_MouseButtonUp
    ) {
        return None;
    }
    let mouse = unsafe { event.data.mouse };
    if kind != openvr_sys::EVREventType_VREvent_MouseMove
        && mouse.button & openvr_sys::EVRMouseButton_VRMouseButton_Left as u32 == 0
    {
        return None;
    }
    let device = event.trackedDeviceIndex;
    let size = super::ocr_wrist_renderer::SIZE as f32;
    // SteamVR mouse coordinates start at the bottom left; button bounds start at the top left.
    let point = [mouse.x / size, 1. - mouse.y / size];
    if device == wrist_device
        || point
            .iter()
            .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
    {
        return Some(LaserEvent::Leave);
    }
    Some(match kind {
        openvr_sys::EVREventType_VREvent_MouseButtonDown => LaserEvent::Down { device, point },
        openvr_sys::EVREventType_VREvent_MouseButtonUp => LaserEvent::Up { device, point },
        _ => LaserEvent::Move { device, point },
    })
}

pub struct Pointer {
    pressed: bool,
    captured: Option<Action>,
    device: Option<u32>,
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            pressed: true,
            captured: None,
            device: None,
        }
    }
}

impl Pointer {
    pub fn event(&mut self, event: LaserEvent, hit: Option<Action>) -> Option<Action> {
        let device = match event {
            LaserEvent::Move { device, .. } | LaserEvent::Down { device, .. } => device,
            LaserEvent::Up { device, .. } if self.device == Some(device) => {
                return self.update(hit, false);
            }
            LaserEvent::Up { .. } | LaserEvent::Leave => {
                *self = Self::default();
                return None;
            }
        };
        if self.device != Some(device) {
            *self = Self {
                pressed: false,
                captured: None,
                device: Some(device),
            };
        }
        if matches!(event, LaserEvent::Down { .. }) {
            self.update(hit, true)
        } else {
            None
        }
    }

    fn update(&mut self, hit: Option<Action>, pressed: bool) -> Option<Action> {
        let action = if pressed && !self.pressed {
            self.captured = hit;
            None
        } else if !pressed && self.pressed {
            self.captured.take().filter(|action| Some(*action) == hit)
        } else {
            None
        };
        self.pressed = pressed;
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrcs_core::ocr::{BlockTranslation, TextBlock};

    fn block(id: usize, y: f32, original: &str, translated: Option<&str>) -> TranslatedBlock {
        TranslatedBlock {
            fragments: vec![],
            source: TextBlock {
                id,
                text: original.into(),
                confidence: 1.,
                polygon: [[0., y], [10., y], [10., y + 1.], [0., y + 1.]],
            },
            translations: vec![BlockTranslation {
                target_language: "zh-Hans".into(),
                text: translated.map(str::to_owned),
                error_code: None,
            }],
        }
    }

    #[test]
    fn empty_results_do_not_report_a_translation_failure() {
        let reader = Reader::default();
        for state in [OcrState::Recognizing, OcrState::NoText, OcrState::Error] {
            let view = reader.view(state, false, false);
            assert_eq!(view.block_count, 0);
            assert!(view.source.is_empty());
            assert!(view.translation.is_empty());
            assert_eq!(view.status, state_label(state));
        }
    }

    #[test]
    fn translated_result_keeps_the_original_in_the_same_view() {
        let mut reader = Reader::default();
        reader.sync(1, &[block(10, 0., "hello", Some("你好"))]);
        let view = reader.view(OcrState::Visible, false, false);
        assert_eq!(view.source, "hello");
        assert_eq!(view.translation, "你好");
        assert_eq!(view.translation_label, "zh-Hans");
    }

    #[test]
    fn updates_preserve_the_current_block_page_and_pin() {
        let mut reader = Reader::default();
        let blocks = [block(10, 0., "first", None), block(20, 10., "second", None)];
        reader.sync(1, &blocks);
        reader.act(Action::NextBlock, 1);
        reader.act(Action::NextPage, 3);
        reader.act(Action::TogglePin, 3);
        reader.sync(
            1,
            &[block(20, 10., "second", Some("第二")), blocks[0].clone()],
        );
        let view = reader.view(OcrState::Visible, false, false);
        assert_eq!(view.source, "second");
        assert_eq!(view.translation, "第二");
        assert_eq!(view.page, 1);
        assert!(view.pinned);
        reader.sync(2, &[block(30, 0., "new", None)]);
        let view = reader.view(OcrState::Translating, false, false);
        assert_eq!(view.source, "new");
        assert_eq!(view.page, 0);
        assert!(!view.pinned);
    }

    #[test]
    fn grouped_reading_order_does_not_interleave_columns() {
        let mut reader = Reader::default();
        reader.sync(
            1,
            &[
                block(2, 0., "right column", None),
                block(1, 40., "left lower", None),
                block(0, 0., "left upper", None),
            ],
        );
        assert_eq!(
            reader.view(OcrState::Translating, false, false).source,
            "left upper"
        );
        reader.act(Action::NextBlock, 1);
        assert_eq!(
            reader.view(OcrState::Translating, false, false).source,
            "left lower"
        );
    }

    #[test]
    fn missing_translations_distinguish_pending_failure_and_source_only() {
        let mut reader = Reader::default();
        let mut item = block(1, 0., "original", None);
        item.translations[0].error_code = Some("translation.timeout".into());
        reader.sync(1, &[item]);
        assert_eq!(
            reader.view(OcrState::Translating, false, false).translation,
            "Translating…"
        );
        assert_eq!(
            reader.view(OcrState::TimedOut, false, false).translation,
            "Translation timed out"
        );
        let view = reader.view(OcrState::SourceVisible, true, false);
        assert!(view.source_only);
        assert!(view.translation.is_empty());
    }

    #[test]
    fn language_switch_keeps_the_original_and_changes_only_the_translation() {
        let mut reader = Reader::default();
        let mut item = block(1, 0., "source", Some("中文"));
        item.translations.push(BlockTranslation {
            target_language: "ja".into(),
            text: Some("日本語".into()),
            error_code: None,
        });
        reader.sync(1, &[item]);
        reader.act(Action::NextLanguage, 1);
        let view = reader.view(OcrState::Visible, false, false);
        assert_eq!(view.source, "source");
        assert_eq!(view.translation, "日本語");
        assert_eq!(view.translation_label, "ja");
    }

    #[test]
    fn navigation_stops_at_the_first_and_last_block_and_page() {
        let mut reader = Reader::default();
        reader.sync(1, &[block(1, 0., "only", None)]);
        reader.act(Action::PreviousBlock, 1);
        reader.act(Action::NextBlock, 1);
        reader.act(Action::PreviousPage, 2);
        for _ in 0..4 {
            reader.act(Action::NextPage, 2);
        }
        let view = reader.view(OcrState::Translating, false, false);
        assert_eq!(view.block_index, 0);
        assert_eq!(view.page, 1);
    }

    #[test]
    fn pointer_click_requires_press_and_release_on_the_same_button() {
        let mut pointer = Pointer::default();
        assert_eq!(pointer.update(None, false), None);
        assert_eq!(pointer.update(Some(Action::NextPage), true), None);
        assert_eq!(pointer.update(Some(Action::NextPage), true), None);
        assert_eq!(
            pointer.update(Some(Action::NextPage), false),
            Some(Action::NextPage)
        );
        assert_eq!(pointer.update(Some(Action::NextPage), false), None);
        pointer.update(Some(Action::Close), true);
        assert_eq!(pointer.update(Some(Action::TogglePin), false), None);
    }

    #[test]
    fn a_trigger_held_before_the_panel_opens_does_not_click_on_release() {
        let mut pointer = Pointer::default();
        pointer.update(Some(Action::Close), true);
        assert_eq!(pointer.update(Some(Action::Close), false), None);
    }

    #[test]
    fn native_laser_click_works_without_a_prior_release_event() {
        let mut pointer = Pointer::default();
        let down = LaserEvent::Down {
            device: 2,
            point: [0.9, 0.9],
        };
        let up = LaserEvent::Up {
            device: 2,
            point: [0.9, 0.9],
        };
        assert_eq!(pointer.event(down, Some(Action::Close)), None);
        assert_eq!(pointer.event(up, Some(Action::Close)), Some(Action::Close));
        assert_eq!(pointer.event(up, Some(Action::Close)), None);
    }

    #[test]
    fn native_laser_cancels_clicks_on_focus_loss_or_device_change() {
        for reset in [
            LaserEvent::Leave,
            LaserEvent::Move {
                device: 3,
                point: [0.9, 0.9],
            },
        ] {
            let mut pointer = Pointer::default();
            pointer.event(
                LaserEvent::Down {
                    device: 2,
                    point: [0.9, 0.9],
                },
                Some(Action::Close),
            );
            pointer.event(reset, Some(Action::Close));
            assert_eq!(
                pointer.event(
                    LaserEvent::Up {
                        device: 2,
                        point: [0.9, 0.9]
                    },
                    Some(Action::Close)
                ),
                None
            );
        }
    }

    #[test]
    fn native_mouse_coordinates_match_rendered_buttons_and_exclude_the_bound_hand() {
        let mouse = |kind: i32, device, button, x, y| {
            let mut event = openvr_sys::VREvent_t {
                eventType: kind as u32,
                trackedDeviceIndex: device,
                ..Default::default()
            };
            event.data.mouse = openvr_sys::VREvent_Mouse_t {
                x,
                y,
                button,
                cursorIndex: 0,
            };
            event
        };
        let down = mouse(
            openvr_sys::EVREventType_VREvent_MouseButtonDown,
            2,
            1,
            900.,
            64.,
        );
        assert_eq!(
            laser_event(&down, 1),
            Some(LaserEvent::Down {
                device: 2,
                point: [900. / 1024., 960. / 1024.]
            })
        );
        assert_eq!(laser_event(&down, 2), Some(LaserEvent::Leave));
        assert_eq!(
            laser_event(
                &mouse(
                    openvr_sys::EVREventType_VREvent_MouseButtonDown,
                    2,
                    2,
                    900.,
                    64.
                ),
                1
            ),
            None
        );
        assert_eq!(
            laser_event(
                &mouse(
                    openvr_sys::EVREventType_VREvent_MouseMove,
                    2,
                    0,
                    f32::NAN,
                    64.
                ),
                1
            ),
            Some(LaserEvent::Leave)
        );
        let leave = openvr_sys::VREvent_t {
            eventType: openvr_sys::EVREventType_VREvent_FocusLeave as u32,
            ..Default::default()
        };
        assert_eq!(laser_event(&leave, 1), Some(LaserEvent::Leave));
    }
}
