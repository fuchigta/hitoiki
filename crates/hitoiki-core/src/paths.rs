//! ファイル配置（docs/dev/architecture.md「ファイル配置と設定」）。環境変数 `HITOIKI_HOME` が設定されていれば、
//! すべてをその配下に置く（テスト・ポータブル利用向け）。

use std::path::PathBuf;

fn home_override() -> Option<PathBuf> {
    std::env::var_os("HITOIKI_HOME").map(PathBuf::from)
}

/// 設定ディレクトリ（`%APPDATA%\hitoiki`）。
pub fn config_dir() -> PathBuf {
    home_override().unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hitoiki")
    })
}

/// データディレクトリ（`%LOCALAPPDATA%\hitoiki`）。
pub fn data_dir() -> PathBuf {
    home_override().unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hitoiki")
    })
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn db_file() -> PathBuf {
    data_dir().join("hitoiki.db")
}

pub fn log_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn plugins_dir() -> PathBuf {
    config_dir().join("plugins")
}

pub fn questions_dir() -> PathBuf {
    config_dir().join("questions")
}

pub fn templates_dir() -> PathBuf {
    config_dir().join("templates")
}
