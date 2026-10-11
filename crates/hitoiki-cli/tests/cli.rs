//! CLI の統合テスト。`HITOIKI_HOME` を一時ディレクトリに向けて実バイナリを起動する。

use hitoiki_core::{db, paths, timefmt};
use std::io::Write;
use std::process::{Command, Stdio};

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("JSON ではありません: {e}\n{}", self.stdout))
    }
}

fn run(home: &std::path::Path, args: &[&str]) -> Out {
    run_with_stdin(home, args, None)
}

fn run_with_stdin(home: &std::path::Path, args: &[&str], stdin: Option<&str>) -> Out {
    run_full(home, args, stdin.map(str::as_bytes), &[])
}

fn run_full(
    home: &std::path::Path,
    args: &[&str],
    stdin: Option<&[u8]>,
    envs: &[(&str, &std::path::Path)],
) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_hitoiki"));
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd
        .args(args)
        .env("HITOIKI_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut si = child.stdin.take().unwrap();
        if let Some(s) = stdin {
            si.write_all(s).unwrap();
        }
    }
    let o = child.wait_with_output().unwrap();
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn ms(s: &str) -> i64 {
    timefmt::parse_instant(chrono_tz::Asia::Tokyo, s, 0).unwrap()
}

/// HITOIKI_HOME を向けた状態で DB に区間を直接挿入する。
fn insert_segment(
    home: &std::path::Path,
    id: &str,
    start: &str,
    end: &str,
    kind: &str,
    task: Option<&str>,
    status: &str,
) {
    std::env::set_var("HITOIKI_HOME", home);
    let conn = db::open(&paths::db_file()).unwrap();
    let task_sql = task.map(|t| format!("'{t}'")).unwrap_or_else(|| "NULL".into());
    conn.execute(
        &format!(
            "INSERT INTO activity_segment (id, start_at, end_at, kind, task_id, status, window_span_ids, exe_name, title) \
             VALUES ('{id}', {}, {}, '{kind}', {task_sql}, '{status}', '[]', 'Code.exe', 'main.rs')",
            ms(start),
            ms(end)
        ),
        [],
    )
    .unwrap();
}

#[test]
fn task_lifecycle_with_json_and_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();

    let a = run(
        h,
        &[
            "--json",
            "task",
            "add",
            "設計レビュー",
            "--urgent",
            "--important",
            "--estimate",
            "1h30m",
        ],
    );
    assert_eq!(a.code, 0, "{}", a.stderr);
    let v = a.json();
    assert_eq!(v["version"], 1);
    assert_eq!(v["estimate_min"], 90);
    assert_eq!(v["urgent"], true);
    let id = v["id"].as_str().unwrap().to_string();

    let i = run(h, &["--json", "task", "add", "障害対応", "--interrupt"]);
    assert_eq!(i.json()["kind"], "interrupt");
    assert_eq!(i.json()["urgent"], true);

    // 前方一致で開始
    let s = run(h, &["--json", "task", "start", &id[..14]]);
    assert_eq!(s.code, 0, "{}", s.stderr);
    assert_eq!(s.json()["task"]["status"], "doing");

    let l = run(h, &["task", "list"]);
    assert_eq!(l.code, 0);
    assert!(l.stdout.contains("設計レビュー"), "{}", l.stdout);
    assert!(l.stdout.contains("障害対応"));

    let st = run(h, &["--json", "status"]);
    assert_eq!(st.json()["doing"][0]["task"]["title"], "設計レビュー");

    let d = run(h, &["--json", "task", "done", &id]);
    assert_eq!(d.json()["status"], "done");
    assert!(d.json()["done_at"].is_i64());

    // 終了コード
    assert_eq!(run(h, &["task", "show", "ZZZZZZ"]).code, 3);
    assert_eq!(run(h, &["task", "add", "x", "--estimate", "abc"]).code, 2);
    assert_eq!(run(h, &["task", "bogus"]).code, 2);
    assert_eq!(run(h, &["--json", "task", "add", "y", "--skill", "なし"]).code, 3);
}

#[test]
fn ambiguous_prefix_is_exit_4() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    run(h, &["task", "add", "a"]);
    run(h, &["task", "add", "b"]);
    // ULID は先頭が時刻なので 1 文字は共通になる
    let l = run(h, &["--json", "task", "list"]);
    let ids: Vec<String> = l.json()["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(run(h, &["task", "show", &ids[0][..1]]).code, 4);
}

