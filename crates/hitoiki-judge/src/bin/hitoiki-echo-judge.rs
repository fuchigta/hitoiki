//! 外部コマンド型の自動判定のテスト用・サンプル実装。
//! 使い方: `hitoiki-echo-judge [ok|sleep|bad|fail|short]`
//! - ok: 各質問に最初の選択肢 / false / 最小値で confidence 0.9 の回答を返す
//! - sleep: 応答せず 30 秒眠る（タイムアウト試験）
//! - bad: 不正な JSON を返す
//! - fail: 非 0 で終了する
//! - short: バッチ入力に対し results を 1 件少なく返す（長さ不一致の試験）
//!
//! 入力に `states` 配列があればバッチ応答（`results`）を返す。
//! バッチ時は順序確認用に、bool の回答を「states 内の添字が奇数なら true」にする。

use serde_json::{json, Value};
use std::io::Read;

fn answers_for(req: &Value, index: Option<usize>) -> Value {
    let mut answers = serde_json::Map::new();
    for q in req["questions"].as_array().into_iter().flatten() {
        let value = match q["type"].as_str() {
            Some("choice") => q["options"][0]["id"].clone(),
            Some("score") => q["scale"][0].clone(),
            _ => json!(index.is_some_and(|i| i % 2 == 1)),
        };
        answers.insert(
            q["name"].as_str().unwrap_or_default().to_string(),
            json!({"value": value, "confidence": 0.9}),
        );
    }
    Value::Object(answers)
}

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
            if let Some(states) = req["states"].as_array() {
                let mut n = states.len();
                if mode == "short" {
                    n = n.saturating_sub(1);
                }
                let results: Vec<Value> = (0..n)
                    .map(|i| json!({"answers": answers_for(&req, Some(i))}))
                    .collect();
                println!("{}", json!({"version": 1, "results": results}));
            } else {
                println!("{}", json!({"version": 1, "answers": answers_for(&req, None)}));
            }
        }
    }
}
