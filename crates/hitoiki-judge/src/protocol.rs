//! 判定器のプロトコル型（8.3）。外部コマンドの JSON と同じ形。
//! 将来の Jev（Choice / Score / Bool の型付き問い + 確率）に合わせた形にしてある。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;

/// 問いの型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Choice,
    Score,
    Bool,
}

/// 選択肢（id は機械可読、label は人間・LLM 向け）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionSpec {
    pub id: String,
    pub label: String,
}

/// 解決済みの問い（選択肢が確定している）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: QuestionType,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<OptionSpec>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_none: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[i64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentState {
    pub start: String,
    pub end: String,
    pub exe: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StateContext {
    pub doing_task: Option<Value>,
    #[serde(default)]
    pub recent_segments: Vec<Value>,
    #[serde(default)]
    pub calendar: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub segment: SegmentState,
    #[serde(default)]
    pub context: StateContext,
}

/// 問い合わせ。単件は `state`、バッチは `states` を使う。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeRequest {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub states: Option<Vec<State>>,
    pub questions: Vec<QuestionSpec>,
}

impl JudgeRequest {
    pub fn single(state: State, questions: Vec<QuestionSpec>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            state: Some(state),
            states: None,
            questions,
        }
    }
}

/// 1 つの問いへの回答。`value` は choice なら id 文字列("none" 可)、bool なら真偽、score なら整数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub value: Value,
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probs: Option<BTreeMap<String, f64>>,
}

pub type Answers = BTreeMap<String, Answer>;

/// 回答。単件は `answers`、バッチは `results`（`states` と同じ長さ）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct JudgeResponse {
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<Answers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<Answers>>,
}

/// 判定器。差し替え可能（OpenAI 互換 / 外部コマンド / 将来の Jev）。
pub trait Judge: Send + Sync {
    fn name(&self) -> &str;
    fn ask(&self, req: &JudgeRequest) -> anyhow::Result<JudgeResponse>;

    /// バッチ問い合わせ。既定は 1 件ずつ `ask` する。バッチ対応バックエンドは上書きする。
    /// 戻り値は `states` と同じ長さ。個別の失敗は None。
    fn ask_batch(&self, states: &[State], questions: &[QuestionSpec]) -> Vec<Option<Answers>> {
        states
            .iter()
            .map(
                |s| match self.ask(&JudgeRequest::single(s.clone(), questions.to_vec())) {
                    Ok(r) => r.answers,
                    Err(e) => {
                        tracing::warn!("判定器 {} の問い合わせに失敗: {e:#}", self.name());
                        None
                    }
                },
            )
            .collect()
    }
}

/// confidence を 0..1 に丸める（NaN は 0）。
pub fn clamp01(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(0.0, 1.0)
    }
}
