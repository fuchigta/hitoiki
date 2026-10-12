use super::*;
use crate::model::{SegmentKind, SegmentStatus};
use crate::repo::okr;
use crate::service::impact::DeleteMode;
use crate::service::okr::{self as okr_svc, KrEdit, KrInput, ObjectiveInput};
use crate::service::task::{self, TaskInput};
use crate::service::testutil::{insert_segment, Env};
use crate::Clock;

const MIN: i64 = 60_000;
const DAY: i64 = 24 * 60 * MIN;

fn setup(ctx: &Ctx) -> (String, String) {
    let o = okr_svc::add_objective(
        ctx,
        &ObjectiveInput {
            title: "O".into(),
            from: None,
            to: None,
        },
    )
    .unwrap();
    let k = okr_svc::add_kr(
        ctx,
        &KrInput {
            objective: o.id,
            title: "K".into(),
            target: Some("10".into()),
        },
    )
    .unwrap();
    let t = task::add(
        ctx,
        &TaskInput {
            title: Some("T".into()),
            key_result: Some(k.id.clone()),
            ..Default::default()
        },
    )
    .unwrap()
    .task
    .id;
    (k.id, t)
}

fn count_rows(c: &Ctx) -> i64 {
    c.conn
        .query_row("SELECT COUNT(*) FROM kr_progress", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn 数値は先頭の数だけ読む() {
    assert_eq!(parse_value("12"), Some(12.0));
    assert_eq!(parse_value(" 3.5 回"), Some(3.5));
    assert_eq!(parse_value("-2"), Some(-2.0));
    assert_eq!(parse_value("7."), Some(7.0));
    assert_eq!(parse_value("週 3 回"), None);
    assert_eq!(parse_value(""), None);
    assert_eq!(parse_value("-"), None);
}

#[test]
fn 現在値が変わったときだけ履歴が残る() {
    let env = Env::new();
    let c = env.ctx();
    let (kr, _) = setup(&c);
    let edit = |current: Option<&str>, note: Option<&str>, title: Option<&str>| {
        okr_svc::edit_kr(
            &c,
            &kr,
            &KrEdit {
                current: current.map(Into::into),
                current_note: note.map(Into::into),
                title: title.map(Into::into),
                ..Default::default()
            },
        )
        .unwrap();
    };
    edit(None, None, Some("K2")); // 現在値に触れない編集では残さない
    edit(Some("3"), Some("  初回 "), None);
    edit(Some("3"), None, None); // 同じ値では残さない
    edit(Some("4 件"), None, None);
    edit(Some(""), None, None); // 消した
    let h = kr_progress::list_of_kr(c.conn, &kr).unwrap();
    let got: Vec<_> = h
        .iter()
        .map(|p| (p.value, p.text.as_deref(), p.note.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            (Some(3.0), Some("3"), Some("初回")),
            (Some(4.0), Some("4 件"), None),
            (None, None, None),
        ]
    );
}

#[test]
fn 記録は同じ値でも1行残し_空は受け付けない() {
    let env = Env::new();
    let c = env.ctx();
    let (kr, _) = setup(&c);
    let r = record(&c, &kr, "5", Some("メモ")).unwrap();
    assert_eq!(r.key_result.current.as_deref(), Some("5"));
    record(&c, "K", "5", None).unwrap(); // タイトルでも指定できる
    assert_eq!(count_rows(&c), 2);
    assert!(record(&c, &kr, "  ", None).is_err());
    assert!(record(&c, "なし", "1", None).is_err());
}

#[test]
fn 時系列は週ごとの値と時間を並べる() {
    let env = Env::new();
    let c = env.ctx();
    let (kr, t) = setup(&c);
    let now = env.clock.now_ms();
    // 8 日前（先週）に 1、今に 5 回。
    env.clock.set(now - 8 * DAY);
    record(&c, &kr, "1", None).unwrap();
    env.clock.set(now);
    record(&c, &kr, "5 回", Some("追い込み")).unwrap();
    let d = env.day_start(); // 2026-10-10（土）
    insert_segment(
        c.conn,
        "A",
        d + 60 * MIN,
        d + 90 * MIN,
        SegmentKind::Work,
        Some(&t),
        SegmentStatus::Auto,
    );
    insert_segment(
        c.conn,
        "B",
        d - 7 * DAY + 60 * MIN,
        d - 7 * DAY + 80 * MIN,
        SegmentKind::Work,
        Some(&t),
        SegmentStatus::Auto,
    );
    let h = kr_history(&c, &kr, None).unwrap();
    assert_eq!(h.weeks.len(), 8);
    let n = h.weeks.len();
    let last = &h.weeks[n - 1];
    let prev = &h.weeks[n - 2];
    let before = &h.weeks[n - 3];
    assert_eq!(last.week_start, "2026-10-05");
    assert_eq!((last.value, last.text.as_deref()), (Some(5.0), Some("5 回")));
    assert_eq!((last.duration_ms, last.changes), (30 * MIN, 1));
    assert_eq!(
        (prev.value, prev.duration_ms, prev.changes),
        (Some(1.0), 20 * MIN, 1)
    );
    assert_eq!((before.value, before.duration_ms, before.changes), (None, 0, 0));
    assert_eq!(h.entries.len(), 2);
}

#[test]
fn 数として読めない値の週はその文字列を残し_数値はnull() {
    let env = Env::new();
    let c = env.ctx();
    let (kr, _) = setup(&c);
    record(&c, &kr, "半分くらい", None).unwrap();
    let h = kr_history(&c, &kr, None).unwrap();
    let last = h.weeks.last().unwrap();
    assert_eq!((last.value, last.text.as_deref()), (None, Some("半分くらい")));
}

#[test]
fn kr_と_objective_の削除で履歴も消える() {
    let env = Env::new();
    let c = env.ctx();
    let (kr, _) = setup(&c);
    record(&c, &kr, "1", None).unwrap();
    okr_svc::delete_kr(&c, &kr, DeleteMode::FORCE).unwrap();
    assert_eq!(count_rows(&c), 0);

    let (kr2, _) = setup(&c);
    record(&c, &kr2, "2", None).unwrap();
    let oid = okr::get_kr(c.conn, &kr2).unwrap().objective_id;
    okr_svc::delete_objective(&c, &oid, DeleteMode::FORCE).unwrap();
    assert_eq!(count_rows(&c), 0);
}
