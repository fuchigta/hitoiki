use hitoiki_judge::*;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn request() -> JudgeRequest {
    let q = vec![
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
            name: "is_interrupt".into(),
            kind: QuestionType::Bool,
            description: String::new(),
            options: None,
            allow_none: false,
            scale: None,
        },
    ];
    JudgeRequest::single(
        State {
            segment: SegmentState {
                start: "2026-01-01T00:00:00+09:00".into(),
                end: "2026-01-01T00:05:00+09:00".into(),
                exe: Some("a.exe".into()),
                title: None,
                cwd: None,
            },
            context: Default::default(),
        },
        q,
    )
}

fn ext(mode: &str, timeout: Duration) -> ExternalCommandJudge {
    ExternalCommandJudge::new(
        "echo",
        vec![env!("CARGO_BIN_EXE_hitoiki-echo-judge").into(), mode.into()],
        timeout,
    )
}

#[test]
fn external_ok() {
    let r = ext("ok", Duration::from_secs(10)).ask(&request()).unwrap();
    let a = r.answers.unwrap();
    assert_eq!(a["task"].value, json!("T1"));
    assert_eq!(a["is_interrupt"].value, json!(false));
    assert!((a["task"].confidence - 0.9).abs() < 1e-9);
}

#[test]
fn external_timeout_kills() {
    let t = Instant::now();
    let e = ext("sleep", Duration::from_millis(300)).ask(&request());
    assert!(e.is_err());
    assert!(t.elapsed() < Duration::from_secs(10));
}

#[test]
fn external_bad_json_and_exit_code() {
    assert!(ext("bad", Duration::from_secs(10)).ask(&request()).is_err());
    assert!(ext("fail", Duration::from_secs(10)).ask(&request()).is_err());
}

#[test]
fn external_missing_command() {
    let j = ExternalCommandJudge::new(
        "x",
        vec!["hitoiki-no-such-command-xyz".into()],
        Duration::from_secs(1),
    );
    assert!(j.ask(&request()).is_err());
    let j = ExternalCommandJudge::new("x", vec![], Duration::from_secs(1));
    assert!(j.ask(&request()).is_err());
}

#[test]
fn external_batch_default_one_by_one() {
    let req = request();
    let states = vec![req.state.clone().unwrap(), req.state.clone().unwrap()];
    let r = ext("ok", Duration::from_secs(10)).ask_batch(&states, &req.questions);
    assert_eq!(r.len(), 2);
    assert!(r.iter().all(|a| a.is_some()));
}

/// 擬似 HTTP サーバ。各接続でリクエストを読み、`responder(リクエスト本文)` の (status, body) を返す。
type ReqLog = Arc<Mutex<Vec<(String, Value)>>>;

fn mock_server(responder: impl Fn(usize, &Value) -> (u16, String) + Send + 'static) -> (String, ReqLog) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: ReqLog = Arc::default();
    let log2 = log.clone();
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let mut s = stream.unwrap();
            let mut buf = vec![];
            let mut tmp = [0u8; 4096];
            let (head_end, len) = loop {
                let k = s.read(&mut tmp).unwrap();
                if k == 0 {
                    return;
                }
                buf.extend_from_slice(&tmp[..k]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|v| v.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    break (p + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let k = s.read(&mut tmp).unwrap();
                buf.extend_from_slice(&tmp[..k]);
            }
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let body: Value = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap();
            let (status, resp) = responder(n, &body);
            log2.lock().unwrap().push((head, body));
            let out = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                resp.len()
            );
            let _ = s.write_all(out.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), log)
}

fn chat(content: &str) -> String {
    json!({"choices": [{"message": {"content": content}}]}).to_string()
}

#[test]
fn openai_structured_output() {
    let (url, log) = mock_server(|_, _| {
        (
            200,
            chat(
                r#"{"task":{"value":"T1","confidence":2.5},"is_interrupt":{"value":true,"confidence":0.4}}"#,
            ),
        )
    });
    std::env::set_var("HITOIKI_TEST_KEY_A", "secret");
    let j = OpenAiCompatibleJudge::new(
        "aigw",
        &url,
        "m",
        Some("HITOIKI_TEST_KEY_A".into()),
        Duration::from_secs(5),
    );
    let a = j.ask(&request()).unwrap().answers.unwrap();
    assert_eq!(a["task"].confidence, 1.0);
    assert_eq!(a["is_interrupt"].value, json!(true));
    let log = log.lock().unwrap();
    assert!(log[0].0.starts_with("POST /v1/chat/completions"));
    assert!(log[0].0.to_lowercase().contains("authorization: bearer secret"));
    assert_eq!(log[0].1["response_format"]["type"], "json_schema");
    assert_eq!(log[0].1["model"], "m");
}

#[test]
fn openai_fallback_to_json_object() {
    let (url, log) = mock_server(|_, body| {
        if body["response_format"]["type"] == "json_schema" {
            (400, r#"{"error":"unsupported"}"#.into())
        } else {
            (200, chat(r#"{"is_interrupt":{"value":false,"confidence":0.8}}"#))
        }
    });
    let j = OpenAiCompatibleJudge::new("aigw", &url, "m", None, Duration::from_secs(5));
    let a = j.ask(&request()).unwrap().answers.unwrap();
    assert_eq!(a["is_interrupt"].value, json!(false));
    assert_eq!(log.lock().unwrap().len(), 2);
    // 2 回目以降は最初から json_object
    j.ask(&request()).unwrap();
    let log = log.lock().unwrap();
    assert_eq!(log.len(), 3);
    assert_eq!(log[2].1["response_format"]["type"], "json_object");
}

#[test]
fn openai_errors() {
    let (url, _) = mock_server(|_, _| (500, "{}".into()));
    let j = OpenAiCompatibleJudge::new("aigw", &url, "m", None, Duration::from_secs(5));
    assert!(j.ask(&request()).is_err());

    let (url, _) = mock_server(|_, _| (200, chat("garbage")));
    let j = OpenAiCompatibleJudge::new("aigw", &url, "m", None, Duration::from_secs(5));
    assert!(j.ask(&request()).is_err());

    // API キーの環境変数が無い
    let j = OpenAiCompatibleJudge::new(
        "aigw",
        "http://127.0.0.1:9/v1",
        "m",
        Some("HITOIKI_TEST_KEY_UNSET".into()),
        Duration::from_secs(1),
    );
    assert!(j.ask(&request()).is_err());
}

#[test]
fn openai_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        // 受け付けるが応答しない
        let _held: Vec<_> = listener.incoming().take(1).collect();
        std::thread::sleep(Duration::from_secs(5));
    });
    let j = OpenAiCompatibleJudge::new(
        "aigw",
        &format!("http://127.0.0.1:{port}/v1"),
        "m",
        None,
        Duration::from_millis(300),
    );
    let t = Instant::now();
    assert!(j.ask(&request()).is_err());
    assert!(t.elapsed() < Duration::from_secs(4));
}
