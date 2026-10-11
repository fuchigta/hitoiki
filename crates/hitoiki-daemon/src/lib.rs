//! Hitoiki の常駐処理ランタイム（docs/dev/architecture.md「クレート構成とプロセスモデル」）。
//!
//! `hitoiki daemon`（ヘッドレス）と Tauri 常駐アプリの両方がこれを使う。
//! UI への通知は [`RuntimeSink`] 経由で行う。

pub mod doctor;
pub mod headless;
pub mod push_ctx;
pub mod runtime;
pub mod sink;

pub use doctor::{doctor, doctor_with, DoctorItem, DoctorOptions, DoctorReport, Status};
pub use headless::{init_logging, run_headless, run_headless_with, LogGuard};
pub use push_ctx::{build_context, load_snapshot, push_tick, PushSnapshot};
pub use runtime::{AlreadyRunning, Runtime, RuntimeHandle, RuntimeOptions};
pub use sink::{LogSink, RuntimeSink};
