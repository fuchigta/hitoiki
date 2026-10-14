//! AI 振り返りの構造化出力（docs/dev/architecture.md「AI 振り返り」）。
//!
//! 問いを「チェックイン / ハッスル / ABC / 自由記述」の種類つきで JSON として受け取り、画面で種類ごとに
//! 答えられるようにする。ここにはスキーマの定義と生成、AI コマンドの出力の読み取り、Markdown への整形、
//! コマンド引数の組み立てを置く。AI コマンドの起動と保存は `reflect.rs`。

use crate::config::{Ai, AiStructured};
use crate::id::new_id;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 1 回の振り返りで受け付ける問いの数の上限（AI が多く返しても切り詰める）。
pub const MAX_QUESTIONS: usize = 8;

/// 問いの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    Checkin,
    Hassle,
    Abc,
    Free,
}

impl QuestionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            QuestionKind::Checkin => "checkin",
            QuestionKind::Hassle => "hassle",
            QuestionKind::Abc => "abc",
            QuestionKind::Free => "free",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            QuestionKind::Checkin => "調子",
            QuestionKind::Hassle => "気になったこと",
            QuestionKind::Abc => "捉え直し",
            QuestionKind::Free => "自由記述",
        }
    }
}

/// チェックインで尋ねる尺度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    Energy,
    Mood,
    Stress,
}

impl Scale {
    pub const ALL: [Scale; 3] = [Scale::Energy, Scale::Mood, Scale::Stress];

    pub fn label(&self) -> &'static str {
        match self {
            Scale::Energy => "元気",
            Scale::Mood => "気分",
            Scale::Stress => "ストレス",
        }
    }
}

/// 問い 1 件。種類ごとの項目は、使わない種類では空にする（スキーマはフラットな形）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub kind: QuestionKind,
    pub prompt: String,
    /// checkin: 尋ねる尺度。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scales: Vec<Scale>,
    /// hassle / abc: 例示。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    /// abc: A（出来事）の候補。ユーザーが編集できる。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activating_event: Option<String>,
    /// abc: B を掘り下げる問い。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub belief_prompt: Option<String>,
}

/// 構造化出力の全体。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StructuredOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub questions: Vec<Question>,
}

impl StructuredOutput {
    /// 内容を検査し、正規化する（空の問いは捨てる、checkin の尺度の既定、数の上限）。
    /// 問いが 1 つも残らなければスキーマ違反として扱う。
    pub fn normalize(mut self) -> Result<Self> {
        self.summary = self
            .summary
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        for q in &mut self.questions {
            q.prompt = q.prompt.trim().to_string();
            q.examples.retain(|e| !e.trim().is_empty());
            q.activating_event = q
                .activating_event
                .take()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            q.belief_prompt = q
                .belief_prompt
                .take()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if q.kind == QuestionKind::Checkin && q.scales.is_empty() {
                q.scales = Scale::ALL.to_vec();
            }
            dedup(&mut q.scales);
        }
        self.questions.retain(|q| !q.prompt.is_empty());
        self.questions.truncate(MAX_QUESTIONS);
        if self.questions.is_empty() {
            return Err(Error::Other(anyhow::anyhow!(
                "構造化出力に有効な問いがありません"
            )));
        }
        Ok(self)
    }

    /// 表示互換のための Markdown（`reflection.questions_md` に保存する）。
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        if let Some(sum) = &self.summary {
            s += &format!("## 要約\n\n{sum}\n\n");
        }
        s += "## 問い\n\n";
        for (i, q) in self.questions.iter().enumerate() {
            s += &format!("{}. {}\n", i + 1, q.prompt);
            match q.kind {
                QuestionKind::Checkin => {
                    let names: Vec<&str> = q.scales.iter().map(Scale::label).collect();
                    s += &format!("   - 調子（{}）\n", names.join("・"));
                }
                QuestionKind::Abc => {
                    if let Some(a) = &q.activating_event {
                        s += &format!("   - 出来事の候補: {a}\n");
                    }
                    if let Some(b) = &q.belief_prompt {
                        s += &format!("   - 受け止め方を探る問い: {b}\n");
                    }
                }
                _ => {}
            }
            if !q.examples.is_empty() {
                s += &format!("   - 例: {}\n", q.examples.join(" / "));
            }
        }
        s.trim_end().to_string()
    }
}

fn dedup<T: PartialEq + Copy>(v: &mut Vec<T>) {
    let mut seen: Vec<T> = Vec::new();
    v.retain(|x| {
        if seen.contains(x) {
            false
        } else {
            seen.push(*x);
            true
        }
    });
}