#[test]
fn report_and_review_flow() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let t = run(
        h,
        &["--json", "task", "add", "実装", "--important", "--not-urgent"],
    );
    let tid = t.json()["id"].as_str().unwrap().to_string();

    insert_segment(
        h,
        "SEGA",
        "2026-01-05 10:00",
        "2026-01-05 11:00",
        "work",
        Some(&tid),
        "confirmed",
    );
    insert_segment(
        h,
        "SEGB",
        "2026-01-05 11:00",
        "2026-01-05 11:30",
        "work",
        None,
        "needs_review",
    );
    insert_segment(
        h,
        "SEGC",
        "2026-01-05 11:30",
        "2026-01-05 12:00",
        "idle",
        None,
        "auto",
    );

    let r = run(
        h,
        &[
            "--json",
            "report",
            "--from",
            "2026-01-05",
            "--to",
            "2026-01-05",
            "--by",
            "task",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["version"], 1);
    assert_eq!(v["total_ms"], 90 * 60_000);
    assert_eq!(v["excluded"]["idle_ms"], 30 * 60_000);
    assert_eq!(v["rows"][0]["label"], "実装");
    assert_eq!(v["rows"][1]["label"], "(なし)");

    let q = run(
        h,
        &["--json", "report", "--period", "2026-01-05", "--by", "quadrant"],
    );
    assert_eq!(q.json()["rows"][0]["label"], "重要（緊急でない）");

    let human = run(h, &["report", "--period", "2026-01-05", "--by", "kind"]);
    assert!(human.stdout.contains("作業"), "{}", human.stdout);
    assert!(human.stdout.contains("除外"));

    assert_eq!(run(h, &["report", "--by", "nope"]).code, 2);

    let rl = run(h, &["--json", "review", "list", "--date", "2026-01-05"]);
    assert_eq!(rl.json()["count"], 1);

    let set = run(h, &["--json", "review", "set", "SEGB", "--task", &tid, "--rule"]);
    assert_eq!(set.code, 0, "{}", set.stderr);
    let sv = set.json();
    assert_eq!(sv["segment"]["status"], "confirmed");
    assert_eq!(sv["rule"]["field"], "title");
    assert_eq!(sv["rule"]["pattern_type"], "contains");

    assert_eq!(
        run(h, &["--json", "review", "list", "--date", "2026-01-05"]).json()["count"],
        0
    );
    let rules = run(h, &["--json", "task", "rule", "list", &tid]);
    assert_eq!(rules.json()["rules"].as_array().unwrap().len(), 1);

    let lg = run(h, &["log", "--date", "2026-01-05", "--raw"]);
    assert_eq!(lg.code, 0, "{}", lg.stderr);
    assert!(lg.stdout.contains("SEGA"));
}

#[test]
fn records_and_planning() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    assert_eq!(run(h, &["check", "-e", "6"]).code, 2);
    let c = run(
        h,
        &[
            "--json", "check", "-e", "4", "-m", "3", "--erg", "growth", "--note", "ok",
        ],
    );
    assert_eq!(c.code, 0, "{}", c.stderr);
    assert_eq!(c.json()["erg_tag"], "growth");

    let hs = run(h, &["--json", "hassle", "会議が長い"]);
    assert_eq!(hs.json()["intensity"], 3);
    let hid = hs.json()["id"].as_str().unwrap().to_string();
    let abc = run(
        h,
        &["--json", "abc", "add", "--from-hassle", &hid, "--b", "無駄だ"],
    );
    assert_eq!(abc.code, 0, "{}", abc.stderr);
    assert_eq!(abc.json()["activating_event"], "会議が長い");

    let o = run(
        h,
        &[
            "--json",
            "okr",
            "add-objective",
            "Q4",
            "--from",
            "2026-10-01",
            "--to",
            "2026-12-31",
        ],
    );
    let oid = o.json()["id"].as_str().unwrap().to_string();
    assert_eq!(run(h, &["okr", "add-kr", &oid, "KR1", "--target", "80%"]).code, 0);
    let l = run(h, &["--json", "okr", "list"]);
    assert_eq!(l.json()["objectives"][0]["key_results"][0]["title"], "KR1");

    assert_eq!(run(h, &["skill", "import-preset"]).code, 0);
    let sk = run(h, &["--json", "skill", "list"]);
    assert!(sk.json()["skills"].as_array().unwrap().len() > 5);
    assert_eq!(run(h, &["task", "add", "t", "--skill", "Rust,設計"]).code, 0);

    let b = run(
        h,
        &[
            "--json", "block", "add", "09:00", "10:30", "--label", "集中", "--focus",
        ],
    );
    assert_eq!(b.code, 0, "{}", b.stderr);
    let bl = run(h, &["--json", "block", "list"]);
    assert_eq!(bl.json()["blocks"].as_array().unwrap().len(), 1);

    // 標準入力から本文
    let t = run_with_stdin(
        h,
        &["--json", "task", "add", "本文あり", "--body", "-"],
        Some("詳細\nです"),
    );
    assert_eq!(t.json()["body_md"], "詳細\nです");
}

