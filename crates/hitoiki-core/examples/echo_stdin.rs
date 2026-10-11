//! テスト用の AI コマンド代替。標準入力をそのまま標準出力へ返す。
//! 引数 `fail` で終了コード 3、`sleep` で長時間待機する。
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
