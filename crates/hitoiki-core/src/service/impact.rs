//! 削除の影響（確認ダイアログ・`--force` / `--dry-run` の共通部品）。
//!
//! 削除系のユースケースは影響を先に数え、影響があるのに `force` が無ければ失敗する。
//! `dry_run` なら数えるだけで何も変更しない。UI は dry_run で影響を表示し、確認後に `force` つきで削除する。

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// 影響を受ける件数 1 行（例: 「紐付いたタスク」3 件）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImpactLine {
    pub label: String,
    pub count: i64,
}

/// 削除の影響の一覧。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Impact {
    /// 対象の種類（`goal` / `key_result` / `skill` / ...）。
    pub entity: String,
    pub id: String,
    /// 確認ダイアログに出す名前。
    pub name: String,
    pub lines: Vec<ImpactLine>,
    /// 補足（取り込み済みプリセットの扱いなど）。
    pub note: Option<String>,
    /// 警告（振り返りの回答から参照されている、など）。1 件以上あると `--force` が必要になる。
    pub warnings: Vec<String>,
}

impl Impact {
    pub fn new(entity: &str, id: &str, name: &str) -> Self {
        Self {
            entity: entity.into(),
            id: id.into(),
            name: name.into(),
            lines: vec![],
            note: None,
            warnings: vec![],
        }
    }

    /// 件数が 1 以上の行だけ追加する。
    pub fn line(mut self, label: &str, count: i64) -> Self {
        if count > 0 {
            self.lines.push(ImpactLine {
                label: label.into(),
                count,
            });
        }
        self
    }

    pub fn warning(mut self, text: String) -> Self {
        self.warnings.push(text);
        self
    }

    pub fn note(mut self, note: &str) -> Self {
        self.note = Some(note.into());
        self
    }

    /// 影響の合計件数。0 なら確認なしで消してよい。
    pub fn total(&self) -> i64 {
        self.lines.iter().map(|l| l.count).sum::<i64>() + self.warnings.len() as i64
    }

    /// 「紐付いたタスク 3 件、配下の成果の指標 2 件」のような 1 行の説明。
    pub fn summary(&self) -> String {
        self.lines
            .iter()
            .map(|l| format!("{} {} 件", l.label, l.count))
            .chain(self.warnings.iter().cloned())
            .collect::<Vec<_>>()
            .join("、")
    }
}

/// 削除の動作指定。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub struct DeleteMode {
    /// 数えるだけで削除しない。
    pub dry_run: bool,
    /// 影響があっても削除する。
    pub force: bool,
}

impl DeleteMode {
    pub const FORCE: DeleteMode = DeleteMode {
        dry_run: false,
        force: true,
    };
    pub const DRY_RUN: DeleteMode = DeleteMode {
        dry_run: true,
        force: false,
    };
}

/// 削除の結果。
#[derive(Debug, Clone, Serialize)]
pub struct DeleteOutcome {
    pub dry_run: bool,
    pub deleted: bool,
    pub impact: Impact,
}

impl DeleteOutcome {
    /// 実際に削除してよいか判定する。dry_run なら `false`、影響ありで force なしならエラー。
    pub(crate) fn gate(mode: DeleteMode, impact: &Impact) -> Result<bool> {
        if mode.dry_run {
            return Ok(false);
        }
        if impact.total() > 0 && !mode.force {
            return Err(Error::InvalidArgument(format!(
                "「{}」を削除すると {} に影響します。削除するには --force を指定してください（--dry-run で確認のみ）",
                impact.name,
                impact.summary()
            )));
        }
        Ok(true)
    }

    pub(crate) fn new(mode: DeleteMode, impact: Impact, deleted: bool) -> Self {
        Self {
            dry_run: mode.dry_run,
            deleted,
            impact,
        }
    }
}