#[test]
fn config_doctor_and_reflect() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    assert_eq!(run(h, &["config", "set", "task.single_doing", "false"]).code, 0);
    let g = run(h, &["config", "get", "task.single_doing"]);
    assert_eq!(g.stdout.trim(), "false");
    assert!(run(h, &["config", "path"]).stdout.contains("config.toml"));
    assert_eq!(run(h, &["config", "get", "no.such"]).code, 3);

    // doctor は診断結果を JSON で返す（終了コードは環境次第なので見ない）
    let d = run(h, &["--json", "doctor"]);
    assert!(
        d.json()["items"].as_array().is_some_and(|a| !a.is_empty()),
        "{}",
        d.stdout
    );
    assert_eq!(run(h, &["version"]).code, 0);

    // 振り返り系: context は JSON、存在しない ID は 3
    let c = run(h, &["context", "--kind", "day_start"]);
    assert_eq!(c.code, 0);
    assert!(c.stdout.contains("\"version\": 1"), "{}", c.stdout);
    assert_eq!(run(h, &["context", "--kind", "bogus"]).code, 2);
    assert_eq!(run(h, &["reflect", "list"]).code, 0);
    assert_eq!(run(h, &["abc", "dispute", "X"]).code, 3);
}

#[test]
fn structured_reflection_show_and_answer() {
    use hitoiki_core::model::{Reflection, ReflectionKind};
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    // AI は呼ばず、構造化された振り返りを直接保存しておく。
    let structured = r#"{"summary":"要約","questions":[
        {"kind":"checkin","prompt":"いまの調子は？","scales":["energy","mood"]},
        {"kind":"hassle","prompt":"引っかかったことは？"},
        {"kind":"abc","prompt":"場面を選んで","activating_event":"指摘された"},
        {"kind":"free","prompt":"ひとことで"}]}"#;
    let conn = db::open(&h.join("hitoiki.db")).unwrap();
    hitoiki_core::repo::reflection::insert(
        &conn,
        &Reflection {
            id: "01HZZZZZZZZZZZZZZZZZZZZZZZ".into(),
            at: 1_000,
            kind: ReflectionKind::DayEnd,
            context_json: "{}".into(),
            questions_md: "## 問い".into(),
            answers_md: None,
            summary_md: Some("要約".into()),
            structured_json: Some(structured.to_string()),
            answers_json: None,
        },
    )
    .unwrap();
    drop(conn);

    // 人間向けの表示: 種類ごとの問いと番号
    let s = run(h, &["reflect", "show", "01HZZZ"]);
    assert_eq!(s.code, 0, "{}", s.stderr);
    for want in [
        "[0] (チェックイン・未回答)",
        "[1] (ハッスル・未回答)",
        "A（出来事）の候補: 指摘された",
        "[3] (自由記述",
    ] {
        assert!(s.stdout.contains(want), "{want}\n{}", s.stdout);
    }
    // --json は構造をそのまま返す
    let j = run(h, &["--json", "reflect", "show", "01HZZZ"]).json();
    assert_eq!(j["structured"]["questions"].as_array().unwrap().len(), 4);
    assert_eq!(j["structured"]["questions"][2]["kind"], "abc");
    assert!(j["answers"].is_null());

    // 回答 JSON（標準入力）
    let ans = r#"{"answers":[
        {"index":0,"energy":3,"mood":4},
        {"index":1,"text":"会議が長い","intensity":4},
        {"index":2,"activating_event":"指摘された","belief":"向いていない","emotion":"落ち込み","emotion_intensity":3},
        {"index":3,"text":"まあまあ"}]}"#;
    let a = run_with_stdin(
        h,
        &["--json", "reflect", "answer", "01HZZZ", "--file", "-"],
        Some(ans),
    );
    assert_eq!(a.code, 0, "{}", a.stderr);
    let aj = a.json();
    assert_eq!(aj["answers"]["items"].as_array().unwrap().len(), 4);
    assert!(aj["answers_md"].as_str().unwrap().contains("まあまあ"));
    // 保存先の確認: checkin / hassle / abc_record にそれぞれ 1 件
    let conn = db::open(&h.join("hitoiki.db")).unwrap();
    let hs = hitoiki_core::repo::record::list_hassles(&conn, 0, i64::MAX).unwrap();
    assert_eq!(
        (hs.len(), hs[0].text.as_str(), hs[0].intensity),
        (1, "会議が長い", 4)
    );
    assert_eq!(
        hitoiki_core::repo::record::list_checkins(&conn, 0, i64::MAX)
            .unwrap()
            .len(),
        1
    );
    let abcs = hitoiki_core::repo::record::list_abc(&conn, 0, i64::MAX).unwrap();
    assert_eq!((abcs.len(), abcs[0].belief.as_deref()), (1, Some("向いていない")));
    drop(conn);
    // 範囲外の番号は引数エラー（終了コード 2）
    let bad = run_with_stdin(
        h,
        &["reflect", "answer", "01HZZZ", "--file", "-"],
        Some(r#"{"answers":[{"index":9,"text":"x"}]}"#),
    );
    assert_eq!(bad.code, 2, "{}", bad.stderr);
    // 従来の Markdown 形式も受け付ける（構造化された振り返りにも書ける）
    let md = run_with_stdin(
        h,
        &["reflect", "answer", "01HZZZ", "--file", "-"],
        Some("ふつうの Markdown の回答"),
    );
    assert_eq!(md.code, 0, "{}", md.stderr);
    let shown = run(h, &["--json", "reflect", "show", "01HZZZ"]).json();
    assert_eq!(shown["answers_md"], "ふつうの Markdown の回答");
}

