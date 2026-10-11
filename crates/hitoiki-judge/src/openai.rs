//! OpenAI 互換 Chat Completions 判定器（docs/architecture.md「判定器」）。LiteLLM プロキシ（AIGW）を想定。
//!
//! 構造化出力（`json_schema`）で問い合わせ、サーバが 400/422 で拒否したら
//! `json_object` モード（スキーマはプロンプトに埋め込む）に自動でフォールバックする。
//! モデルは確率を返せないため、confidence はモデルの自己申告値を 0..1 に丸めて使う。

use crate::protocol::{
    clamp01, Answer, Answers, Judge, JudgeRequest, JudgeResponse, QuestionSpec, QuestionType, State,
    PROTOCOL_VERSION,
};
use anyhow::{anyhow, bail, Context};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// バッチ問い合わせの 1 回あたり最大件数（既定）。
pub const DEFAULT_BATCH_SIZE: usize = 10;

pub struct OpenAiCompatibleJudge {
    name: String,
    endpoint: String,
    model: String,
    api_key_env: Option<String>,
    agent: ureq::Agent,
    /// json_schema が拒否された後は json_object を使い続ける。
    json_object_mode: AtomicBool,
    batch_size: usize,
}

impl OpenAiCompatibleJudge {
    pub fn new(
        name: impl Into<String>,
        base_url: &str,
        model: impl Into<String>,
        api_key_env: Option<String>,
        timeout: Duration,
    ) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        Self {
            name: name.into(),
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            model: model.into(),
            api_key_env,
            agent,
            json_object_mode: AtomicBool::new(false),
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    /// バッチ問い合わせの最大件数を変える（既定 10）。
    pub fn with_batch_size(mut self, n: usize) -> Self {
        self.batch_size = n.max(1);
        self
    }

    /// json_schema で問い合わせ、400/422 なら json_object に切り替えて再試行する。
    fn chat(&self, payload: &Value, schema: &Value) -> anyhow::Result<Value> {
        let use_object = self.json_object_mode.load(Ordering::Relaxed);
        let body = build_body_value(&self.model, payload, schema, use_object);
        match self.post(&body) {
            Ok(v) => Ok(v),
            Err(PostError::Status(400 | 422)) if !use_object => {
                tracing::info!("json_schema が拒否されたため json_object モードに切り替えます");
                self.json_object_mode.store(true, Ordering::Relaxed);
                let body = build_body_value(&self.model, payload, schema, true);
                match self.post(&body) {
                    Ok(v) => Ok(v),
                    Err(PostError::Status(c)) => bail!("判定 API が HTTP {c} を返しました"),
                    Err(PostError::Other(e)) => Err(e),
                }
            }
            Err(PostError::Status(c)) => bail!("判定 API が HTTP {c} を返しました"),
            Err(PostError::Other(e)) => Err(e),
        }
    }

    fn ask_chunk(
        &self,
        states: &[State],
        questions: &[QuestionSpec],
    ) -> anyhow::Result<Vec<Option<Answers>>> {
        let schema = build_batch_schema(questions, states.len());
        let payload = json!({"states": states, "questions": questions});
        let resp = self.chat(&payload, &schema)?;
        parse_batch_response(&resp, questions, states.len())
    }

    fn post(&self, body: &Value) -> Result<Value, PostError> {
        let mut req = self.agent.post(&self.endpoint);
        if let Some(env) = &self.api_key_env {
            let key = std::env::var(env)
                .map_err(|_| PostError::Other(anyhow!("環境変数 {env} が設定されていません")))?;
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
        match req.send_json(body) {
            Ok(resp) => resp
                .into_json::<Value>()
                .map_err(|e| PostError::Other(anyhow!("応答を JSON として読めません: {e}"))),
            // 本文（モデル出力やエラー詳細）はログに残さない。
            Err(ureq::Error::Status(code, _)) => Err(PostError::Status(code)),
            Err(e) => Err(PostError::Other(anyhow!(
                "HTTP 通信に失敗しました: {}",
                transport_summary(&e)
            ))),
        }
    }
}

enum PostError {
    Status(u16),
    Other(anyhow::Error),
}

fn transport_summary(e: &ureq::Error) -> String {
    // URL を含み得る詳細は避け、種別だけを返す。
    match e {
        ureq::Error::Transport(t) => format!("{:?}", t.kind()),
        ureq::Error::Status(c, _) => format!("status {c}"),
    }
}

impl Judge for OpenAiCompatibleJudge {
    fn name(&self) -> &str {
        &self.name
    }

    fn ask(&self, req: &JudgeRequest) -> anyhow::Result<JudgeResponse> {
        let schema = build_schema(&req.questions);
        let payload = json!({
            "state": req.state,
            "states": req.states,
            "questions": req.questions,
        });
        let resp = self.chat(&payload, &schema)?;
        let answers = parse_chat_response(&resp, &req.questions)?;
        Ok(JudgeResponse {
            version: PROTOCOL_VERSION,
            answers: Some(answers),
            results: None,
        })
    }

    fn ask_batch(&self, states: &[State], questions: &[QuestionSpec]) -> Vec<Option<Answers>> {
        let mut out = Vec::with_capacity(states.len());
        for chunk in states.chunks(self.batch_size) {
            match self.ask_chunk(chunk, questions) {
                Ok(rs) => out.extend(rs),
                Err(e) => {
                    tracing::warn!("判定器 {} のバッチ問い合わせに失敗: {e:#}", self.name);
                    out.extend(chunk.iter().map(|_| None));
                }
            }
        }
        out
    }
}

/// 問いから JSON Schema を作る（strict 構造化出力向け）。
pub fn build_schema(questions: &[QuestionSpec]) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = vec![];
    for q in questions {
        let value = match q.kind {
            QuestionType::Choice => {
                let mut ids: Vec<Value> = q
                    .options
                    .iter()
                    .flatten()
                    .map(|o| Value::String(o.id.clone()))
                    .collect();
                if q.allow_none {
                    ids.push(Value::String("none".into()));
                }
                json!({"type": "string", "enum": ids})
            }
            QuestionType::Bool => json!({"type": "boolean"}),
            QuestionType::Score => {
                let [lo, hi] = q.scale.unwrap_or([1, 5]);
                json!({"type": "integer", "minimum": lo, "maximum": hi})
            }
        };
        props.insert(
            q.name.clone(),
            json!({
                "type": "object",
                "properties": {
                    "value": value,
                    "confidence": {"type": "number", "description": "0 から 1 の自己申告の確信度"}
                },
                "required": ["value", "confidence"],
                "additionalProperties": false
            }),
        );
        required.push(Value::String(q.name.clone()));
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
}

/// バッチ用の JSON Schema。`{"results": [ {各質問...}, ... ]}`（件数は `n` 固定）。
pub fn build_batch_schema(questions: &[QuestionSpec], n: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "results": {
                "type": "array",
                "minItems": n,
                "maxItems": n,
                "items": build_schema(questions)
            }
        },
        "required": ["results"],
        "additionalProperties": false
    })
}

