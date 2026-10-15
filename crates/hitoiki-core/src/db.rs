//! SQLite の接続とマイグレーション（WAL + busy_timeout、適用前にバックアップ。docs/dev/architecture.md「データモデル」）。

use crate::Result;
use rusqlite::Connection;
use std::path::Path;
use std::time::Duration;

/// (バージョン, SQL)。追加時は末尾に足すこと。
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_judge_cache.sql")),
    (3, include_str!("../migrations/0003_block_template.sql")),
    (4, include_str!("../migrations/0004_correction.sql")),
    (5, include_str!("../migrations/0005_reflection_structured.sql")),
    (6, include_str!("../migrations/0006_kr_measure.sql")),
    (7, include_str!("../migrations/0007_erg_label.sql")),
    (8, include_str!("../migrations/0008_kr_skill.sql")),
    (9, include_str!("../migrations/0009_kr_progress.sql")),
    (10, include_str!("../migrations/0010_kr_suggest_reject.sql")),
    (11, include_str!("../migrations/0011_rename_goal.sql")),
    (12, include_str!("../migrations/0012_rename_records.sql")),
    (13, include_str!("../migrations/0013_rename_actual.sql")),
    (14, include_str!("../migrations/0014_rename_nudge.sql")),
    (15, include_str!("../migrations/0015_rename_nudge_id.sql")),
];

/// DB を開き、未適用のマイグレーションを適用する。
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let existed = path.exists();
    let mut conn = Connection::open(path)?;
    configure(&conn)?;
    let pending = pending_versions(&conn)?;
    if existed && !pending.is_empty() && has_user_tables(&conn)? {
        backup(&conn, path)?;
    }
    migrate(&mut conn)?;
    Ok(conn)
}

/// テスト用のインメモリ DB（マイグレーション適用済み）。
pub fn open_in_memory() -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}

fn ensure_migration_table(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_migration (version INTEGER PRIMARY KEY);")?;
    Ok(())
}

fn has_user_tables(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name <> 'schema_migration'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn pending_versions(conn: &Connection) -> Result<Vec<i64>> {
    ensure_migration_table(conn)?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migration",
        [],
        |r| r.get(0),
    )?;
    Ok(MIGRATIONS
        .iter()
        .map(|(v, _)| *v)
        .filter(|v| *v > current)
        .collect())
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let pending = pending_versions(conn)?;
    for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| pending.contains(v)) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_migration(version) VALUES (?1)", [version])?;
        tx.commit()?;
        tracing::info!(version, "マイグレーションを適用しました");
    }
    Ok(())
}

