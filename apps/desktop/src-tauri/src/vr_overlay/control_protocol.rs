use serde::{Deserialize, Serialize};

pub const REQUEST_EVENT: &str = "vr-overlay-control-request";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(
    clippy::enum_variant_names,
    reason = "Wire actions are explicit, idempotent setters."
)]
pub enum ControlAction {
    SetWristEnabled(bool),
    SetHeadsetEnabled(bool),
    SetOscEnabled(bool),
    SetCaptureRunning(bool),
}

impl ControlAction {
    pub fn index(self) -> usize {
        match self {
            Self::SetWristEnabled(_) => 0,
            Self::SetHeadsetEnabled(_) => 1,
            Self::SetOscEnabled(_) => 2,
            Self::SetCaptureRunning(_) => 3,
        }
    }

    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    pub fn target(self) -> bool {
        match self {
            Self::SetWristEnabled(v)
            | Self::SetHeadsetEnabled(v)
            | Self::SetOscEnabled(v)
            | Self::SetCaptureRunning(v) => v,
        }
    }

    pub fn for_index(index: usize, target: bool) -> Self {
        match index {
            0 => Self::SetWristEnabled(target),
            1 => Self::SetHeadsetEnabled(target),
            2 => Self::SetOscEnabled(target),
            _ => Self::SetCaptureRunning(target),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    Dashboard,
    WristMenu,
    DevPreview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlRequest {
    pub owner_epoch: u32,
    pub request_id: u32,
    pub base_revision: u32,
    pub source: ControlSource,
    pub action: ControlAction,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlItem {
    pub requested: bool,
    pub effective: bool,
    pub available: bool,
    pub busy: bool,
    pub label: String,
    pub reason: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlSnapshot {
    pub owner_epoch: u32,
    pub revision: u32,
    pub items: [ControlItem; 4],
    pub dark: bool,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Default)]
pub struct ControlHub {
    active: bool,
    snapshot: ControlSnapshot,
    sequence: u32,
    pending: [Option<ControlRequest>; 4],
    failures: [Option<String>; 4],
    overlay_effective: Option<[bool; 2]>,
    player_audio: super::player_audio_ui::View,
    player_audio_revision: u32,
}

impl ControlHub {
    pub fn player_audio(&self) -> &super::player_audio_ui::View {
        &self.player_audio
    }

    pub fn update_player_audio(
        &mut self,
        epoch: u32,
        revision: u32,
        view: super::player_audio_ui::View,
    ) -> bool {
        if !self.active
            || epoch != self.snapshot.owner_epoch
            || revision <= self.player_audio_revision
        {
            return false;
        }
        self.player_audio_revision = revision;
        self.player_audio = view;
        true
    }

    pub fn player_audio_decision(
        &self,
        request_id: u32,
        accepted: bool,
    ) -> Option<super::player_audio_ui::Decision> {
        let prompt = self.player_audio.notice.as_ref()?;
        (self.active && prompt.request_id == request_id && prompt.remaining_seconds == 0).then_some(
            super::player_audio_ui::Decision {
                owner_epoch: self.snapshot.owner_epoch,
                request_id,
                accepted,
            },
        )
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn snapshot(&self) -> ControlSnapshot {
        let mut view = self.snapshot.clone();
        if let Some(effective) = self.overlay_effective {
            for (item, effective) in view.items.iter_mut().zip(effective) {
                item.effective = effective;
            }
        }
        view
    }

    pub fn set_overlay_effective(&mut self, wrist: bool, headset: bool) {
        self.overlay_effective = Some([wrist, headset]);
    }

    pub fn register(&mut self, mut view: ControlSnapshot) -> Result<ControlSnapshot, String> {
        view.owner_epoch = self
            .snapshot
            .owner_epoch
            .checked_add(1)
            .ok_or("Control epoch exhausted")?;
        view.revision = 1;
        self.snapshot = view;
        self.pending = Default::default();
        self.failures = Default::default();
        self.sequence = 0;
        self.player_audio = Default::default();
        self.player_audio_revision = 0;
        self.active = true;
        Ok(self.snapshot())
    }

    pub fn revoke(&mut self) {
        self.active = false;
        self.player_audio = Default::default();
        self.pending = Default::default();
        for item in &mut self.snapshot.items {
            item.available = false;
            item.busy = false;
        }
    }

    pub fn update(
        &mut self,
        epoch: u32,
        mut view: ControlSnapshot,
    ) -> Result<ControlSnapshot, String> {
        if !self.active || epoch != self.snapshot.owner_epoch {
            return Err("Stale control owner".into());
        }
        view.owner_epoch = epoch;
        view.revision = self.snapshot.revision;
        for (index, item) in view.items.iter_mut().enumerate() {
            item.busy |= self.pending[index].is_some();
            if item.requested != self.snapshot.items[index].requested {
                self.failures[index] = None;
            }
            if item.reason.is_none() {
                item.reason = self.failures[index].clone();
            }
        }
        if self.snapshot != view {
            view.revision = view
                .revision
                .checked_add(1)
                .ok_or("Control revision exhausted")?;
            self.snapshot = view;
        }
        Ok(self.snapshot())
    }

    pub fn begin(
        &mut self,
        source: ControlSource,
        action: ControlAction,
    ) -> Option<ControlRequest> {
        let index = action.index();
        let item = &mut self.snapshot.items[index];
        if !self.active || !item.available || item.busy {
            return None;
        }
        self.sequence = self.sequence.checked_add(1)?;
        let request = ControlRequest {
            owner_epoch: self.snapshot.owner_epoch,
            request_id: self.sequence,
            base_revision: self.snapshot.revision,
            source,
            action,
        };
        item.busy = true;
        item.reason = None;
        self.failures[index] = None;
        self.pending[index] = Some(request.clone());
        Some(request)
    }

    pub fn finish(&mut self, request: &ControlRequest, error: Option<String>) -> bool {
        let index = request.action.index();
        if !self.active || self.pending[index].as_ref() != Some(request) {
            return false;
        }
        self.pending[index] = None;
        self.snapshot.items[index].busy = false;
        self.failures[index] = error.clone();
        self.snapshot.items[index].reason = error;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_is_independent_of_pending_capture_and_rejects_stale_or_early_decisions() {
        use super::super::player_audio_ui::{Prompt, View};
        let mut hub = ready();
        hub.begin(
            ControlSource::WristMenu,
            ControlAction::SetCaptureRunning(true),
        )
        .unwrap();
        let epoch = hub.snapshot().owner_epoch;
        let mut view = View {
            notice: Some(Prompt {
                request_id: 7,
                remaining_seconds: 3,
                ..Default::default()
            }),
            progress: None,
        };
        assert!(hub.update_player_audio(epoch, 1, view.clone()));
        assert!(hub.player_audio_decision(7, true).is_none());
        view.notice.as_mut().unwrap().remaining_seconds = 0;
        assert!(hub.update_player_audio(epoch, 2, view.clone()));
        assert!(!hub.update_player_audio(epoch, 1, View::default()));
        assert!(!hub.update_player_audio(epoch + 1, 3, View::default()));
        assert!(hub.player_audio_decision(6, true).is_none());
        assert!(hub.player_audio_decision(7, true).unwrap().accepted);
        assert!(!hub.player_audio_decision(7, false).unwrap().accepted);
        assert!(hub.snapshot().items[3].busy);
        hub.revoke();
        assert!(hub.player_audio().notice.is_none());
        assert!(hub.player_audio_decision(7, true).is_none());
        hub.register(ControlSnapshot::default()).unwrap();
        assert!(!hub.update_player_audio(epoch, 3, view));
    }

    fn ready() -> ControlHub {
        let mut hub = ControlHub::default();
        let mut view = ControlSnapshot::default();
        for item in &mut view.items {
            item.available = true;
        }
        hub.register(view).unwrap();
        hub
    }

    #[test]
    fn owner_reload_rejects_old_requests_and_results() {
        let mut hub = ready();
        let first = hub
            .begin(
                ControlSource::WristMenu,
                ControlAction::SetWristEnabled(true),
            )
            .unwrap();
        let old_epoch = first.owner_epoch;
        hub.register(ControlSnapshot::default()).unwrap();
        assert_ne!(hub.snapshot().owner_epoch, old_epoch);
        assert!(!hub.finish(&first, None));
        assert!(hub.update(old_epoch, ControlSnapshot::default()).is_err());
    }

    #[test]
    fn one_item_is_busy_until_its_matching_result_and_duplicate_result_is_ignored() {
        let mut hub = ready();
        let request = hub
            .begin(
                ControlSource::Dashboard,
                ControlAction::SetCaptureRunning(true),
            )
            .unwrap();
        assert!(hub.snapshot().items[3].busy);
        assert!(hub
            .begin(
                ControlSource::WristMenu,
                ControlAction::SetCaptureRunning(true)
            )
            .is_none());
        assert!(hub
            .begin(
                ControlSource::WristMenu,
                ControlAction::SetWristEnabled(true)
            )
            .is_some());
        assert!(hub.finish(&request, Some("Failed".into())));
        assert!(!hub.snapshot().items[3].busy);
        assert_eq!(hub.snapshot().items[3].reason.as_deref(), Some("Failed"));
        assert!(!hub.finish(&request, None));
    }

    #[test]
    fn owner_revocation_disables_actions_without_replaying_them() {
        let mut hub = ready();
        hub.revoke();
        assert!(hub
            .begin(ControlSource::WristMenu, ControlAction::SetOscEnabled(true))
            .is_none());
        assert!(hub
            .snapshot()
            .items
            .iter()
            .all(|item| !item.available && !item.busy));
    }
}
