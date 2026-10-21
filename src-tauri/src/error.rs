//! Application-wide error type. Every IPC command returns `AppResult<T>`; the
//! serialized form is `{ code, message }` so the UI can pick a presentation
//! without parsing text. Messages are user-facing Russian strings and must
//! never contain credentials.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Platform(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    Network(String),
    #[error("{0}")]
    Provider(String),
    #[error("Задача отменена")]
    Cancelled,
    #[error("{0}")]
    Timeout(String),
    #[error("{0}")]
    Storage(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Platform(_) => "platform",
            AppError::Unsupported(_) => "unsupported",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::NotFound(_) => "not_found",
            AppError::PermissionDenied(_) => "permission_denied",
            AppError::Network(_) => "network",
            AppError::Provider(_) => "provider",
            AppError::Cancelled => "cancelled",
            AppError::Timeout(_) => "timeout",
            AppError::Storage(_) => "storage",
            AppError::Internal(_) => "internal",
        }
    }

    pub fn platform(msg: impl Into<String>) -> Self {
        AppError::Platform(msg.into())
    }
    pub fn invalid(msg: impl Into<String>) -> Self {
        AppError::InvalidInput(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        AppError::NotFound(msg.into())
    }
    pub fn unsupported(msg: impl Into<String>) -> Self {
        AppError::Unsupported(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for AppError {
    fn from(e: windows::core::Error) -> Self {
        AppError::Platform(format!("Ошибка Windows API (0x{:08X}): {}", e.code().0, e.message()))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Storage(format!("Ошибка базы данных: {e}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::InvalidInput(format!("Некорректный JSON: {e}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => AppError::NotFound(format!("Файл не найден: {e}")),
            std::io::ErrorKind::PermissionDenied => {
                AppError::PermissionDenied(format!("Нет доступа: {e}"))
            }
            _ => AppError::Platform(format!("Ошибка ввода-вывода: {e}")),
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        // reqwest errors can embed the request URL; strip it so query
        // parameters never leak into UI or logs.
        let e = e.without_url();
        if e.is_timeout() {
            AppError::Timeout("Сервер не ответил вовремя".into())
        } else if e.is_connect() {
            AppError::Network("Нет соединения с сервером".into())
        } else {
            AppError::Network(format!("Сетевая ошибка: {e}"))
        }
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Internal(format!("Ошибка Tauri: {e}"))
    }
}

pub type AppResult<T> = Result<T, AppError>;