/// 構造化出力の JSON Schema（文字列）。`oneOf` に頼らず、`kind` の列挙と全種類の項目を
/// 任意にしたフラットな形にする（claude の `--json-schema` で受理されることを確認済み）。
pub fn schema_json() -> String {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string", "description": "要約（Markdown）。任意" },
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["checkin", "hassle", "abc", "free"] },
                        "prompt": { "type": "string" },
                        "scales": {
                            "type": "array",
                            "items": { "type": "string", "enum": ["energy", "mood", "stress"] }
                        },
                        "examples": { "type": "array", "items": { "type": "string" } },
                        "activating_event": { "type": "string" },
                        "belief_prompt": { "type": "string" }
                    },
                    "required": ["kind", "prompt"]
                }
            }
        },
        "required": ["questions"]
    });
    schema.to_string()
}

/// AI コマンドの標準出力から構造化出力を取り出す。
///
/// 標準出力には JSON が複数連結されていたり、配列でまとまっていたりすることがある。先頭から順に
/// JSON 値を読み、`structured_output` を持つオブジェクトを採用する。それが無ければ（`codex` のように）
/// スキーマに従う JSON そのものが出ている場合に備えて、`questions` を持つオブジェクトを採用する。
/// `type == "result"` で `is_error` が真のものはエラー。見つからない・スキーマに合わない場合もエラー。
pub fn parse_output(stdout: &str) -> Result<StructuredOutput> {
    from_value(&extract_value(stdout, "questions")?)
}

/// `parse_output` の前半。標準出力から `structured_output`（無ければ `bare_key` を持つオブジェクト）を取り出す。
/// ERG の判定など、問い以外の構造化出力でも使う。
pub fn extract_value(stdout: &str, bare_key: &str) -> Result<Value> {
    let mut values: Vec<Value> = Vec::new();
    for v in serde_json::Deserializer::from_str(stdout).into_iter::<Value>() {
        match v {
            Ok(Value::Array(a)) => values.extend(a),
            Ok(v) => values.push(v),
            Err(_) => break, // 以降は読めない。ここまでに見つかったものだけで判断する
        }
    }
    let mut error: Option<String> = None;
    let mut bare: Option<&Value> = None;
    for v in &values {
        let Some(obj) = v.as_object() else { continue };
        if let Some(so) = obj.get("structured_output").filter(|x| !x.is_null()) {
            return Ok(so.clone());
        }
        let is_result = obj.get("type").and_then(Value::as_str) == Some("result");
        if is_result && obj.get("is_error").and_then(Value::as_bool) == Some(true) {
            let sub = obj.get("subtype").and_then(Value::as_str).unwrap_or("error");
            error.get_or_insert_with(|| format!("AI コマンドがエラーを返しました ({sub})"));
        }
        if bare.is_none() && obj.contains_key(bare_key) {
            bare = Some(v);
        }
    }
    if let Some(v) = bare {
        return Ok(v.clone());
    }
    Err(Error::Other(anyhow::anyhow!(error.unwrap_or_else(|| {
        if values.is_empty() {
            "AI コマンドの出力を JSON として読めませんでした".to_string()
        } else {
            "AI コマンドの出力に structured_output がありません".to_string()
        }
    }))))
}

/// 文字列に包まれた JSON も受け付けて、JSON 値にする。
pub fn unwrap_json_string(v: &Value) -> Result<Value> {
    match v.as_str() {
        Some(s) => serde_json::from_str::<Value>(s)
            .map_err(|e| Error::Other(anyhow::anyhow!("structured_output を JSON として読めません: {e}"))),
        None => Ok(v.clone()),
    }
}

fn from_value(v: &Value) -> Result<StructuredOutput> {
    let v = &unwrap_json_string(v)?;
    let out: StructuredOutput = serde_json::from_value(v.clone())
        .map_err(|e| Error::Other(anyhow::anyhow!("構造化出力がスキーマに合いません: {e}")))?;
    out.normalize()
}

/// 構造化出力を使うか（`ai.structured`）。`auto` は `ai.command` の先頭が `claude` のときだけ。
pub fn enabled(ai: &Ai) -> bool {
    match ai.structured {
        AiStructured::On => true,
        AiStructured::Off => false,
        AiStructured::Auto => is_claude(&ai.command),
    }
}

pub use crate::config::is_claude;

/// 一時ファイルに書いた JSON Schema。破棄時に削除する。
pub struct SchemaFile(PathBuf);