#[test]
fn rebuild_on_empty_db() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let o = run(h, &["rebuild"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("再計算しました"), "{}", o.stdout);
    let o = run(h, &["--json", "rebuild", "--date", "2026-10-09"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j = o.json();
    assert_eq!(j["segments"], 0);
    assert_eq!(j["bindings"], 0);
    assert_eq!(j["version"], 1);
    assert!(j["range"]["start"].is_string());
    assert_eq!(
        run(h, &["rebuild", "--date", "today", "--from", "2026-01-01"]).code,
        2
    );
}

#[test]
fn ingest_context_cases() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let one = r#"{"source":"browser","hwnd":"0x1","kind":"url","value":"https://example.com"}"#;
    let o = run_with_stdin(h, &["--json", "ingest", "context"], Some(one));
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.json()["saved"], 1);
    let many = r#"[{"at":"2026-10-09T00:00:00Z","source":"vscode","kind":"workspace","value":"/w"},
                   {"at":1700000000000,"source":"vscode","kind":"file","value":"a.rs"}]"#;
    let o = run_with_stdin(h, &["--json", "ingest", "context"], Some(many));
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.json()["saved"], 2);
    let conn = db::open(&paths_db(h)).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM context_hint", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);

    for bad in [
        "not json",
        r#"{"source":"b","kind":"url"}"#,
        r#"[{"source":"b","kind":"url","value":"v"},{}]"#,
        "42",
    ] {
        let o = run_with_stdin(h, &["ingest", "context"], Some(bad));
        assert_eq!(o.code, 2, "{bad}: {}", o.stderr);
    }
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM context_hint", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);
}

fn paths_db(home: &std::path::Path) -> std::path::PathBuf {
    home.join("hitoiki.db")
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut v = (payload.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(payload);
    v
}

fn read_frames(mut b: &[u8]) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    while b.len() >= 4 {
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        out.push(serde_json::from_slice(&b[4..4 + n]).unwrap());
        b = &b[4 + n..];
    }
    out
}

#[test]
fn native_host_process() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let mut input = Vec::new();
    input.extend(frame(
        br#"{"source":"browser","kind":"url","value":"https://a.example"}"#,
    ));
    input.extend(frame(
        br#"[{"source":"browser","kind":"url","value":"u1"},{"source":"browser","kind":"url","value":"u2"}]"#,
    ));
    input.extend(frame(br#"{"source":"browser"}"#));
    // ブラウザが付ける起動引数でもホストとして動く。
    let o = run_full(
        h,
        &["native-host", "chrome-extension://abcdef/"],
        Some(&input),
        &[],
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let replies = read_frames(o.stdout.as_bytes());
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0], serde_json::json!({"ok": true, "saved": 1}));
    assert_eq!(replies[1], serde_json::json!({"ok": true, "saved": 2}));
    assert_eq!(replies[2]["ok"], false);
    let conn = db::open(&paths_db(h)).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM context_hint", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);

    // 1MB 超は拒否して終了する。
    let big = ((1024 * 1024 + 1) as u32).to_le_bytes();
    let o = run_full(h, &["native-host"], Some(&big), &[]);
    assert_ne!(o.code, 0);
    let replies = read_frames(o.stdout.as_bytes());
    assert_eq!(replies[0]["ok"], false);

    // 空入力は即終了。
    assert_eq!(run_full(h, &["native-host"], Some(b""), &[]).code, 0);
    assert_eq!(run(h, &["native-host", "bogus"]).code, 2);
    assert_eq!(run(h, &["native-host", "install"]).code, 2);
}