const SYSTEM_PROMPT: &str =
    "あなたは作業ログの分類器です。与えられた作業区間(state)について、各質問に答えてください。\
choice の value には選択肢の id(または none)を、bool には真偽を、score には整数を返します。\
各回答には 0 から 1 の確信度 confidence を付けてください。根拠が乏しければ低くしてください。\
出力は指定された JSON のみで、説明文は不要です。\
入力に states 配列がある場合は、各 state について同じ順序・同じ件数で results 配列に回答を返してください。";

#[cfg(test)]
fn build_body(model: &str, req: &JudgeRequest, schema: &Value, json_object: bool) -> Value {
    let payload = json!({
        "state": req.state,
        "states": req.states,
        "questions": req.questions,
    });
    build_body_value(model, &payload, schema, json_object)
}

fn build_body_value(model: &str, payload: &Value, schema: &Value, json_object: bool) -> Value {
    let mut user = serde_json::to_string_pretty(payload).unwrap_or_default();
    let response_format = if json_object {
        user.push_str("\n\n次の JSON Schema に従う JSON オブジェクトだけを返してください:\n");
        user.push_str(&schema.to_string());
        json!({"type": "json_object"})
    } else {
        json!({
            "type": "json_schema",
            "json_schema": {"name": "hitoiki_answers", "strict": true, "schema": schema}
        })
    };
    json!({
        "model": model,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user},
        ],
        "response_format": response_format,
    })
}

fn parse_content(resp: &Value) -> anyhow::Result<Value> {
    let content = resp
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("応答に choices[0].message.content がありません"))?;
    serde_json::from_str(strip_fence(content)).context("モデル出力が JSON として不正です")
}

/// 1 件分の回答オブジェクトを検証して取り出す。範囲外の回答は捨てる（空もあり得る）。
fn extract_answers(parsed: &Value, questions: &[QuestionSpec]) -> anyhow::Result<Answers> {
    let obj = parsed
        .as_object()
        .ok_or_else(|| anyhow!("モデル出力がオブジェクトではありません"))?;
    let mut out = Answers::new();
    for q in questions {
        let Some(a) = obj.get(&q.name) else { continue };
        let (value, conf) = match (a.get("value"), a.get("confidence")) {
            (Some(v), c) => (v.clone(), c.and_then(Value::as_f64).unwrap_or(0.5)),
            _ => continue,
        };
        if !value_valid(q, &value) {
            tracing::warn!("質問 {} の回答が選択肢・範囲外のため無視します", q.name);
            continue;
        }
        out.insert(
            q.name.clone(),
            Answer {
                value,
                confidence: clamp01(conf),
                probs: None,
            },
        );
    }
    Ok(out)
}

/// Chat Completions の応答から回答を取り出して検証する。範囲外の回答は捨てる。
pub fn parse_chat_response(resp: &Value, questions: &[QuestionSpec]) -> anyhow::Result<Answers> {
    let out = extract_answers(&parse_content(resp)?, questions)?;
    if out.is_empty() {
        bail!("有効な回答がありません");
    }
    Ok(out)
}

