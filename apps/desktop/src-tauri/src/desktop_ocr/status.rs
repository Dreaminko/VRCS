use serde::Serialize;
use vrcs_core::ocr::TranslatedBlock;

#[derive(Clone, Serialize)]
pub struct DesktopOcrStatus {
    pub scan_id: u64,
    pub revision: u64,
    pub state: &'static str,
    pub blocks: Vec<TranslatedBlock>,
    pub error: Option<String>,
    pub shortcut_error: Option<String>,
    pub timed_out: bool,
}

impl Default for DesktopOcrStatus {
    fn default() -> Self {
        Self {
            scan_id: 0,
            revision: 0,
            state: "disabled",
            blocks: Vec::new(),
            error: None,
            shortcut_error: None,
            timed_out: false,
        }
    }
}

impl DesktopOcrStatus {
    pub fn update(&mut self, scan_id: u64, update: impl FnOnce(&mut Self)) -> bool {
        if self.scan_id != scan_id {
            return false;
        }
        update(self);
        self.revision += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_or_closed_scan_cannot_publish_a_late_result() {
        let mut status = DesktopOcrStatus {
            scan_id: 2,
            state: "idle",
            revision: 3,
            ..Default::default()
        };
        assert!(!status.update(1, |status| status.state = "complete"));
        assert_eq!((status.state, status.revision), ("idle", 3));
        assert!(status.update(2, |status| status.state = "recognizing"));
        assert_eq!((status.state, status.revision), ("recognizing", 4));
    }
}
