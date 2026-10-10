//! 外部コマンド判定器のテスト用・サンプル実装。
//! 使い方: `hitoiki-echo-judge [ok|sleep|bad|fail]`
//! - ok: 各質問に最初の選択肢 / false / 最小値で confidence 0.9 の回答を返す
//! - sleep: 応答せず 30 秒眠る（タイムアウト試験）
//! - bad: 不正な JSON を返す
//! - fail: 非 0 で終了する

use serde_json::{json, Value};
use std::io::Read;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "ok".into());
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    match mode.as_str() {
        "sleep" => std::thread::sleep(std::time::Duration::from_secs(30)),
        "bad" => println!("this is not json"),
        "fail" => std::process::exit(3),
        _ => {
            let req: Value = serde_json::from_str(&input).expect("入力が JSON ではありません");
            let mut answers = serde_json::Map::new();
            for q in req["questions"].as_array().into_iter().flatten() {
                let value = match q["type"].as_str() {
                    Some("choice") => q["options"][0]["id"].clone(),
                    Some("score") => q["scale"][0].clone(),
                    _ => json!(false),
                };
                answers.insert(
                    q["name"].as_str().unwrap_or_default().to_string(),
                    json!({"value": value, "confidence": 0.9}),
                );
            }
            println!("{}", json!({"version": 1, "answers": answers}));
        }
    }
}