/// バッチ応答（`{"results": [...]}`）を解釈する。件数が `n` と違えば全体をエラーにする。
/// 個々の要素に有効な回答が無ければその要素だけ None。
pub fn parse_batch_response(
    resp: &Value,
    questions: &[QuestionSpec],
    n: usize,
) -> anyhow::Result<Vec<Option<Answers>>> {
    let parsed = parse_content(resp)?;
    let items = parsed
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("モデル出力に results 配列がありません"))?;
    if items.len() != n {
        bail!("バッチ応答の件数が一致しません (送信 {n} / 受信 {})", items.len());
    }
    Ok(items
        .iter()
        .map(|it| extract_answers(it, questions).ok().filter(|a| !a.is_empty()))
        .collect())
}

/// 回答値が問いの型・選択肢・範囲に合っているか。
pub fn value_valid(q: &QuestionSpec, v: &Value) -> bool {
    match q.kind {
        QuestionType::Bool => v.is_boolean(),
        QuestionType::Score => {
            let [lo, hi] = q.scale.unwrap_or([i64::MIN, i64::MAX]);
            v.as_i64().is_some_and(|n| n >= lo && n <= hi)
        }
        QuestionType::Choice => v
            .as_str()
            .is_some_and(|s| (q.allow_none && s == "none") || q.options.iter().flatten().any(|o| o.id == s)),
    }
}

fn strip_fence(s: &str) -> &str {
    let t = s.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim().strip_suffix("```").unwrap_or(rest).trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::OptionSpec;

    fn qs() -> Vec<QuestionSpec> {
        vec![
            QuestionSpec {
                name: "task".into(),
                kind: QuestionType::Choice,
                description: "d".into(),
                options: Some(vec![OptionSpec {
                    id: "T1".into(),
                    label: "仕事".into(),
                }]),
                allow_none: true,
                scale: None,
            },
            QuestionSpec {
                name: "flag".into(),
                kind: QuestionType::Bool,
                description: String::new(),
                options: None,
                allow_none: false,
                scale: None,
            },
            QuestionSpec {
                name: "urgency".into(),
                kind: QuestionType::Score,
                description: String::new(),
                options: None,
                allow_none: false,
                scale: Some([1, 5]),
            },
        ]
    }

    #[test]
    fn schema_shape() {
        let s = build_schema(&qs());
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(
            s["properties"]["task"]["properties"]["value"]["enum"],
            json!(["T1", "none"])
        );
        assert_eq!(s["properties"]["flag"]["properties"]["value"]["type"], "boolean");
        let u = &s["properties"]["urgency"]["properties"]["value"];
        assert_eq!((u["minimum"].as_i64(), u["maximum"].as_i64()), (Some(1), Some(5)));
        assert_eq!(s["required"], json!(["task", "flag", "urgency"]));
    }

    fn chat(content: &str) -> Value {
        json!({"choices": [{"message": {"content": content}}]})
    }

    #[test]
    fn parse_clamps_and_validates() {
        let c = r#"```json
{"task":{"value":"T1","confidence":1.7},"flag":{"value":"yes","confidence":0.9},"urgency":{"value":9,"confidence":0.5}}
```"#;
        let a = parse_chat_response(&chat(c), &qs()).unwrap();
        assert_eq!(a["task"].confidence, 1.0);
        assert!(!a.contains_key("flag"));
        assert!(!a.contains_key("urgency"));
    }

    #[test]
    fn batch_schema_and_parse() {
        let s = build_batch_schema(&qs(), 3);
        assert_eq!(s["properties"]["results"]["minItems"], 3);
        assert_eq!(s["properties"]["results"]["maxItems"], 3);
        assert_eq!(
            s["properties"]["results"]["items"]["required"],
            json!(["task", "flag", "urgency"])
        );
        let c = r#"{"results":[{"task":{"value":"T1","confidence":0.5}},{"task":{"value":"zzz","confidence":1}}]}"#;
        let r = parse_batch_response(&chat(c), &qs(), 2).unwrap();
        assert!(r[0].is_some());
        assert!(r[1].is_none());
        assert!(parse_batch_response(&chat(c), &qs(), 3).is_err());
        assert!(parse_batch_response(&chat("{}"), &qs(), 1).is_err());
    }

    #[test]
    fn parse_errors() {
        assert!(parse_chat_response(&chat("not json"), &qs()).is_err());
        assert!(parse_chat_response(&json!({}), &qs()).is_err());
        assert!(parse_chat_response(&chat(r#"{"task":{"value":"zzz","confidence":1}}"#), &qs()).is_err());
    }

    #[test]
    fn body_modes() {
        let req = JudgeRequest::single(
            crate::protocol::State {
                segment: crate::protocol::SegmentState {
                    start: "s".into(),
                    end: "e".into(),
                    exe: None,
                    title: None,
                    cwd: None,
                },
                context: Default::default(),
            },
            qs(),
        );
        let schema = build_schema(&req.questions);
        let b = build_body("m", &req, &schema, false);
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(b["response_format"]["json_schema"]["strict"], true);
        let b = build_body("m", &req, &schema, true);
        assert_eq!(b["response_format"]["type"], "json_object");
        assert!(b["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("JSON Schema"));
    }
}