#[test]
fn agent_import_and_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path().join("home");
    std::fs::create_dir_all(&h).unwrap();
    let claude = dir.path().join("claude");
    let codex = dir.path().join("codex");
    let fx = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hitoiki-agents/tests/fixtures");
    let proj = claude.join("projects/enc");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::copy(
        fx.join("claude-code/session_basic.jsonl"),
        proj.join("session_basic.jsonl"),
    )
    .unwrap();
    let sess = codex.join("sessions/2026/01/01");
    std::fs::create_dir_all(&sess).unwrap();
    for e in std::fs::read_dir(fx.join("codex")).unwrap() {
        let p = e.unwrap().path();
        std::fs::copy(&p, sess.join(p.file_name().unwrap())).unwrap();
    }
    let envs: [(&str, &std::path::Path); 2] = [("CLAUDE_CONFIG_DIR", &claude), ("CODEX_HOME", &codex)];

    let o = run_full(
        &h,
        &["--json", "agent", "import", "--agent", "claude-code"],
        None,
        &envs,
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j = o.json();
    assert_eq!(j["files_scanned"], 1, "{j}");
    assert!(j["events_inserted"].as_i64().unwrap() > 0);

    let o = run_full(&h, &["--json", "agent", "import"], None, &envs);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.json()["files_scanned"].as_i64().unwrap() >= 2);
    assert_eq!(
        run_full(&h, &["agent", "import", "--agent", "x"], None, &envs).code,
        2
    );

    let o = run_full(
        &h,
        &["--json", "agent", "sessions", "--date", "2026-10-09"],
        None,
        &envs,
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j = o.json();
    let s = &j["sessions"][0];
    assert_eq!(s["agent"], "claude-code");
    assert_eq!(s["session_id"], "11111111-2222-4333-8444-555555555555");
    assert!(s["prompt_count"].as_i64().unwrap() >= 1);
    assert!(s["dialog_ms"].is_number());
    let o = run_full(&h, &["agent", "sessions", "--date", "2026-10-09"], None, &envs);
    assert!(o.stdout.contains("11111111"), "{}", o.stdout);
    let o = run_full(&h, &["agent", "sessions", "--date", "2020-01-01"], None, &envs);
    assert!(o.stdout.contains("セッションはありません"));
}

#[test]
fn review_list_hides_short_segments_unless_all() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    insert_segment(
        h,
        "LONGSEG",
        "2026-01-05 10:00",
        "2026-01-05 10:05",
        "work",
        None,
        "needs_review",
    );
    insert_segment(
        h,
        "SHORTSEG",
        "2026-01-05 10:05",
        "2026-01-05 10:05:30",
        "work",
        None,
        "needs_review",
    );

    // 既定: 短い区間は隠し、件数を示す
    let v = run(h, &["--json", "review", "list", "--date", "2026-01-05"]);
    assert_eq!(v.code, 0, "{}", v.stderr);
    let j = v.json();
    assert_eq!(j["count"], 1);
    assert_eq!(j["hidden_short"], 1);
    assert_eq!(j["min_sec"], 60);
    assert_eq!(j["version"], 1);
    let human = run(h, &["review", "list", "--date", "2026-01-05"]);
    assert!(
        human
            .stdout
            .contains("1分未満の短い区間が 1 件あります（--all で表示）"),
        "{}",
        human.stdout
    );

    // --all: 全件、隠した件数は 0
    let j = run(h, &["--json", "review", "list", "--date", "2026-01-05", "--all"]).json();
    assert_eq!(j["count"], 2);
    assert_eq!(j["hidden_short"], 0);

    // status の件数は既定の一覧と同じ数え方（短い区間は別に数える）
    let st = run(h, &["--json", "status"]).json();
    assert_eq!(st["needs_review_count"], 1);
    assert_eq!(st["needs_review_short_count"], 1);
}

