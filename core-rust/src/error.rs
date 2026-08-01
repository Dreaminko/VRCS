//! Core 领域错误；服务层据此选择 HTTP 状态，而不是解析错误文本。

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn validation(error: impl Into<String>) -> Self {
        Self::Validation(error.into())
    }

    pub fn internal(error: impl Into<String>) -> Self {
        Self::Internal(error.into())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
