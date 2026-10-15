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
fn key_result_の現在値の記録と履歴() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let o = run(h, &["--json", "goal", "add", "目標"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let k = run(h, &["--json", "key-result", "add", &o, "成果", "--target", "10"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let p = run(
        h,
        &["--json", "key-result", "progress", &k, "3", "--note", "初回"],
    );
    assert_eq!(p.code, 0, "{}", p.stderr);
    assert_eq!(p.json()["entry"]["value"], 3.0);
    assert_eq!(p.json()["key_result"]["current"], "3");
    // edit-key_result --current でも履歴が残る（同じ値は残さない）
    run(h, &["key-result", "edit", &k, "--current", "5", "--note", "追加"]);
    run(h, &["key-result", "edit", &k, "--current", "5"]);
    assert_eq!(run(h, &["key-result", "progress", &k, " "]).code, 2);
    assert_eq!(run(h, &["key-result", "progress", "ZZZ", "1"]).code, 3);
    assert_eq!(run(h, &["key-result", "edit", &k, "--note", "x"]).code, 2);

    let j = run(h, &["--json", "key-result", "history", &k]).json();
    let entries = j["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["note"], "追加");
    assert_eq!(j["weeks"].as_array().unwrap().len(), 8);
    let text = run(h, &["key-result", "history", &k]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(text.stdout.contains("初回"), "{}", text.stdout);

    // 成果の指標を消すと履歴も消える
    assert_eq!(run(h, &["key-result", "delete", &k]).code, 0);
    assert_eq!(run(h, &["key-result", "history", &k]).code, 3);
}

#[test]
fn goal_show_は目標と配下の成果の指標を出す() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let o = run(h, &["--json", "goal", "add", "読書"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(run(h, &["key-result", "add", &o, "週 3 回読む"]).code, 0);
    let s = run(h, &["--json", "goal", "show", &o[..12]]);
    assert_eq!(s.code, 0, "{}", s.stderr);
    assert_eq!(s.json()["title"], "読書");
    assert_eq!(s.json()["key_results"][0]["title"], "週 3 回読む");
    let human = run(h, &["goal", "show", &o[..12]]);
    assert!(human.stdout.contains("週 3 回読む"), "{}", human.stdout);
    assert_eq!(run(h, &["goal", "show", "ZZZ"]).code, 3);
}

#[test]
fn 編集と削除_force_dry_run_終了コード() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let id_of = |o: &Out| o.json()["id"].as_str().unwrap().to_string();

    // 目標: 成果の指標を編集し、紐付いたタスクがあると --force なしでは消せない
    let o = id_of(&run(h, &["--json", "goal", "add", "目標"]));
    let k = id_of(&run(
        h,
        &["--json", "key-result", "add", &o, "成果", "--target", "10"],
    ));
    let t = run(h, &["--json", "task", "add", "作業", "--key-result", &k]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let e = run(
        h,
        &[
            "--json",
            "key-result",
            "edit",
            &k,
            "--title",
            "成果2",
            "--current",
            "3",
            "--status",
            "achieved",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(e.json()["current"], "3");
    assert_eq!(run(h, &["key-result", "edit", &k, "--status", "bad"]).code, 2);
    assert_eq!(run(h, &["key-result", "edit", "ZZZ", "--title", "x"]).code, 3);

    let d = run(h, &["key-result", "delete", &k]);
    assert_eq!(d.code, 2);
    assert!(d.stderr.contains("--force"), "{}", d.stderr);
    let d = run(h, &["--json", "key-result", "delete", &k, "--dry-run"]);
    assert_eq!(d.code, 0);
    assert_eq!(d.json()["deleted"], false);
    assert_eq!(d.json()["impact"]["lines"][0]["count"], 1);
    let d = run(h, &["--json", "key-result", "delete", &k, "--force"]);
    assert_eq!(d.code, 0, "{}", d.stderr);
    assert_eq!(d.json()["deleted"], true);
    let shown = run(h, &["--json", "task", "show", &t]).json();
    assert!(shown["task"]["key_result"].is_null());
    // 影響のない Goal は --force なしで消える
    assert_eq!(run(h, &["goal", "delete", &o]).code, 0);
    assert_eq!(run(h, &["goal", "delete", &o]).code, 3);

    // スキル: 子の付け替えと削除
    run(h, &["skill", "add", "親"]);
    run(h, &["skill", "add", "子", "--parent", "親"]);
    let e = run(
        h,
        &[
            "--json",
            "skill",
            "edit",
            "子",
            "--name",
            "子2",
            "--description",
            "メモ",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(run(h, &["skill", "edit", "親", "--parent", "子2"]).code, 2);
    let d = run(
        h,
        &[
            "--json",
            "skill",
            "delete",
            "親",
            "--children",
            "reparent",
            "--dry-run",
        ],
    );
    assert_eq!(d.json()["deleted"], false);
    assert_eq!(
        run(h, &["skill", "delete", "親", "--children", "reparent"]).code,
        2
    );
    assert_eq!(run(h, &["skill", "delete", "親", "--force"]).code, 0);
    let l = run(h, &["--json", "skill", "list"]).json();
    assert_eq!(l["skills"].as_array().unwrap().len(), 1);
    assert!(l["skills"][0]["parent_id"].is_null());
    assert_eq!(
        run(h, &["skill", "delete", "子2", "--children", "x", "--force"]).code,
        2
    );

    // ルール: 無効化・編集・削除
    let r = run(
        h,
        &[
            "--json",
            "task",
            "rule",
            "add",
            &t,
            "--field",
            "exe",
            "--pattern",
            "code*",
        ],
    );
    let rid = id_of(&r);
    let e = run(
        h,
        &[
            "--json",
            "task",
            "rule",
            "edit",
            &rid,
            "--disable",
            "--priority",
            "3",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(e.json()["enabled"], false);
    assert_eq!(
        run(
            h,
            &["task", "rule", "edit", &rid, "--type", "regex", "--pattern", "("]
        )
        .code,
        2
    );
    assert_eq!(run(h, &["task", "rule", "delete", &rid, "--dry-run"]).code, 0);
    assert_eq!(run(h, &["task", "rule", "delete", &rid]).code, 0);
    assert_eq!(run(h, &["task", "rule", "delete", &rid]).code, 3);
    let a = id_of(&run(
        h,
        &[
            "--json",
            "activity-rule",
            "add",
            "--field",
            "exe",
            "--pattern",
            "slack*",
            "--label",
            "会議",
        ],
    ));
    assert_eq!(
        run(h, &["activity-rule", "edit", &a, "--label", "連絡", "--disable"]).code,
        0
    );
    let l = run(h, &["--json", "activity-rule", "list"]).json();
    assert_eq!(l["rules"][0]["activity"], "連絡");
    assert_eq!(l["rules"][0]["enabled"], false);
    assert_eq!(run(h, &["activity-rule", "delete", &a]).code, 0);

    // 記録: ハッスルを消すと ABC の紐付けだけ外れる
    let hs = id_of(&run(h, &["--json", "hassle", "会議が長い", "-i", "4"]));
    let abc = id_of(&run(
        h,
        &["--json", "abc", "add", "--from-hassle", &hs, "--b", "無駄"],
    ));
    let e = run(h, &["--json", "hassle", "edit", &hs, "-i", "2"]);
    assert_eq!(e.json()["intensity"], 2);
    assert_eq!(run(h, &["hassle", "delete", &hs]).code, 2);
    assert_eq!(run(h, &["hassle", "delete", &hs, "--force"]).code, 0);
    let l = run(h, &["--json", "abc", "list"]).json();
    assert!(l["items"][0]["hassle_id"].is_null());
    let e = run(
        h,
        &[
            "--json",
            "abc",
            "edit",
            &abc,
            "--a",
            "A2",
            "--b",
            "",
            "--c-emotion",
            "不安",
            "--c-intensity",
            "3",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(e.json()["activating_event"], "A2");
    assert!(e.json()["belief"].is_null());
    assert_eq!(run(h, &["abc", "delete", &abc]).code, 0);
    let c = id_of(&run(h, &["--json", "check", "-e", "2", "-m", "3"]));
    let e = run(
        h,
        &[
            "--json", "check", "edit", &c, "-e", "5", "-m", "0", "--note", "元気",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(
        (e.json()["energy"].as_i64(), e.json()["mood"].is_null()),
        (Some(5), true)
    );
    assert_eq!(run(h, &["check", "edit", &c, "-e", "9"]).code, 2);
    assert_eq!(run(h, &["check", "delete", &c]).code, 0);
    assert_eq!(run(h, &["check", "delete", &c]).code, 3);
    // 引数なしのハッスルは引数エラー
    assert_eq!(run(h, &["hassle"]).code, 2);

    // カレンダーソース・活動: URL は出力に出ない
    let a = run(
        h,
        &[
            "--json",
            "calendar",
            "add",
            "仕事",
            "https://cal.example.com/token-secret/a.ics",
        ],
    );
    assert_eq!(a.code, 0, "{}", a.stderr);
    assert!(!a.stdout.contains("token-secret"));
    let l = run(h, &["calendar", "list"]);
    assert!(l.stdout.contains("cal.example.com") && !l.stdout.contains("token-secret"));
    assert_eq!(
        run(h, &["calendar", "add", "仕事", "https://x.example/b.ics"]).code,
        2
    );
    assert_eq!(run(h, &["calendar", "delete", "仕事", "--dry-run"]).code, 0);
    assert_eq!(run(h, &["calendar", "delete", "仕事"]).code, 0);
    assert_eq!(run(h, &["calendar", "delete", "仕事"]).code, 3);
    assert_eq!(run(h, &["activity", "add", "運動"]).code, 0);
    assert_eq!(run(h, &["activity", "rename", "運動", "トレーニング"]).code, 0);
    assert!(run(h, &["activity", "list"]).stdout.contains("トレーニング"));
    assert_eq!(run(h, &["activity", "remove", "トレーニング"]).code, 0);
    assert_eq!(run(h, &["activity", "remove", "トレーニング"]).code, 3);
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
    assert_eq!(v["version"], 2);
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
    assert_eq!(v["version"], 2);
    assert_eq!(v["total_ms"], 90 * 60_000);
    assert_eq!(v["excluded"]["idle_ms"], 30 * 60_000);
    assert_eq!(v["rows"][0]["label"], "実装");
    assert_eq!(v["rows"][1]["label"], "(なし)");

    let q = run(
        h,
        &[
            "--json",
            "report",
            "--period",
            "2026-01-05",
            "--by",
            "urgent_important",
        ],
    );
    assert_eq!(q.json()["rows"][0]["label"], "重要（緊急でない）");

    let human = run(h, &["report", "--period", "2026-01-05", "--by", "kind"]);
    assert!(human.stdout.contains("作業"), "{}", human.stdout);
    assert!(human.stdout.contains("除外"));

    assert_eq!(run(h, &["report", "--by", "nope"]).code, 2);

    let rl = run(h, &["--json", "actual", "review", "--date", "2026-01-05"]);
    assert_eq!(rl.json()["count"], 1);

    let set = run(
        h,
        &["--json", "actual", "confirm", "SEGB", "--task", &tid, "--rule"],
    );
    assert_eq!(set.code, 0, "{}", set.stderr);
    let sv = set.json();
    assert_eq!(sv["segment"]["status"], "confirmed");
    assert_eq!(sv["rule"]["field"], "title");
    assert_eq!(sv["rule"]["pattern_type"], "contains");

    assert_eq!(
        run(h, &["--json", "actual", "review", "--date", "2026-01-05"]).json()["count"],
        0
    );
    let rules = run(h, &["--json", "task", "rule", "list", &tid]);
    assert_eq!(rules.json()["rules"].as_array().unwrap().len(), 1);

    let lg = run(h, &["actual", "list", "--date", "2026-01-05", "--raw"]);
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
            "goal",
            "add",
            "Q4",
            "--from",
            "2026-10-01",
            "--to",
            "2026-12-31",
        ],
    );
    let oid = o.json()["id"].as_str().unwrap().to_string();
    assert_eq!(
        run(h, &["key-result", "add", &oid, "KR1", "--target", "80%"]).code,
        0
    );
    let l = run(h, &["--json", "goal", "list"]);
    assert_eq!(l.json()["goals"][0]["key_results"][0]["title"], "KR1");

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
    assert!(c.stdout.contains("\"version\": 2"), "{}", c.stdout);
    assert_eq!(run(h, &["context", "--kind", "bogus"]).code, 2);
    assert_eq!(run(h, &["reflect", "list"]).code, 0);
    assert_eq!(run(h, &["abc", "dispute", "X"]).code, 3);
}

#[test]
fn erg_判定_確認待ち_確定() {
    use hitoiki_core::model::{ErgChoice, ErgLabel, ErgRecordKind, ErgSource};
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    // AI が未設定なら、エラーにせず判定しない
    std::fs::write(h.join("config.toml"), "[ai]\ncommand = []\n").unwrap();
    let id = run(h, &["--json", "hassle", "新しいことを学んだ"]).json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let c = run(h, &["--json", "erg", "classify"]);
    assert_eq!(c.code, 0, "{}", c.stderr);
    assert_eq!(c.json()["skipped"], "AI が設定されていないため判定しません");
    let c = run(h, &["erg", "classify", "--dry-run"]);
    assert_eq!(c.code, 0);
    assert!(
        c.stdout.contains("AI が設定されていないため判定しません"),
        "{}",
        c.stdout
    );
    assert_eq!(
        run(h, &["--json", "erg", "review"]).json()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    // 確認待ちの提案を直接用意する（AI は呼ばない）
    let conn = db::open(&h.join("hitoiki.db")).unwrap();
    hitoiki_core::repo::erg::set_auto(
        &conn,
        &ErgLabel {
            record_kind: ErgRecordKind::Hassle,
            record_id: id.clone(),
            tag: ErgChoice::Growth,
            source: ErgSource::Suggested,
            confidence: Some(0.5),
            reason: Some("学びに触れている".into()),
            at: 1,
        },
    )
    .unwrap();
    drop(conn);
    let r = run(h, &["--json", "erg", "review"]).json();
    assert_eq!(r["items"][0]["tag"], "growth");
    assert_eq!(r["items"][0]["source"], "suggested");
    assert_eq!(r["items"][0]["reason"], "学びに触れている");
    assert!(run(h, &["erg", "review"]).stdout.contains("成長"));
    let s = run(h, &["--json", "erg", "show"]).json();
    assert_eq!(s["distribution"]["pending"], 1);
    assert_eq!(s["pending_total"], 1);

    // そのまま確定 → user。確認待ちではなくなる
    let c = run(h, &["--json", "erg", "confirm", &id[..14]]);
    assert_eq!(c.code, 0, "{}", c.stderr);
    assert_eq!(c.json()["source"], "user");
    assert_eq!(
        run(h, &["--json", "erg", "review"]).json()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        run(h, &["--json", "erg", "show"]).json()["distribution"]["growth"],
        1
    );
    // 確認待ちでないものは confirm できない（2）。変えるのは set
    assert_eq!(run(h, &["erg", "confirm", &id]).code, 2);
    let s = run(h, &["--json", "erg", "set", &id, "none"]);
    assert_eq!((s.code, s.json()["tag"].as_str()), (0, Some("none")));
    assert_eq!(
        run(h, &["--json", "erg", "show"]).json()["distribution"]["none"],
        1
    );
    // 終了コード: 不正なタグ 2 / 対象なし 3
    assert_eq!(run(h, &["erg", "set", &id, "power"]).code, 2);
    assert_eq!(run(h, &["erg", "set", "ZZZZZZ", "growth"]).code, 3);
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
        "[0] (調子・未回答)",
        "[1] (気になったこと・未回答)",
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
    assert_eq!(j["version"], 2);
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
    let v = run(h, &["--json", "actual", "review", "--date", "2026-01-05"]);
    assert_eq!(v.code, 0, "{}", v.stderr);
    let j = v.json();
    assert_eq!(j["count"], 1);
    assert_eq!(j["hidden_short"], 1);
    assert_eq!(j["min_sec"], 60);
    assert_eq!(j["version"], 2);
    let human = run(h, &["actual", "review", "--date", "2026-01-05"]);
    assert!(
        human
            .stdout
            .contains("1分未満の短い区間が 1 件あります（--all で表示）"),
        "{}",
        human.stdout
    );

    // --all: 全件、隠した件数は 0
    let j = run(
        h,
        &["--json", "actual", "review", "--date", "2026-01-05", "--all"],
    )
    .json();
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
    let mut base = vec!["--json", "actual", "correct"];
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
    assert!(run(h, &["--json", "actual", "corrections"]).json()["corrections"]
        .as_array()
        .unwrap()
        .is_empty());
    // 人間向けの出力にも確定前である旨と合計が出る。
    let mut human = vec!["actual", "correct"];
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
    assert_eq!(j["preview"]["off_pc_ms"], 5 * 60_000);
    let cid = j["correction"]["id"].as_str().unwrap().to_string();

    let l = run(h, &["--json", "actual", "corrections", "--task", &tid[..14]]).json();
    assert_eq!(l["count"], 1);
    assert_eq!(l["corrections"][0]["id"], cid.as_str());
    assert_eq!(l["corrections"][0]["off_pc_ms"], 5 * 60_000);
    assert_eq!(
        run(h, &["--json", "actual", "corrections", "--date", "2026-01-06"]).json()["count"],
        0
    );
    let log = run(h, &["--json", "actual", "list", "--date", "2026-01-05"]).json();
    let segs = log["segments"].as_array().unwrap();
    let idle = segs
        .iter()
        .find(|s| s["start_at"] == ms("2026-01-05 10:40"))
        .unwrap();
    assert_eq!(
        (idle["kind"].as_str(), idle["off_pc"].as_bool()),
        (Some("work"), Some(true))
    );

    // 取り消し。
    let u = run(h, &["--json", "actual", "undo-correction", &cid[..12]]);
    assert_eq!(u.code, 0, "{}", u.stderr);
    assert_eq!(u.json()["confirmations_removed"], 2);
    assert_eq!(run(h, &["--json", "actual", "corrections"]).json()["count"], 0);
    assert_eq!(run(h, &["actual", "undo-correction", &cid]).code, 3);
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
        "actual", "correct", "WORKSEG", "--from", "10:00", "--to", "11:00", "--task", &tid,
    ];
    assert_eq!(run(h, &both).code, 2);
    assert_eq!(
        run(h, &["actual", "correct", "--from", "10:00", "--task", &tid]).code,
        2
    );
    let no_target = [
        "actual",
        "correct",
        "--from",
        "2026-01-05 10:00",
        "--to",
        "2026-01-05 11:00",
    ];
    assert_eq!(run(h, &no_target).code, 2);
    // 存在しない除外 ID は対象なし(3)。
    let bad_exclude = [
        "actual",
        "correct",
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

#[test]
fn ignore_list_add_remove_suggest_and_segments() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    // 既定の一覧に explorer.exe がある
    let o = run(h, &["ignore", "list", "--json"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let v = o.json();
    assert_eq!(v["customized"], false);
    assert!(v["exes"].as_array().unwrap().iter().any(|e| e == "explorer.exe"));

    // 追加は既定＋追加の形で設定ファイルに書かれ、二重追加は何もしない
    let o = run(h, &["ignore", "add", "Noisy.exe", "--json"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.json()["changed"], true);
    let v = run(h, &["ignore", "list", "--json"]).json();
    assert_eq!(v["customized"], true);
    assert!(v["exes"].as_array().unwrap().iter().any(|e| e == "explorer.exe"));
    assert!(v["exes"].as_array().unwrap().iter().any(|e| e == "Noisy.exe"));
    assert_eq!(
        run(h, &["ignore", "add", "noisy.exe", "--json"]).json()["changed"],
        false
    );

    // 外す。無いものは終了コード 3、空の exe は 2
    assert_eq!(run(h, &["ignore", "remove", "noisy.exe"]).code, 0);
    assert_eq!(run(h, &["ignore", "remove", "noisy.exe"]).code, 3);
    assert_eq!(run(h, &["ignore", "add", " "]).code, 2);

    // 提案は空でも成功し、断る・引数エラーも規約どおり
    let o = run(h, &["ignore", "suggest", "--json"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.json()["data"].as_array().unwrap().is_empty());
    assert_eq!(run(h, &["ignore", "suggest", "--dismiss", "x.exe"]).code, 0);
    assert_eq!(run(h, &["ignore", "suggest", "--days", "0"]).code, 2);

    // 区間の無視。対象なしは 3
    insert_segment(
        h,
        "IGN1",
        "2026-10-10T10:00:00+09:00",
        "2026-10-10T10:30:00+09:00",
        "work",
        None,
        "needs_review",
    );
    let o = run(h, &["actual", "ignore", "IGN1", "--json"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.json()["segments"][0]["kind"], "ignored");
    assert_eq!(run(h, &["actual", "ignore", "NOPE"]).code, 3);
    // 無視でない区間の取り消しは引数エラー
    insert_segment(
        h,
        "WRK1",
        "2026-10-10T11:00:00+09:00",
        "2026-10-10T11:30:00+09:00",
        "work",
        None,
        "needs_review",
    );
    assert_eq!(run(h, &["actual", "unignore", "WRK1"]).code, 2);
    // 取り消し（確定を削除して再計算）
    assert_eq!(run(h, &["actual", "unignore", "IGN1"]).code, 0);
}

#[test]
fn nudge_list_and_answer() {
    use hitoiki_core::nudge::{store, NudgeDecision, NudgePayload};
    let mk = |trigger: &str, title: &str| -> NudgePayload {
        serde_json::from_value(serde_json::json!({
            "trigger": trigger, "title": title, "body": "本文",
            "actions": [{"id": "energy", "label": "3", "value": 3}],
            "input": null, "open_screen": null
        }))
        .unwrap()
    };
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    std::env::set_var("HITOIKI_HOME", h);
    let conn = db::open(&paths::db_file()).unwrap();
    let now = hitoiki_core::Clock::now_ms(&hitoiki_core::SystemClock);
    let a = store::record_decision(
        &conn,
        &NudgeDecision::Deliver(mk("day_start", "おはようございます")),
        now,
    )
    .unwrap()
    .unwrap();
    let b = store::record_decision(
        &conn,
        &NudgeDecision::Deliver(mk("after_interrupt", "割り込み")),
        // 同じ時刻だと並び（at, id）が ULID の乱数部で決まるので 1ms ずらす。
        now + 1,
    )
    .unwrap()
    .unwrap();
    drop(conn);

    let l = run(h, &["--json", "nudge", "list", "--unanswered"]);
    assert_eq!(l.code, 0, "{}", l.stderr);
    assert_eq!(l.json()["nudges"].as_array().unwrap().len(), 2);
    assert_eq!(l.json()["nudges"][0]["trigger_label"], "始業");
    let human = run(h, &["nudge", "list"]);
    assert!(human.stdout.contains("おはようございます"), "{}", human.stdout);
    assert!(run(h, &["status"]).stdout.contains("[始業] おはようございます"));

    let ans = run(
        h,
        &["--json", "nudge", "answer", &a, "--energy", "4", "--mood", "3"],
    );
    assert_eq!(ans.code, 0, "{}", ans.stderr);
    assert_eq!(ans.json()["state"], "answered");
    let dis = run(h, &["--json", "nudge", "answer", &b, "--dismiss"]);
    assert_eq!(dis.json()["state"], "dismissed");
    let l = run(h, &["--json", "nudge", "list", "--unanswered"]);
    assert!(l.json()["nudges"].as_array().unwrap().is_empty());
    let all = run(h, &["--json", "nudge", "list"]);
    assert_eq!(all.json()["nudges"].as_array().unwrap().len(), 2);

    // 終了コード: 引数不正 2、対象なし 3。
    assert_eq!(run(h, &["nudge", "answer", &a]).code, 2);
    assert_eq!(run(h, &["nudge", "answer", &a, "--energy", "9"]).code, 2);
    assert_eq!(run(h, &["nudge", "answer", &a, "--idle", "x"]).code, 2);
    assert_eq!(run(h, &["nudge", "answer", "ZZZZ", "--dismiss"]).code, 3);
}

#[test]
fn nudge_key_result_idle_の見送りに答える() {
    use hitoiki_core::nudge::{store, NudgeDecision, NudgePayload};
    let p: NudgePayload = serde_json::from_value(serde_json::json!({
        "trigger": "key_result_idle", "title": "今週、時間が入っていない成果の指標があります",
        "body": "「成果」に、今週はまだ時間が入っていません。",
        "actions": [{"id": "open", "label": "今日の予定に入れる", "value": null},
                    {"id": "key_result_skip_week", "label": "今週は見送る", "value": null}],
        "input": null, "open_screen": "today/design"
    }))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    std::env::set_var("HITOIKI_HOME", h);
    let conn = db::open(&paths::db_file()).unwrap();
    let now = hitoiki_core::Clock::now_ms(&hitoiki_core::SystemClock);
    let id = store::record_decision(&conn, &NudgeDecision::Deliver(p), now)
        .unwrap()
        .unwrap();
    drop(conn);
    let l = run(h, &["--json", "nudge", "list", "--unanswered"]);
    assert_eq!(
        l.json()["nudges"][0]["trigger_label"],
        "時間が入っていない成果の指標"
    );
    let a = run(
        h,
        &["--json", "nudge", "answer", &id, "--task", "key_result_skip_week"],
    );
    assert_eq!(a.code, 0, "{}", a.stderr);
    assert_eq!(a.json()["state"], "answered");
    assert_eq!(run(h, &["nudge", "answer", &id, "--task", "nope"]).code, 2);
}

#[test]
fn link_suggestion_候補を出し_付ける_付けないを選ぶ() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let id_of = |o: &Out| o.json()["id"].as_str().unwrap().to_string();
    let o = id_of(&run(h, &["--json", "goal", "add", "英語力を上げる"]));
    let k = id_of(&run(
        h,
        &["--json", "key-result", "add", &o, "英語の勉強を続ける"],
    ));
    let t1 = id_of(&run(h, &["--json", "task", "add", "英語の教材を読む"]));
    let t2 = id_of(&run(h, &["--json", "task", "add", "英語の教材を買う"]));
    run(h, &["--json", "task", "add", "経費精算"]);
    assert_eq!(run(h, &["skill", "add", "英語"]).code, 0);

    let s = run(h, &["--json", "key-result", "suggest", &k, "--no-ai"]);
    assert_eq!(s.code, 0, "{}", s.stderr);
    let j = s.json();
    assert_eq!(j["eligible"], true);
    assert_eq!(j["ai"]["used"], false);
    let cands = j["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 3, "{j}");
    assert!(cands.iter().all(|c| c["name"] != "経費精算"));
    let human = run(h, &["key-result", "suggest", &k, "--no-ai"]);
    assert!(human.stdout.contains("英語の教材を読む"), "{}", human.stdout);
    assert!(human.stdout.contains("自動では紐付けません"), "{}", human.stdout);

    // 付けない: 以後は出ない。
    let r = run(
        h,
        &[
            "--json",
            "key-result",
            "suggest-reject",
            &k,
            &format!("task:{t2}"),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let j = run(h, &["--json", "key-result", "suggest", &k, "--no-ai"]).json();
    assert_eq!(j["candidates"].as_array().unwrap().len(), 2);
    assert!(j["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["id"] != t2.as_str()));

    // 付ける: タスクは紐付き、スキルは成果の指標に加わる。以後は提案の対象外。
    let a = run(
        h,
        &[
            "--json",
            "key-result",
            "suggest-accept",
            &k,
            &format!("task:{t1}"),
            "skill:英語",
        ],
    );
    assert_eq!(a.code, 0, "{}", a.stderr);
    assert_eq!(a.json()["items"].as_array().unwrap().len(), 2);
    let j = run(h, &["--json", "key-result", "suggest", &k, "--no-ai"]).json();
    assert_eq!(j["eligible"], false);
    let shown = run(h, &["--json", "key-result", "show", &k]).json();
    assert_eq!(shown["key_result_skills"][0], "英語");
    assert_eq!(shown["task_count"], 1);

    // 終了コード: 候補の書式が不正 2、成果の指標 / 候補が見つからない 3。
    assert_eq!(run(h, &["key-result", "suggest-accept", &k, "foo"]).code, 2);
    assert_eq!(
        run(h, &["key-result", "suggest-accept", &k, "skill:存在しない"]).code,
        3
    );
    assert_eq!(run(h, &["key-result", "suggest", "ZZZ", "--no-ai"]).code, 3);
}

#[test]
fn heatmap_日別の濃淡と終了コード() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let t = run(h, &["--json", "task", "add", "実装"]);
    let tid = t.json()["id"].as_str().unwrap().to_string();
    insert_segment(
        h,
        "H1",
        "2026-01-05 10:00",
        "2026-01-05 11:00",
        "work",
        Some(&tid),
        "confirmed",
    );
    insert_segment(
        h,
        "H2",
        "2026-01-06 10:00",
        "2026-01-06 12:00",
        "work",
        None,
        "confirmed",
    );
    insert_segment(
        h,
        "H3",
        "2026-01-06 12:00",
        "2026-01-06 13:00",
        "idle",
        None,
        "auto",
    );

    let r = run(
        h,
        &["--json", "heatmap", "--from", "2026-01-05", "--to", "2026-01-11"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = r.json();
    assert_eq!(v["version"], 2);
    assert_eq!(v["by"], "time");
    assert_eq!(v["days"].as_array().unwrap().len(), 7);
    assert_eq!(v["days"][0]["value"], 60 * 60_000);
    assert_eq!(v["days"][1]["value"], 120 * 60_000);
    assert_eq!(v["days"][1]["level"], 4);
    assert_eq!(v["total"], 180 * 60_000);
    assert_eq!(v["active_days"], 2);

    let tk = run(
        h,
        &[
            "--json",
            "heatmap",
            "--by",
            "task:実装",
            "--from",
            "2026-01-05",
            "--to",
            "2026-01-11",
        ],
    );
    assert_eq!(tk.json()["total"], 60 * 60_000);

    let human = run(h, &["heatmap", "--from", "2026-01-05", "--to", "2026-01-11"]);
    assert_eq!(human.code, 0, "{}", human.stderr);
    assert!(human.stdout.contains('█'), "{}", human.stdout);
    assert!(human.stdout.contains("値のあった日: 2 日"), "{}", human.stdout);

    assert_eq!(run(h, &["heatmap", "--by", "nope"]).code, 2);
    assert_eq!(run(h, &["heatmap", "--by", "task:存在しないタスク"]).code, 3);
    assert_eq!(
        run(h, &["heatmap", "--from", "2026-02-01", "--to", "2026-01-01"]).code,
        2
    );
}

#[test]
fn key_result_link_unlink_show_と数え方の編集() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let id_of = |o: &Out| o.json()["id"].as_str().unwrap().to_string();
    let o = id_of(&run(
        h,
        &[
            "--json",
            "goal",
            "add",
            "読書",
            "--from",
            "2026-01-01",
            "--to",
            "2026-01-31",
        ],
    ));
    let k1 = id_of(&run(
        h,
        &[
            "--json",
            "key-result",
            "add",
            &o,
            "週 3 回読書",
            "--target",
            "週 3 回",
        ],
    ));
    let k2 = id_of(&run(h, &["--json", "key-result", "add", &o, "別の成果"]));
    let t1 = id_of(&run(h, &["--json", "task", "add", "本を読む"]));
    let t2 = id_of(&run(
        h,
        &["--json", "task", "add", "記事を読む", "--key-result", &k2],
    ));

    // 紐付け（付け替えは previous_key_result に出る）
    let l = run(h, &["--json", "key-result", "link", &k1, &t1, &t2]);
    assert_eq!(l.code, 0, "{}", l.stderr);
    let v = l.json();
    assert_eq!(v["linked"].as_array().unwrap().len(), 2);
    assert!(v["linked"][0]["previous_key_result"].is_null());
    assert_eq!(v["linked"][1]["previous_key_result"]["id"], k2.as_str());
    let human = run(h, &["key-result", "link", &k1, &t1]);
    assert!(human.stdout.contains("紐付けました"), "{}", human.stdout);
    assert_eq!(run(h, &["key-result", "link", "ZZZZ", &t1]).code, 3);
    assert_eq!(run(h, &["key-result", "link", &k1, "ZZZZ"]).code, 3);
    assert_eq!(run(h, &["key-result", "link", &k1]).code, 2);

    // 数え方の編集
    let e = run(
        h,
        &[
            "--json",
            "key-result",
            "edit",
            &k1,
            "--measure",
            "active_days",
            "--active-day-min",
            "20",
        ],
    );
    assert_eq!(e.code, 0, "{}", e.stderr);
    assert_eq!(e.json()["measure"], "active_days");
    assert_eq!(e.json()["active_day_min"], 20);
    assert_eq!(run(h, &["key-result", "edit", &k1, "--measure", "bad"]).code, 2);
    assert_eq!(
        run(h, &["key-result", "edit", &k1, "--active-day-min", "0"]).code,
        2
    );

    // 集計: 25 分の日と 15 分の日（active_day_min 20 なら 1 日だけ）
    insert_segment(
        h,
        "K1",
        "2026-01-05 10:00",
        "2026-01-05 10:25",
        "work",
        Some(&t1),
        "confirmed",
    );
    insert_segment(
        h,
        "K2",
        "2026-01-06 10:00",
        "2026-01-06 10:15",
        "work",
        Some(&t2),
        "confirmed",
    );
    insert_segment(
        h,
        "K3",
        "2026-01-06 11:00",
        "2026-01-06 12:00",
        "idle",
        Some(&t2),
        "auto",
    );
    let s = run(h, &["--json", "key-result", "show", &k1]);
    assert_eq!(s.code, 0, "{}", s.stderr);
    let v = s.json();
    assert_eq!(v["measure"], "active_days");
    assert_eq!(v["range_source"], "goal");
    assert_eq!(v["time_ms"], 40 * 60_000);
    assert_eq!(v["active_days"], 1);
    assert_eq!(v["task_count"], 2);
    assert_eq!(v["key_result"]["target"], "週 3 回");
    let human = run(h, &["key-result", "show", &k1]);
    assert_eq!(human.code, 0, "{}", human.stderr);
    assert!(
        human.stdout.contains("数え方（取り組んだ日数）"),
        "{}",
        human.stdout
    );
    // 期間の指定
    let r = run(
        h,
        &[
            "--json",
            "key-result",
            "show",
            &k1,
            "--from",
            "2026-01-06",
            "--to",
            "2026-01-06",
        ],
    );
    assert_eq!(r.json()["time_ms"], 15 * 60_000);
    assert_eq!(r.json()["range_source"], "given");
    assert_eq!(run(h, &["key-result", "show", "ZZZZ"]).code, 3);

    // heatmap の key_result の集計の軸
    let hm = run(
        h,
        &[
            "--json",
            "heatmap",
            "--by",
            &format!("key_result:{k1}"),
            "--from",
            "2026-01-05",
            "--to",
            "2026-01-11",
        ],
    );
    assert_eq!(hm.code, 0, "{}", hm.stderr);
    assert_eq!(hm.json()["total"], 40 * 60_000);

    // 外す（成果の指標に紐付いていないものは 0 件）
    let u = run(h, &["--json", "key-result", "unlink", &t1]);
    assert_eq!(u.json()["unlinked"].as_array().unwrap().len(), 1);
    let u = run(h, &["--json", "key-result", "unlink", &t1]);
    assert_eq!(u.code, 0);
    assert_eq!(u.json()["unlinked"].as_array().unwrap().len(), 0);
    assert_eq!(
        run(h, &["--json", "key-result", "show", &k1]).json()["time_ms"],
        15 * 60_000
    );

    // task edit --no-key-result
    let ne = run(h, &["--json", "task", "edit", &t2, "--no-key-result"]);
    assert_eq!(ne.code, 0, "{}", ne.stderr);
    assert!(run(h, &["--json", "task", "show", &t2]).json()["task"]["key_result"].is_null());
    assert_eq!(
        run(h, &["task", "edit", &t2, "--key-result", &k1, "--no-key-result"]).code,
        2
    );

    // 成果の指標のスキル: 紐付いていないタスク（t2、15 分）でも、スキルが付いていれば成果の指標の時間に入る
    assert_eq!(run(h, &["skill", "add", "Rust"]).code, 0);
    assert_eq!(run(h, &["task", "edit", &t2, "--skill", "Rust"]).code, 0);
    assert_eq!(run(h, &["key-result", "edit", &k1, "--skill", "ZZZ"]).code, 3);
    let e = run(h, &["--json", "key-result", "edit", &k1, "--skill", "Rust"]);
    assert_eq!(e.code, 0, "{}", e.stderr);
    let v = run(h, &["--json", "key-result", "show", &k1]).json();
    assert_eq!(v["key_result_skills"][0], "Rust");
    assert_eq!(v["time_ms"], 15 * 60_000);
    assert_eq!(v["from_skill_ms"], 15 * 60_000);
    assert_eq!(v["from_task_ms"], 0);
    let human = run(h, &["key-result", "show", &k1]);
    assert!(human.stdout.contains("内訳"), "{}", human.stdout);
    let e = run(h, &["--json", "key-result", "edit", &k1, "--no-skill"]);
    assert_eq!(e.code, 0, "{}", e.stderr);
    let v = run(h, &["--json", "key-result", "show", &k1]).json();
    assert_eq!(v["time_ms"], 0);
    assert!(v["key_result_skills"].as_array().unwrap().is_empty());
}