fn backup(conn: &Connection, path: &Path) -> Result<()> {
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let dest = path.with_extension(format!("db.bak-{stamp}"));
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
    tracing::info!(dest = %dest.display(), "マイグレーション前にバックアップしました");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_in_memory() {
        let conn = open_in_memory().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migration", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n as usize, MIGRATIONS.len());
    }

    /// 0005 までの DB にある成果の指標は、0006 の適用後に既定の数え方（time）と最小分数（15）になる。
    #[test]
    fn migration_0006_既存の成果の指標に既定値が入る() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v <= 5) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        conn.execute(
            "INSERT INTO objective (id, title, status) VALUES ('O1', 'o', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO key_result (id, objective_id, title) VALUES ('K1', 'O1', 'k')",
            [],
        )
        .unwrap();
        let expected: Vec<i64> = MIGRATIONS.iter().map(|(v, _)| *v).filter(|v| *v > 5).collect();
        assert_eq!(pending_versions(&conn).unwrap(), expected);
        migrate(&mut conn).unwrap();
        let (m, d): (String, i64) = conn
            .query_row(
                "SELECT measure, active_day_min FROM key_result WHERE id = 'K1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((m.as_str(), d), ("time", 15));
    }

    /// 0010 までの DB の目標・成果の指標は、0011 の適用後に新しい表名・列名・値になる。
    #[test]
    fn migration_0011_目標と成果の指標の物理名と値が書き換わる() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v <= 10) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO objective (id, title, status) VALUES ('O1', 'o1', 'done'), ('O2', 'o2', 'active');
             INSERT INTO key_result (id, objective_id, title, status, measure, day_min) VALUES
               ('K1', 'O1', 'k1', 'done', 'days', 20),
               ('K2', 'O1', 'k2', 'active', 'done', 15),
               ('K3', 'O2', 'k3', 'dropped', 'time', 15);
             INSERT INTO skill (id, name, preset) VALUES ('S1', 's', 0);
             INSERT INTO kr_skill (kr_id, skill_id) VALUES ('K1', 'S1');
             INSERT INTO kr_progress (id, kr_id, value, text, note, at) VALUES ('P1', 'K1', 1, '1', NULL, 100);
             INSERT INTO kr_suggest_reject (kr_id, kind, target_id, at) VALUES ('K1', 'task', 'T1', 100);",
        )
        .unwrap();
        migrate(&mut conn).unwrap();

        let goals: Vec<(String, String)> = conn
            .prepare("SELECT id, status FROM goal ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            goals,
            vec![("O1".into(), "achieved".into()), ("O2".into(), "active".into())]
        );
        let key_results: Vec<(String, String, String, String, i64)> = conn
            .prepare("SELECT id, goal_id, status, measure, active_day_min FROM key_result ORDER BY id")
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let t = |a: &str, b: &str, c: &str, d: &str, e: i64| {
            (a.to_string(), b.to_string(), c.to_string(), d.to_string(), e)
        };
        assert_eq!(
            key_results,
            vec![
                t("K1", "O1", "achieved", "active_days", 20),
                t("K2", "O1", "active", "done_tasks", 15),
                t("K3", "O2", "dropped", "time", 15),
            ]
        );
        let n = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(
            n("SELECT COUNT(*) FROM key_result_skill WHERE key_result_id = 'K1'"),
            1
        );
        assert_eq!(
            n("SELECT COUNT(*) FROM key_result_progress WHERE key_result_id = 'K1'"),
            1
        );
        assert_eq!(
            n("SELECT COUNT(*) FROM link_suggestion_reject WHERE key_result_id = 'K1'"),
            1
        );
        // 旧名の表は残らない。
        assert_eq!(
            n("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('objective', 'kr_skill', 'kr_progress', 'kr_suggest_reject')"),
            0
        );
    }

    #[test]
    fn 従来の調子のerg_tagはユーザーのラベルに引き継がれる() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v < 7) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        conn.execute(
            "INSERT INTO checkin (id, at, erg_tag) VALUES ('C1', 1000, 'growth'), ('C2', 2000, NULL)",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let rows: Vec<(String, String, String, i64)> = conn
            .prepare("SELECT record_kind, record_id, tag || '/' || source, at FROM erg_label")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("condition".into(), "C1".into(), "growth/user".into(), 1000)]
        );
    }

    /// 0012: 調子・気になったこと・捉え直しの表と列、保存済みの種類の値が用語集の名前になる。
    #[test]
    fn migration_0012_記録の表と保存済みの値が新しい名前になる() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v < 12) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO checkin (id, at, energy) VALUES ('C1', 1000, 4);
             INSERT INTO hassle (id, at, text, intensity) VALUES ('H1', 2000, 'x', 3);
             INSERT INTO abc_record (id, at, activating_event, belief, consequence_emotion,
               consequence_intensity, dispute, ai_dialog_md, hassle_id)
               VALUES ('A1', 3000, 'e', 'b', 'em', 2, 'd', 'dm', 'H1');
             INSERT INTO erg_label (record_kind, record_id, tag, source, at) VALUES
               ('checkin', 'C1', 'growth', 'user', 1000),
               ('hassle', 'H1', 'existence', 'suggested', 2000),
               ('abc', 'A1', 'none', 'ai', 3000),
               ('reflection', 'R1', 'growth', 'suggested', 4000);
             INSERT INTO reflection (id, at, kind, context_json, questions_md, structured_json, answers_json)
               VALUES ('R1', 4000, 'day_end',
                 '{\"checkins\":[],\"hassles\":[],\"abc_records\":[{\"activating_event\":\"e\",\"has_dispute\":true}],\"erg_distribution\":{\"pending\":1}}',
                 '',
                 '{\"questions\":[{\"kind\":\"checkin\"},{\"kind\":\"abc\",\"activating_event\":\"e\"},{\"kind\":\"free\"}]}',
                 '{\"items\":[{\"index\":0,\"kind\":\"hassle\",\"record_id\":\"H1\"}]}');",
        )
        .unwrap();
        migrate(&mut conn).unwrap();

        let c: (i64, i64) = conn
            .query_row("SELECT at, energy FROM condition WHERE id = 'C1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(c, (1000, 4));
        // 使っていなかった erg_tag 列は無くなる。
        assert!(conn.prepare("SELECT erg_tag FROM condition").is_err());
        let h: String = conn
            .query_row("SELECT text FROM concern WHERE id = 'H1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(h, "x");
        let a: (String, String, String, i64, String, String, String) = conn
            .query_row(
                "SELECT event, belief, emotion, emotion_intensity, alternative, deepening_md, concern_id
                 FROM reframe WHERE id = 'A1'",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            a,
            (
                "e".into(),
                "b".into(),
                "em".into(),
                2,
                "d".into(),
                "dm".into(),
                "H1".into()
            )
        );
        for old in ["checkin", "hassle", "abc_record"] {
            let n: i64 = conn
                .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = ?1", [old], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(n, 0, "{old}");
        }

        let labels: Vec<String> = conn
            .prepare("SELECT record_kind || '/' || source FROM erg_label ORDER BY at")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            labels,
            vec![
                "condition/user",
                "concern/unconfirmed",
                "reframe/ai",
                "reflection/unconfirmed"
            ]
        );

        let (ctx, st, ans): (String, String, String) = conn
            .query_row(
                "SELECT context_json, structured_json, answers_json FROM reflection WHERE id = 'R1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            ctx,
            r#"{"conditions":[],"concerns":[],"reframes":[{"event":"e","has_alternative":true}],"erg_distribution":{"unconfirmed":1}}"#
        );
        assert_eq!(
            st,
            r#"{"questions":[{"kind":"condition"},{"kind":"reframe","event":"e"},{"kind":"free"}]}"#
        );
        assert_eq!(
            ans,
            r#"{"items":[{"index":0,"kind":"concern","record_id":"H1"}]}"#
        );
    }

    /// 0013: 実績まわりのテーブル・列・保存済みの文字列値が新しい名前に移る。
    #[test]
    fn migration_0013_実績まわりの名前と値が書き換わる() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v < 13) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO task (id, title, created_at, updated_at) VALUES ('T1', 't', 0, 0);
             INSERT INTO activity_segment (id, start_at, end_at, kind, activity_label, decided_by, status, offline)
               VALUES ('S1', 0, 10, 'agent_watch', '開発', 'user', 'confirmed', 1),
                      ('S2', 10, 20, 'work', NULL, 'rule', 'auto', 0);
             INSERT INTO segment_override (id, start_at, end_at, kind, task_id, activity_label, created_at, offline, correction_id)
               VALUES ('O1', 0, 10, 'agent_watch', 'T1', '調査', 5, 1, 'C1');
             INSERT INTO segment_correction (id, start_at, end_at, task_id, kind, activity_label, total_ms, offline_ms, segment_count, created_at)
               VALUES ('C1', 0, 10, 'T1', 'agent_watch', '調査', 10, 4, 1, 5);
             INSERT INTO activity_rule (id, field, pattern, activity_label) VALUES ('A1', 'exe', 'x', '連絡');
             INSERT INTO task_match_rule (id, task_id, field, pattern) VALUES ('R1', 'T1', 'exe', 'code*');
             INSERT INTO setting (key, value_json) VALUES
               ('review.rule_suggest_dismissed', '[1]'), ('review.ignore_suggest_dismissed', '[2]');
             INSERT INTO judge_cache (key, answer_json, judge_name, created_at) VALUES ('k', '{}', 'j', 0);",
        )
        .unwrap();
        assert!(pending_versions(&conn).unwrap().contains(&13));
        migrate(&mut conn).unwrap();

        let row = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap() };
        assert_eq!(
            row("SELECT kind || '/' || activity || '/' || decided_by || '/' || off_pc FROM activity_segment WHERE id = 'S1'"),
            "agent_wait/開発/confirmation/1"
        );
        assert_eq!(
            row("SELECT decided_by FROM activity_segment WHERE id = 'S2'"),
            "rule"
        );
        assert_eq!(
            row("SELECT kind || '/' || activity || '/' || off_pc || '/' || correction_id FROM segment_confirmation"),
            "agent_wait/調査/1/C1"
        );
        assert_eq!(
            row("SELECT kind || '/' || activity || '/' || off_pc_ms FROM segment_correction"),
            "agent_wait/調査/4"
        );
        assert_eq!(row("SELECT activity FROM activity_rule"), "連絡");
        assert_eq!(
            row("SELECT task_id || '/' || pattern || '/' || pattern_type FROM task_assign_rule"),
            "T1/code*/contains"
        );
        // 作り直したテーブルの既定の種類は glob（CLI・サービスの既定と同じ）。
        conn.execute(
            "INSERT INTO task_assign_rule (id, task_id, field, pattern) VALUES ('R2', 'T1', 'exe', 'y')",
            [],
        )
        .unwrap();
        assert_eq!(
            row("SELECT pattern_type FROM task_assign_rule WHERE id = 'R2'"),
            "glob"
        );
        assert_eq!(
            row("SELECT group_concat(key) FROM (SELECT key FROM setting ORDER BY key)"),
            "actual.ignore_suggest_dismissed,actual.rule_suggest_dismissed"
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM judge_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
        let gone: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('segment_override', 'task_match_rule')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(gone, 0);
    }

    /// 0014: push_log -> nudge_log、time_block -> plan_block、保存済みの文字列値も新しい名前になる。
    #[test]
    fn migration_0014_問いかけと計画ブロックとはじめにの名前が揃う() {
        let mut conn = Connection::open_in_memory().unwrap();
        ensure_migration_table(&conn).unwrap();
        for (v, sql) in MIGRATIONS.iter().filter(|(v, _)| *v < 14) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO schema_migration(version) VALUES (?1)", [v])
                .unwrap();
        }
        let p1 = r#"{"trigger":"kr_idle","title":"t","body":"b","actions":[{"id":"kr_skip_week","label":"x","value":null},{"id":"open","label":"y","value":null}],"input":{"id":"hassle","placeholder":"p"},"open_screen":"reflection/day_end"}"#;
        let p2 = r#"{"trigger":"long_focus","title":"t","body":"b","actions":[],"input":null,"open_screen":"checkin"}"#;
        let p3 = r#"{"trigger":"day_start","title":"t","body":"b","actions":[],"input":{"id":"hassle","placeholder":"p"},"open_screen":"hassle"}"#;
        let p4 = r#"{"trigger":"idle_return","title":"t","body":"b","actions":[],"input":null,"open_screen":"review"}"#;
        for (id, trig, payload, resp) in [
            (
                "N1",
                "kr_idle",
                p1,
                Some(r#"{"action":"kr_skip_week","value":null,"text":null}"#),
            ),
            ("N2", "long_focus", p2, None),
            ("N3", "day_start", p3, Some(r#"{"action":"open"}"#)),
            ("N4", "idle_return", p4, None),
        ] {
            conn.execute(
                "INSERT INTO push_log (id, at, trigger, payload_json, delivered, response_json) VALUES (?1, 1000, ?2, ?3, 1, ?4)",
                rusqlite::params![id, trig, payload, resp],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO time_block (id, start_at, end_at, label, focus) VALUES ('B1', 1000, 2000, 'x', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reflection (id, at, kind, context_json, questions_md) VALUES ('R1', 1, 'day_end', '{\"time_blocks\":[]}', 'q')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO setting (key, value_json) VALUES ('onboarding.completed_at', '123')",
            [],
        )
        .unwrap();

        assert!(pending_versions(&conn).unwrap().contains(&14));
        migrate(&mut conn).unwrap();

        let get = |id: &str| -> (String, String, Option<String>) {
            conn.query_row(
                "SELECT trigger, payload_json, response_json FROM nudge_log WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        let (t, p, r) = get("N1");
        assert_eq!(t, "key_result_idle");
        assert!(p.contains(r#""trigger":"key_result_idle""#));
        assert!(p.contains(r#""id":"key_result_skip_week""#));
        assert!(p.contains(r#""id":"open""#));
        assert!(p.contains(r#""input":{"id":"concern""#));
        assert!(p.contains(r#""open_screen":"records/new/day_end""#));
        assert_eq!(
            r.unwrap(),
            r#"{"action":"key_result_skip_week","value":null,"text":null}"#
        );
        let (t, p, _) = get("N2");
        assert_eq!(t, "long_work");
        assert!(p.contains(r#""trigger":"long_work""#));
        assert!(p.contains(r#""open_screen":"records/new/condition""#));
        let (t, p, r) = get("N3");
        assert_eq!(t, "day_start");
        assert!(p.contains(r#""input":{"id":"concern""#));
        assert!(p.contains(r#""open_screen":"records/new/concern""#));
        assert_eq!(r.unwrap(), r#"{"action":"open"}"#);
        let (_, p, _) = get("N4");
        assert!(p.contains(r#""open_screen":"actual?status=review""#));

        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM plan_block WHERE id = 'B1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
        assert!(conn.prepare("SELECT 1 FROM push_log").is_err());
        assert!(conn.prepare("SELECT 1 FROM time_block").is_err());
        let ctx: String = conn
            .query_row("SELECT context_json FROM reflection WHERE id = 'R1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(ctx, r#"{"plan_blocks":[]}"#);
        let k: String = conn
            .query_row("SELECT key FROM setting", [], |r| r.get(0))
            .unwrap();
        assert_eq!(k, "welcome.completed_at");
    }

    /// 0015: 調子・気になったことの push_id -> nudge_id。
    #[test]
    fn migration_0015_調子と気になったことの問いかけの参照がnudge_idになる() {
        let conn = open_in_memory().unwrap();
        for t in ["condition", "concern"] {
            assert!(conn.prepare(&format!("SELECT nudge_id FROM {t}")).is_ok());
            assert!(conn.prepare(&format!("SELECT push_id FROM {t}")).is_err());
        }
    }

    #[test]
    fn reopen_file_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        drop(open(&p).unwrap());
        drop(open(&p).unwrap());
    }
}
