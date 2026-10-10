//! UI へ返すエラー。`{ code, message }` の形で JSON 化される。

use hitoiki_core::Error;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// 機械可読な種別: invalid_argument / not_found / ambiguous / db / io / internal
    pub code: &'static str,
    pub message: String,
}

impl AppError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: "internal",
            message: msg.into(),
        }
    }
}

impl From<Error> for AppError {
    fn from(e: Error) -> Self {
        let code = match &e {
            Error::InvalidArgument(_) => "invalid_argument",
            Error::NotFound(_) => "not_found",
            Error::Ambiguous(_) => "ambiguous",
            Error::Db(_) => "db",
            Error::Io(_) => "io",
            Error::Other(_) => "internal",
        };
        Self {
            code,
            message: e.to_string(),
        }
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self::internal(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn コードに変換される() {
        let e: AppError = Error::NotFound("x".into()).into();
        assert_eq!(e.code, "not_found");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "not_found");
        assert!(v["message"].as_str().unwrap().contains('x'));
    }
}
