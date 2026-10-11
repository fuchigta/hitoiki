use thiserror::Error;

/// コアのエラー。CLI の終了コード（docs/architecture.md「CLI」）に対応付けられるよう種別を持つ。
#[derive(Debug, Error)]
pub enum Error {
    #[error("引数が不正です: {0}")]
    InvalidArgument(String),
    #[error("対象が見つかりません: {0}")]
    NotFound(String),
    #[error("候補が複数あります: {0}")]
    Ambiguous(String),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    /// CLI の終了コード: 1 一般、2 引数、3 対象なし、4 曖昧。
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::InvalidArgument(_) => 2,
            Error::NotFound(_) => 3,
            Error::Ambiguous(_) => 4,
            _ => 1,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
