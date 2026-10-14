//! テスト用の AI コマンド代替。標準入力をそのまま標準出力へ返す。
//! 引数 `fail` で終了コード 3、`sleep` で長時間待機する。`args` は後ろの引数を `args: ...` として返す。
//!
//! 構造化出力のテスト用に、追加の引数（構造化のときだけ付く）がある場合の挙動を切り替えられる。
//! - `structured`: 追加の引数があれば、claude 風の JSON を 2 つ連結して返す。無ければ Markdown を返す。
//! - `badjson`: 追加の引数があれば structured_output の無い JSON を返す。無ければ Markdown を返す。
//! - `badschema`: 追加の引数があればスキーマ違反の structured_output を返す。無ければ Markdown を返す。

use std::io::{Read, Write};

const QUESTIONS: &str = r#"{"summary":"今日の要約","questions":[
{"kind":"checkin","prompt":"いまの調子は？","scales":["energy","mood"],"erg_candidates":["growth"]},
{"kind":"hassle","prompt":"今日引っかかったことは？","examples":["会議が長い"]},
{"kind":"abc","prompt":"場面を選んでください","activating_event":"レビューで指摘された","belief_prompt":"何がよぎりましたか"},
{"kind":"free","prompt":"ひとことで言うと？"}]}"#;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().cloned().unwrap_or_default();
    let structured_call = args.len() > 1;
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).unwrap();
    let mut out = std::io::stdout();
    match mode.as_str() {
        "fail" => {
            eprintln!("失敗しました");
            std::process::exit(3);
        }
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(60)),
        // モードより後ろの引数を空白区切りで返す（モデル引数などの argv の検証用）。
        "args" => {
            write!(out, "args: {}", args[1..].join(" ")).unwrap();
            return;
        }
        // ERG 判定のプロンプト（`"id": "r1"` などを含む）には、r1 は growth（確信度 0.9）、
        // それ以外は relatedness（確信度 0.5）の JSON を返す。それ以外のプロンプトは空の判定を返す。
        "erg" => {
            let text = String::from_utf8_lossy(&buf);
            let ids: Vec<String> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("\"id\": \""))
                .filter_map(|l| l.split('"').next())
                .map(String::from)
                .collect();
            let labels: Vec<String> = ids
                .iter()
                .map(|id| {
                    let (tag, conf) = if id == "r1" {
                        ("growth", 0.9)
                    } else {
                        ("relatedness", 0.5)
                    };
                    format!(r#"{{"id":"{id}","tag":"{tag}","confidence":{conf},"reason":"テスト"}}"#)
                })
                .collect();
            write!(out, r#"{{"labels":[{}]}}"#, labels.join(",")).unwrap();
            return;
        }
        // ERG 判定のプロンプトのときだけ失敗する（振り返りの生成は成功する）。
        "ergfail" if String::from_utf8_lossy(&buf).contains("\"id\": \"r1\"") => {
            eprintln!("失敗しました");
            std::process::exit(3);
        }
        "structured" | "badjson" | "badschema" if structured_call => {
            let payload = match mode.as_str() {
                "structured" => format!(
                    r#"{{"type":"result","subtype":"success","is_error":false,"result":"x","structured_output":{QUESTIONS}}}"#
                ),
                "badjson" => r#"{"type":"result","subtype":"success","is_error":false,"result":"x"}"#.to_string(),
                _ => r#"{"type":"result","is_error":false,"structured_output":{"questions":[{"kind":"quiz","prompt":"x"}]}}"#
                    .to_string(),
            };
            writeln!(out, r#"{{"type":"system","subtype":"init"}}"#).unwrap();
            writeln!(out, "{payload}").unwrap();
            return;
        }
        "structured" | "badjson" | "badschema" => {
            out.write_all("## 問い\n\n1. 従来の Markdown の問い".as_bytes())
                .unwrap();
            return;
        }
        _ => {}
    }
    out.write_all(&buf).unwrap();
}
