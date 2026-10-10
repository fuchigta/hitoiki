use hitoiki_agents::{import_with, AgentAdapter, ClaudeCodeAdapter, CodexAdapter, Importer};
use hitoiki_core::db;
use hitoiki_core::model::AgentEventKind as K;
use hitoiki_core::repo::agent as repo;
use hitoiki_core::timefmt::Range;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn kinds(evs: &[hitoiki_core::model::AgentEvent]) -> Vec<K> {
    evs.iter().map(|e| e.kind).collect()
}

#[test]
fn claude_edge_cases() {
    let a = ClaudeCodeAdapter::with_root(fixtures().join("claude-code"), 20);
    let f = fixtures().join("claude-code/session_edge.jsonl");
    let (evs, off) = a.parse_incremental(&f, 0).unwrap();
    assert_eq!(off, std::fs::metadata(&f).unwrap().len());
    assert_eq!(
        kinds(&evs),
        [
            K::SessionStart,
            K::PromptSubmit,
            K::ResponseComplete,
            K::PromptSubmit,
            K::PromptSubmit,
            K::ResponseComplete
        ]
    );
    // tool_result / sidechain / isMeta / コマンド出力 / task-notification は prompt にならない
    assert_eq!(
        evs[1].prompt_excerpt.as_deref(),
        Some("最初の依頼です。 2行目の内容がここに入")
    );
    assert_eq!(evs[3].prompt_excerpt.as_deref(), Some("/review dummy args"));
    assert_eq!(evs[4].prompt_excerpt.as_deref(), Some("画像付きの依頼"));
    // 連続する end_turn 行は最後の 1 件に
    assert_eq!(evs[2].at - evs[0].at, 5000);
    assert_eq!(evs[1].git_branch.as_deref(), Some("feature/x"));
    assert!(evs[1].cwd.as_deref().unwrap().ends_with("proj"));
    assert_eq!(evs[0].session_id, "aaaaaaaa-0000-4000-8000-000000000001");
}

#[test]
fn claude_excerpt_disabled() {
    let a = ClaudeCodeAdapter::with_root(fixtures().join("claude-code"), 0);
    let (evs, _) = a
        .parse_incremental(&fixtures().join("claude-code/session_edge.jsonl"), 0)
        .unwrap();
    assert!(evs.iter().all(|e| e.prompt_excerpt.is_none()));
}

#[test]
fn claude_real_structure_fixture() {
    let a = ClaudeCodeAdapter::with_root(fixtures().join("claude-code"), 50);
    let (evs, _) = a
        .parse_incremental(&fixtures().join("claude-code/session_basic.jsonl"), 0)
        .unwrap();
    let prompts = evs.iter().filter(|e| e.kind == K::PromptSubmit).count();
    let done = evs.iter().filter(|e| e.kind == K::ResponseComplete).count();
    // 実ログ由来: tool_result 11 件は prompt に数えない
    assert!((1..=3).contains(&prompts), "prompts = {prompts}");
    assert_eq!(done, 2);
    assert_eq!(evs[0].kind, K::SessionStart);
}

#[test]
fn incomplete_line_does_not_advance_offset_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("s.jsonl");
    let full = std::fs::read_to_string(fixtures().join("claude-code/session_edge.jsonl")).unwrap();
    let cut = full.len() - 30; // 最終行の途中
    std::fs::write(&f, &full.as_bytes()[..cut]).unwrap();
    let a = ClaudeCodeAdapter::with_root(dir.path().to_path_buf(), 20);
    let (e1, o1) = a.parse_incremental(&f, 0).unwrap();
    let last_nl = full[..cut].rfind('\n').unwrap() as u64 + 1;
    assert_eq!(o1, last_nl);
    // 追記が完了したら続きから読める
    std::fs::write(&f, &full).unwrap();
    let (e2, o2) = a.parse_incremental(&f, o1).unwrap();
    assert_eq!(o2, full.len() as u64);
    assert_eq!(kinds(&e2), [K::ResponseComplete]);
    assert_eq!(e1.len() + e2.len(), 6);
    // 再度呼んでも何も出ない
    let (e3, o3) = a.parse_incremental(&f, o2).unwrap();
    assert!(e3.is_empty());
    assert_eq!(o3, o2);
}

#[test]
fn codex_new_format() {
    let a = CodexAdapter::with_root(fixtures().join("codex"), 100);
    let f = fixtures().join("codex/rollout-2026-01-01T00-00-00-bbbbbbbb-0000-4000-8000-000000000002.jsonl");
    let (evs, _) = a.parse_incremental(&f, 0).unwrap();
    assert_eq!(
        kinds(&evs),
        [
            K::SessionStart,
            K::PromptSubmit,
            K::ResponseComplete,
            K::PromptSubmit,
            K::ResponseComplete
        ]
    );
    assert_eq!(evs[0].session_id, "bbbbbbbb-0000-4000-8000-000000000002");
    assert_eq!(evs[0].git_branch.as_deref(), Some("main"));
    assert_eq!(evs[1].prompt_excerpt.as_deref(), Some("ダミーの依頼 2行目"));
    assert!(evs[1].cwd.as_deref().unwrap().ends_with("proj2")); // turn_context で更新
    assert!(evs[0].cwd.as_deref().unwrap().ends_with("proj"));
}

