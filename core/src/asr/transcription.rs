use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
}
