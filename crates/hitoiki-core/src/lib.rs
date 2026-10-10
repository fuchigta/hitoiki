//! Hitoiki のコア: ドメインモデル、ストレージ、集計、帰属判定パイプライン。
//!
//! CLI と Tauri アプリは同じこのクレートの関数を呼ぶ（ロジックの重複禁止、15.3）。

pub mod clock;
pub mod config;
pub mod db;
pub mod error;
pub mod id;
pub mod model;
pub mod paths;
pub mod timefmt;

pub use clock::{Clock, FixedClock, SystemClock};
pub use error::{Error, Result};

/// UTC の Unix ミリ秒。
pub type Millis = i64;