#[test]
fn block_edit_delete_and_template_flow() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    assert_eq!(
        run(h, &["config", "set", "general.timezone", "Asia/Tokyo"]).code,
        0
    );

    // block edit / delete
    let b = run(
        h,
        &[
            "--json",
            "block",
            "add",
            "2026-10-14 09:00",
            "2026-10-14 10:00",
            "--label",
            "a",
        ],
    );
    assert_eq!(b.code, 0, "{}", b.stderr);
    let id = b.json()["id"].as_str().unwrap().to_string();
    let e = run(
        h,
        &[
            "--json",
            "block",
            "edit",
            &id[..8],
            "--end",
            "2026-10-14 11:00",
            "--focus",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(e.json()["focus"], true);
    assert_eq!(e.json()["label"], "a");
    assert_eq!(
        e.json()["end_at"].as_i64().unwrap() - e.json()["start_at"].as_i64().unwrap(),
        7_200_000
    );
    assert_eq!(run(h, &["block", "delete", &id]).code, 0);
    assert_eq!(run(h, &["block", "delete", &id]).code, 3);

    // 空のとき
    let l = run(h, &["--json", "block", "template", "list"]);
    assert_eq!(l.json()["templates"].as_array().unwrap().len(), 0);

    // add / show
    let a = run(
        h,
        &[
            "--json",
            "block",
            "template",
            "add",
            "水曜",
            "--weekdays",
            "wed",
            "--description",
            "定例の日",
            "--block",
            "10:00-11:00,label=定例",
            "--block",
            "14:00-15:00,focus,label=集中",
        ],
    );
    assert_eq!(a.code, 0, "{}", a.stderr);
    assert_eq!(a.json()["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(a.json()["weekdays"][0], 3);
    // 引数エラー: 終了コード 2
    assert_eq!(
        run(
            h,
            &["block", "template", "add", "x", "--block", "11:00-10:00,label=a"]
        )
        .code,
        2
    );
    assert_eq!(run(h, &["block", "template", "add", "水曜"]).code, 2);
    assert_eq!(run(h, &["block", "template", "show", "ない"]).code, 3);
    let s = run(h, &["block", "template", "show", "水曜"]);
    assert!(s.stdout.contains("10:00-11:00 定例"), "{}", s.stdout);

    // suggest（2026-10-14 は水曜）
    let g = run(
        h,
        &["--json", "block", "template", "suggest", "--date", "2026-10-14"],
    );
    assert_eq!(g.json()["templates"][0]["name"], "水曜");
    assert_eq!(g.json()["weekday"], 3);

    // apply --dry-run は何も作らない
    let d = run(
        h,
        &[
            "--json",
            "block",
            "template",
            "apply",
            "水曜",
            "--date",
            "2026-10-14",
            "--dry-run",
        ],
    );
    assert_eq!(d.json()["blocks"].as_array().unwrap().len(), 2);
    let l = run(h, &["--json", "block", "list", "--date", "2026-10-14"]);
    assert_eq!(l.json()["blocks"].as_array().unwrap().len(), 0);

    // apply: 10:00 JST = 01:00 UTC
    let r = run(
        h,
        &[
            "--json",
            "block",
            "template",
            "apply",
            "水曜",
            "--date",
            "2026-10-14",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.json()["created"].as_array().unwrap().len(), 2);
    assert_eq!(r.json()["created"][0]["start_at"], ms("2026-10-14 10:00"));
    // 2 回目は重なるのでスキップ
    let r2 = run(
        h,
        &[
            "--json",
            "block",
            "template",
            "apply",
            "水曜",
            "--date",
            "2026-10-14",
        ],
    );
    assert_eq!(r2.json()["created"].as_array().unwrap().len(), 0);
    assert_eq!(r2.json()["skipped"].as_array().unwrap().len(), 2);
    assert_eq!(
        run(h, &["block", "template", "apply", "水曜", "--overlap", "bogus"]).code,
        2
    );
    // 直前の適用（2 回目、作成 0 件）を取り消しても 1 回目のブロックは残る
    let u = run(h, &["--json", "block", "template", "undo"]);
    assert_eq!(u.json()["removed"], 0);
    let u = run(
        h,
        &["--json", "block", "template", "undo", "--date", "2026-10-14"],
    );
    assert_eq!(u.json()["removed"], 2);
    assert_eq!(run(h, &["block", "template", "undo"]).code, 3);

    // save-from
    assert_eq!(
        run(h, &["block", "template", "apply", "水曜", "--date", "2026-10-15"]).code,
        0
    );
    let sf = run(
        h,
        &[
            "--json",
            "block",
            "template",
            "save-from",
            "--date",
            "2026-10-15",
            "--name",
            "コピー",
            "--weekdays",
            "thu",
        ],
    );
    assert_eq!(sf.code, 0, "{}", sf.stderr);
    assert_eq!(sf.json()["blocks"][1]["start"], "14:00");
    assert_eq!(sf.json()["blocks"][1]["focus"], true);
    assert_eq!(
        run(
            h,
            &[
                "block",
                "template",
                "save-from",
                "--date",
                "2026-10-20",
                "--name",
                "空"
            ]
        )
        .code,
        2
    );

    // edit（名前変更・曜日の解除・ブロック置き換え）/ delete
    let ed = run(
        h,
        &[
            "--json",
            "block",
            "template",
            "edit",
            "コピー",
            "--new-name",
            "朝型",
            "--weekdays",
            "",
            "--block",
            "07:00-08:00,label=読書",
        ],
    );
    assert_eq!(ed.code, 0, "{}", ed.stderr);
    assert_eq!(ed.json()["name"], "朝型");
    assert_eq!(ed.json()["weekdays"].as_array().unwrap().len(), 0);
    assert_eq!(ed.json()["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(run(h, &["block", "template", "delete", "朝型"]).code, 0);
    assert_eq!(run(h, &["block", "template", "delete", "朝型"]).code, 3);
    assert_eq!(
        run(h, &["--json", "block", "template", "list"]).json()["templates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn review_set_by_range_dry_run_apply_list_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let t = run(h, &["--json", "task", "add", "補正先"]);
    let tid = t.json()["id"].as_str().unwrap().to_string();
    let seg = |id: &str, s: &str, e: &str, kind: &str, status: &str| {
        insert_segment(
            h,
            id,
            &format!("2026-01-05 {s}"),
            &format!("2026-01-05 {e}"),
            kind,
            None,
            status,
        )
    };
    seg("WORKSEG", "10:00", "10:30", "work", "needs_review");
    seg("CONFSEG", "10:30", "10:40", "work", "confirmed");
    seg("IDLESEG", "10:40", "10:50", "idle", "needs_review");
    let range = ["--from", "2026-01-05 10:10", "--to", "2026-01-05 10:45"];
    let mut base = vec!["--json", "review", "set"];
    base.extend(range);

    // --dry-run: 変更せず、対象と合計を返す。境界にまたがる区間は範囲の内側だけ（10:10-10:30 の 20 分）。
    let mut args = base.clone();
    args.extend(["--task", &tid, "--dry-run"]);
    let o = run(h, &args);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j = o.json();
    assert_eq!(j["selected_count"], 1);
    assert_eq!(j["total_ms"], 20 * 60_000);
    assert_eq!(j["excluded_count"], 2);
    let cands = j["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 3);
    assert_eq!(cands[0]["duration_ms"], 20 * 60_000);
    assert_eq!(cands[0]["full_duration_ms"], 30 * 60_000);
    assert_eq!(cands[2]["category"], "idle");
    assert_eq!(cands[2]["checked"], false);
    assert!(run(h, &["--json", "review", "corrections"]).json()["corrections"]
        .as_array()
        .unwrap()
        .is_empty());
    // 人間向けの出力にも確定前である旨と合計が出る。
    let mut human = vec!["review", "set"];
    human.extend(range);
    human.extend(["--task", &tid, "--dry-run"]);
    let h_out = run(h, &human);
    assert!(h_out.stdout.contains("20分"), "{}", h_out.stdout);
    assert!(h_out.stdout.contains("確定していません"), "{}", h_out.stdout);

    // 適用（無操作も PC 外の作業として含める）。
    let mut args = base.clone();
    args.extend(["--task", &tid, "--include-idle"]);
    let o = run(h, &args);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let j = o.json();
    assert_eq!(j["preview"]["total_ms"], 25 * 60_000);
    assert_eq!(j["preview"]["offline_ms"], 5 * 60_000);
    let cid = j["correction"]["id"].as_str().unwrap().to_string();

    let l = run(h, &["--json", "review", "corrections", "--task", &tid[..14]]).json();
    assert_eq!(l["count"], 1);
    assert_eq!(l["corrections"][0]["id"], cid.as_str());
    assert_eq!(l["corrections"][0]["offline_ms"], 5 * 60_000);
    assert_eq!(
        run(h, &["--json", "review", "corrections", "--date", "2026-01-06"]).json()["count"],
        0
    );
    let log = run(h, &["--json", "log", "--date", "2026-01-05"]).json();
    let segs = log["segments"].as_array().unwrap();
    let idle = segs
        .iter()
        .find(|s| s["start_at"] == ms("2026-01-05 10:40"))
        .unwrap();
    assert_eq!(
        (idle["kind"].as_str(), idle["offline"].as_bool()),
        (Some("work"), Some(true))
    );

    // 取り消し。
    let u = run(h, &["--json", "review", "undo-correction", &cid[..12]]);
    assert_eq!(u.code, 0, "{}", u.stderr);
    assert_eq!(u.json()["overrides_removed"], 2);
    assert_eq!(run(h, &["--json", "review", "corrections"]).json()["count"], 0);
    assert_eq!(run(h, &["review", "undo-correction", &cid]).code, 3);
}

#[test]
fn review_set_range_argument_errors() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let t = run(h, &["--json", "task", "add", "x"]);
    let tid = t.json()["id"].as_str().unwrap().to_string();
    insert_segment(
        h,
        "WORKSEG",
        "2026-01-05 10:00",
        "2026-01-05 10:30",
        "work",
        None,
        "needs_review",
    );
    // 区間 ID と範囲は併用できない / --from には --to が必要 / 割り当て先が無い。
    let both = [
        "review", "set", "WORKSEG", "--from", "10:00", "--to", "11:00", "--task", &tid,
    ];
    assert_eq!(run(h, &both).code, 2);
    assert_eq!(
        run(h, &["review", "set", "--from", "10:00", "--task", &tid]).code,
        2
    );
    let no_target = [
        "review",
        "set",
        "--from",
        "2026-01-05 10:00",
        "--to",
        "2026-01-05 11:00",
    ];
    assert_eq!(run(h, &no_target).code, 2);
    // 存在しない除外 ID は対象なし(3)。
    let bad_exclude = [
        "review",
        "set",
        "--from",
        "2026-01-05 10:00",
        "--to",
        "2026-01-05 11:00",
        "--task",
        &tid,
        "--exclude",
        "ZZZZ",
    ];
    assert_eq!(run(h, &bad_exclude).code, 3);
}

#[test]
fn task_delete_force_dry_run_and_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let tid = run(h, &["--json", "task", "add", "誤入力"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    insert_segment(
        h,
        "SEG1",
        "2026-01-05 10:00",
        "2026-01-05 10:30",
        "work",
        Some(&tid),
        "auto",
    );

    // --dry-run は影響の表示だけ（何も消えない）。
    let d = run(h, &["--json", "task", "delete", &tid, "--dry-run"]);
    assert_eq!(d.code, 0, "{}", d.stderr);
    let j = d.json();
    assert_eq!(j["segments"], 1);
    assert_eq!(j["tracked_ms"], 30 * 60_000);
    assert_eq!(j["needs_force"], true);
    let human = run(h, &["task", "delete", &tid, "--dry-run"]);
    assert!(human.stdout.contains("実行はしていません"), "{}", human.stdout);
    assert_eq!(run(h, &["task", "show", &tid]).code, 0);

    // 計上済みがあると --force なしは終了コード 2 で、影響を表示する。
    let e = run(h, &["task", "delete", &tid]);
    assert_eq!(e.code, 2);
    assert!(e.stderr.contains("--force"), "{}", e.stderr);
    assert!(e.stderr.contains("30分"), "{}", e.stderr);
    assert_eq!(run(h, &["task", "show", &tid]).code, 0);

    let o = run(h, &["--json", "task", "delete", &tid, "--force"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.json()["deleted"]["segments"], 1);
    assert_eq!(run(h, &["task", "show", &tid]).code, 3);
    assert_eq!(run(h, &["task", "delete", &tid, "--force"]).code, 3);

    // 記録の無いタスクは --force なしで消せる。
    let t2 = run(h, &["--json", "task", "add", "空"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let o = run(h, &["task", "delete", &t2]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("削除しました"), "{}", o.stdout);
}

#[test]
fn task_switch_と_back() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let a = run(h, &["--json", "task", "add", "設計レビュー"]);
    let a_id = a.json()["id"].as_str().unwrap().to_string();
    assert_eq!(run(h, &["task", "start", &a_id]).code, 0);

    // 戻り先がまだ無い。
    assert_eq!(run(h, &["task", "back"]).code, 3);

    // 割り込みタスクを作って開始する。
    let s = run(
        h,
        &["--json", "task", "switch", "障害対応", "--new", "--interrupt"],
    );
    assert_eq!(s.code, 0, "{}", s.stderr);
    let v = s.json();
    assert_eq!(v["created"], true);
    assert_eq!(v["task"]["status"], "doing");
    assert_eq!(v["task"]["kind"], "interrupt");
    assert_eq!(v["task"]["urgent"], true);
    assert_eq!(v["demoted"][0]["id"], a_id.as_str());

    // 直前の doing（設計レビュー）に戻る。
    let b = run(h, &["--json", "task", "back"]);
    assert_eq!(b.code, 0, "{}", b.stderr);
    assert_eq!(b.json()["task"]["id"], a_id.as_str());
    assert_eq!(b.json()["created"], false);
    let st = run(h, &["--json", "status"]);
    assert_eq!(st.json()["doing"][0]["task"]["title"], "設計レビュー");

    // 既存タスクをタイトルの一部で選ぶ。
    let t = run(h, &["task", "switch", "障害"]);
    assert_eq!(t.code, 0, "{}", t.stderr);
    assert!(t.stdout.contains("開始しました"), "{}", t.stdout);

    // 終了コード: 対象なし 3、曖昧 4、引数エラー 2。
    assert_eq!(run(h, &["task", "switch", "存在しない"]).code, 3);
    assert_eq!(run(h, &["task", "add", "障害対応の振り返り"]).code, 0);
    assert_eq!(run(h, &["task", "switch", "障害"]).code, 4);
    assert_eq!(run(h, &["task", "switch", "  ", "--new"]).code, 2);
    assert_eq!(run(h, &["task", "switch", "x", "--interrupt"]).code, 2);
}
