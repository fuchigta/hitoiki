//! UI 側への通知口。Tauri 常駐アプリはイベント送信・トースト表示を、ヘッドレスはログ出力を実装する。

use hitoiki_agents::ImportStats;
use hitoiki_core::push::PushPayload;
use hitoiki_core::timefmt::Range;

/// 常駐処理からの通知を受ける側。全メソッドは任意のスレッドから呼ばれる。
/// 重い処理を行わず、速やかに戻ること。
pub trait RuntimeSink: Send + Sync {
    /// 帰属判定の再計算が終わり、`range` の区間が更新された。
    fn segments_updated(&self, range: Range);
    /// プッシュ通知を出す。`push_id` は `push_log.id`（応答の保存に使う）。
    fn push(&self, push_id: &str, payload: &PushPayload);
    /// doing のタスクが変わった（CLI の `task start` 等）。
    fn doing_changed(&self, task_id: Option<&str>);
    /// エージェントログの取り込みが完了した。
    fn import_completed(&self, stats: &ImportStats);
    /// UI を開く要求（`hitoiki ui`）。
    fn open_ui(&self, screen: Option<&str>);
    /// IPC で終了が要求された。呼び出し側が `RuntimeHandle::stop` を行うこと。
    fn shutdown_requested(&self);
}

/// tracing に出すだけの実装。ウインドウタイトル等の内容は出さない。
#[derive(Debug, Default, Clone, Copy)]
pub struct LogSink;

impl RuntimeSink for LogSink {
    fn segments_updated(&self, range: Range) {
        tracing::info!(start = range.start, end = range.end, "区間を更新しました");
    }

    fn push(&self, push_id: &str, payload: &PushPayload) {
        tracing::info!(push_id, trigger = %payload.trigger, "プッシュ通知（ヘッドレスのため表示しません）");
    }

    fn doing_changed(&self, task_id: Option<&str>) {
        tracing::info!(task_id = task_id.unwrap_or("-"), "doing タスクが変わりました");
    }

    fn import_completed(&self, stats: &ImportStats) {
        tracing::info!(
            files_updated = stats.files_updated,
            events_inserted = stats.events_inserted,
            files_failed = stats.files_failed,
            "エージェントログを取り込みました"
        );
    }

    fn open_ui(&self, screen: Option<&str>) {
        tracing::info!(
            screen = screen.unwrap_or("-"),
            "UI を開く要求（ヘッドレスのため無視）"
        );
    }

    fn shutdown_requested(&self) {
        tracing::info!("終了が要求されました");
    }
}