impl SchemaFile {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SchemaFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 構造化出力のために `command` の後ろへ付ける引数と、必要なら一時スキーマファイル。
/// `ai.structured_args` が空なら、`claude` 用の既定を使い、それ以外は何も付けない。
pub fn build_args(ai: &Ai, schema: &str) -> Result<(Vec<String>, Option<SchemaFile>)> {
    let template: Vec<String> = if !ai.structured_args.is_empty() {
        ai.structured_args.clone()
    } else if is_claude(&ai.command) {
        ["--output-format", "json", "--json-schema", "{schema}"]
            .map(String::from)
            .to_vec()
    } else {
        Vec::new()
    };
    let file = if template.iter().any(|a| a.contains("{schema_file}")) {
        let p = std::env::temp_dir().join(format!("hitoiki-schema-{}.json", new_id()));
        std::fs::write(&p, schema)?;
        Some(SchemaFile(p))
    } else {
        None
    };
    let path = file
        .as_ref()
        .map(|f| f.path().to_string_lossy().into_owned())
        .unwrap_or_default();
    let args = template
        .iter()
        .map(|a| a.replace("{schema_file}", &path).replace("{schema}", schema))
        .collect();
    Ok((args, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    const OUT: &str = r#"{"summary":"要約","questions":[
        {"kind":"checkin","prompt":"いまの調子は","scales":["energy","mood"]},
        {"kind":"hassle","prompt":"引っかかったことは","examples":["会議が長い"],"erg_candidates":["growth"]},
        {"kind":"abc","prompt":"場面を選んで","activating_event":"レビューで指摘","belief_prompt":"何がよぎった"},
        {"kind":"free","prompt":"ひとことで"}]}"#;

    #[test]
    fn schema_is_flat_and_valid_json() {
        let v: Value = serde_json::from_str(&schema_json()).unwrap();
        assert_eq!(v["type"], "object");
        let kinds = &v["properties"]["questions"]["items"]["properties"]["kind"]["enum"];
        assert_eq!(kinds, &serde_json::json!(["checkin", "hassle", "abc", "free"]));
        assert!(!schema_json().contains("oneOf"));
        assert_eq!(v["required"], serde_json::json!(["questions"]));
        let props = v["properties"]["questions"]["items"]["properties"]
            .as_object()
            .unwrap();
        for k in ["scales", "examples", "activating_event", "belief_prompt"] {
            assert!(props.contains_key(k), "{k}");
        }
        // ERG は問いに含めない（記録の内容から AI が判定する。service/erg.rs）。過去の JSON に残っていても読める（OUT）。
        assert!(!props.contains_key("erg_candidates"));
    }

    #[test]
    fn parses_claude_envelope() {
        let env = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false,
            "result": "text", "structured_output": serde_json::from_str::<Value>(OUT).unwrap()
        });
        let o = parse_output(&env.to_string()).unwrap();
        assert_eq!(o.summary.as_deref(), Some("要約"));
        assert_eq!(o.questions.len(), 4);
        assert_eq!(o.questions[0].scales, vec![Scale::Energy, Scale::Mood]);
        assert_eq!(o.questions[2].activating_event.as_deref(), Some("レビューで指摘"));
    }

    #[test]
    fn parses_concatenated_json_and_arrays() {
        let init = r#"{"type":"system","subtype":"init"}"#;
        let res = format!(r#"{{"type":"result","is_error":false,"structured_output":{OUT}}}"#);
        let o = parse_output(&format!("{init}\n{res}\n")).unwrap();
        assert_eq!(o.questions.len(), 4);
        let o = parse_output(&format!("[{init},{res}]")).unwrap();
        assert_eq!(o.questions.len(), 4);
        // 後ろにゴミが付いていても、見つかっていれば採用する
        let o = parse_output(&format!("{res}\nゴミ")).unwrap();
        assert_eq!(o.questions.len(), 4);
    }

    #[test]
    fn parses_bare_and_string_wrapped() {
        assert_eq!(parse_output(OUT).unwrap().questions.len(), 4);
        let wrapped = serde_json::json!({"structured_output": OUT});
        assert_eq!(parse_output(&wrapped.to_string()).unwrap().questions.len(), 4);
    }

    #[test]
    fn missing_structured_output_is_error() {
        let e = parse_output(r#"{"type":"result","is_error":false,"result":"markdown"}"#).unwrap_err();
        assert!(e.to_string().contains("structured_output"), "{e}");
        let e = parse_output("# ただの Markdown").unwrap_err();
        assert!(e.to_string().contains("JSON"), "{e}");
        let e = parse_output(r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#).unwrap_err();
        assert!(e.to_string().contains("error_max_turns"), "{e}");
    }

    #[test]
    fn schema_violation_is_error() {
        // 未知の kind
        let bad = r#"{"structured_output":{"questions":[{"kind":"quiz","prompt":"x"}]}}"#;
        assert!(parse_output(bad).unwrap_err().to_string().contains("スキーマ"));
        // prompt 欠落
        let bad = r#"{"structured_output":{"questions":[{"kind":"free"}]}}"#;
        assert!(parse_output(bad).is_err());
        // 未知の尺度
        let bad = r#"{"structured_output":{"questions":[{"kind":"checkin","prompt":"x","scales":["joy"]}]}}"#;
        assert!(parse_output(bad).is_err());
        // 問いが空
        let bad = r#"{"structured_output":{"questions":[{"kind":"free","prompt":"  "}]}}"#;
        assert!(parse_output(bad).is_err());
        let bad = r#"{"structured_output":{"questions":[]}}"#;
        assert!(parse_output(bad).is_err());
    }

    #[test]
    fn normalize_defaults_and_caps() {
        let q = |p: &str| Question {
            kind: QuestionKind::Checkin,
            prompt: p.into(),
            scales: vec![],
            examples: vec![],
            activating_event: None,
            belief_prompt: None,
        };
        let o = StructuredOutput {
            summary: Some("  ".into()),
            questions: (0..12).map(|i| q(&format!("q{i}"))).collect(),
        }
        .normalize()
        .unwrap();
        assert_eq!(o.summary, None);
        assert_eq!(o.questions.len(), MAX_QUESTIONS);
        assert_eq!(o.questions[0].scales, Scale::ALL.to_vec());
    }

    #[test]
    fn markdown_rendering() {
        let o = parse_output(OUT).unwrap();
        let md = o.to_markdown();
        assert!(md.starts_with("## 要約\n\n要約"));
        assert!(md.contains("1. いまの調子は"));
        assert!(md.contains("調子（元気・気分）"));
        assert!(md.contains("出来事の候補: レビューで指摘"));
        assert!(md.contains("例: 会議が長い"));
        assert!(md.contains("4. ひとことで"));
    }

    #[test]
    fn auto_detects_claude() {
        let mut ai = Ai::default();
        assert!(enabled(&ai));
        for c in [
            r"C:\Users\x\bin\claude.exe",
            "/usr/local/bin/claude",
            "CLAUDE.CMD",
            "claude",
        ] {
            ai.command = cmd(&[c, "-p"]);
            assert!(enabled(&ai), "{c}");
        }
        for c in ["codex", "claude-wrapper", "my-claude", ""] {
            ai.command = cmd(&[c, "exec"]);
            assert!(!enabled(&ai), "{c}");
        }
        ai.command = vec![];
        assert!(!enabled(&ai));
        // on / off は command に関わらず強制
        ai.command = cmd(&["codex"]);
        ai.structured = AiStructured::On;
        assert!(enabled(&ai));
        ai.command = cmd(&["claude"]);
        ai.structured = AiStructured::Off;
        assert!(!enabled(&ai));
    }

    #[test]
    fn default_args_for_claude_embed_schema() {
        let ai = Ai::default();
        let (args, file) = build_args(&ai, "{\"a\":1}").unwrap();
        assert!(file.is_none());
        assert_eq!(
            args,
            cmd(&["--output-format", "json", "--json-schema", "{\"a\":1}"])
        );
        // claude 以外で structured_args が空なら何も付けない
        let ai = Ai {
            command: cmd(&["codex"]),
            structured: AiStructured::On,
            ..Ai::default()
        };
        assert!(build_args(&ai, "{}").unwrap().0.is_empty());
    }

    #[test]
    fn model_args_come_before_structured_args() {
        let ai = Ai {
            model: Some("sonnet".into()),
            ..Ai::default()
        };
        let mut argv = ai.argv();
        argv.extend(build_args(&ai, "{}").unwrap().0);
        assert_eq!(
            argv,
            cmd(&[
                "claude",
                "-p",
                "--model",
                "sonnet",
                "--output-format",
                "json",
                "--json-schema",
                "{}"
            ])
        );
    }

    #[test]
    fn custom_args_substitute_and_schema_file_is_removed() {
        let ai = Ai {
            command: cmd(&["codex", "exec"]),
            structured: AiStructured::On,
            structured_args: cmd(&["--output-schema", "{schema_file}", "--inline={schema}"]),
            ..Ai::default()
        };
        let path;
        {
            let (args, file) = build_args(&ai, "{\"x\":true}").unwrap();
            let file = file.expect("schema_file を使うので一時ファイルができる");
            path = file.path().to_path_buf();
            assert_eq!(args[0], "--output-schema");
            assert_eq!(args[1], path.to_string_lossy());
            assert_eq!(args[2], "--inline={\"x\":true}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"x\":true}");
        }
        assert!(!path.exists(), "破棄時に削除される");
    }
}