#[test]
fn codex_legacy_format() {
    let a = CodexAdapter::with_root(fixtures().join("codex"), 100);
    let f = fixtures().join("codex/rollout-2025-05-01T00-00-00-cccccccc-0000-4000-8000-000000000003.jsonl");
    let (evs, _) = a.parse_incremental(&f, 0).unwrap();
    assert_eq!(
        kinds(&evs),
        [
            K::SessionStart,
            K::PromptSubmit,
            K::ResponseComplete,
            K::PromptSubmit,
            K::ResponseComplete
        ]
    );
    assert_eq!(evs[1].prompt_excerpt.as_deref(), Some("古い形式の依頼"));
    let mut ats: Vec<_> = evs.iter().map(|e| (e.kind.as_str(), e.at)).collect();
    ats.sort();
    ats.dedup();
    assert_eq!(ats.len(), evs.len());
    // 途中からでもセッション ID を引き継ぐ
    let (later, _) = a
        .parse_incremental(&f, evs[3].source_offset.unwrap() as u64)
        .unwrap();
    assert!(later
        .iter()
        .all(|e| e.session_id == "cccccccc-0000-4000-8000-000000000003"));
}

#[test]
fn import_is_idempotent_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projects/enc");
    std::fs::create_dir_all(root.join("subagents")).unwrap();
    let src = fixtures().join("claude-code/session_edge.jsonl");
    let f = root.join("s.jsonl");
    std::fs::copy(&src, &f).unwrap();
    std::fs::copy(&src, root.join("subagents/agent-x.jsonl")).unwrap();

    let adapters = || -> Vec<Box<dyn AgentAdapter>> {
        vec![Box::new(ClaudeCodeAdapter::with_root(
            dir.path().join("projects"),
            20,
        ))]
    };
    let mut conn = db::open_in_memory().unwrap();
    let s1 = import_with(&mut conn, adapters(), None).unwrap();
    assert_eq!(s1.files_scanned, 1); // subagents は対象外
    assert_eq!(s1.events_inserted, 6);
    let s2 = import_with(&mut conn, adapters(), None).unwrap();
    assert_eq!(s2.events_inserted, 0);
    assert_eq!(s2.files_updated, 0);

    // 縮んだファイルは先頭から読み直しても重複しない
    let full = std::fs::read_to_string(&src).unwrap();
    let first_two: String = full.lines().take(2).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::write(&f, first_two).unwrap();
    let s3 = import_with(&mut conn, adapters(), None).unwrap();
    assert_eq!(s3.events_inserted, 0);
    assert_eq!(s3.files_updated, 1);

    // 追記分だけ増える
    std::fs::write(&f, full).unwrap();
    let s4 = import_with(&mut conn, adapters(), None).unwrap();
    assert_eq!(s4.events_inserted, 0);

    let sessions = repo::list_sessions(&conn, Range::new(0, i64::MAX)).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].prompt_count, 3);
    assert_eq!(sessions[0].git_branch.as_deref(), Some("feature/x"));
    assert!(sessions[0]
        .first_prompt
        .as_deref()
        .unwrap()
        .starts_with("最初の依頼"));
}

#[test]
fn importer_import_file_routes_by_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir_all(&root).unwrap();
    let f = root.join("s.jsonl");
    std::fs::copy(fixtures().join("claude-code/session_edge.jsonl"), &f).unwrap();
    let imp = Importer::new(vec![Box::new(ClaudeCodeAdapter::with_root(root.clone(), 20))]);
    let mut conn = db::open_in_memory().unwrap();
    assert!(imp
        .import_file(&mut conn, &dir.path().join("other.jsonl"))
        .is_none());
    let s = imp.import_file(&mut conn, &f).unwrap();
    assert_eq!(s.events_inserted, 6);
}

#[test]
fn watcher_imports_new_file_and_notifies() {
    use hitoiki_agents::Watcher;
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("p");
    std::fs::create_dir_all(&root).unwrap();
    let imp = Importer::new(vec![Box::new(ClaudeCodeAdapter::with_root(root.clone(), 20))]);
    let conn = db::open(&dir.path().join("h.db")).unwrap();
    let w = Watcher::start(imp, conn, Duration::from_millis(300)).unwrap();
    // 起動時の全件取り込み通知
    w.notices.recv_timeout(Duration::from_secs(10)).unwrap();
    let src = fixtures().join("claude-code/session_edge.jsonl");
    std::fs::copy(src, root.join("s.jsonl")).unwrap();
    let n = w.notices.recv_timeout(Duration::from_secs(15)).unwrap();
    assert_eq!(n.stats.events_inserted, 6);
    drop(w);
}
