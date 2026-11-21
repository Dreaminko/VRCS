//! Platform capture failures share one recovery vocabulary.
use super::recovery::Fault;

#[derive(Debug, Clone)]
pub(crate) struct CaptureError {
    pub kind: Fault,
    pub stage: String,
    pub hresult: Option<i32>,
}

impl CaptureError {
    pub fn new(kind: Fault, stage: impl Into<String>) -> Self {
        Self {
            kind,
            stage: stage.into(),
            hresult: None,
        }
    }
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.stage)?;
        if let Some(code) = self.hresult {
            write!(f, ": HRESULT {code:#010x}")?;
        }
        Ok(())
    }
}

impl From<&str> for CaptureError {
    fn from(value: &str) -> Self {
        Self::new(super::recovery::Fault::InvalidFrame, value)
    }
}

impl From<String> for CaptureError {
    fn from(value: String) -> Self {
        Self::new(super::recovery::Fault::InvalidFrame, value)
    }
}

impl From<CaptureError> for String {
    fn from(value: CaptureError) -> Self {
        value.to_string()
    }
}
