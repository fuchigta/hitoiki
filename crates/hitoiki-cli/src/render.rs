//! 人間向けの出力整形。JSON 出力は service 層の構造体をそのまま使う。

use crate::table::{pad, table, truncate};
use chrono_tz::Tz;
use hitoiki_core::model::{Checkin, Hassle, KeyResult, Objective, TaskMatchRule, TimeBlock};
use hitoiki_core::repo::segment::kind_label;
use hitoiki_core::service::{
    log::Log,
    okr::OkrList,
    plan::BlockList,
    report::{Report, ReportBy, ReportRow},
    review::{CreatedRule, ReviewList, ReviewSetResult},
    skill::{PresetImport, SkillList},
    status::Status,
    task::{RuleList, TaskList, TaskShow, TaskView},
    SegmentView,
};
use hitoiki_core::timefmt::{fmt_duration, fmt_hm, to_local};
use hitoiki_core::Millis;

/// 表示用の短縮 ID（ULID は先頭の時刻部が共通になりやすいので 12 文字）。
pub fn short(id: &str) -> String {
    id.chars().take(12).collect()
}

fn opt(s: &Option<String>) -> String {
    s.clone().unwrap_or_default()
}

fn flag(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => "○",
        Some(false) => "×",
        None => "-",
    }
}

fn dt(tz: Tz, ms: Millis) -> String {
    to_local(tz, ms).format("%Y-%m-%d %H:%M").to_string()
}

fn dur(ms: i64) -> String {
    fmt_duration(ms)
}

pub fn tasks(list: &TaskList) -> String {
    if list.tasks.is_empty() {
        return "タスクはありません。".into();
    }
    let rows: Vec<Vec<String>> = list
        .tasks
        .iter()
        .map(|t| {
            vec![
                short(&t.task.id),
                t.task.status.to_string(),
                t.task.kind.to_string(),
                format!("{}/{}", flag(t.task.urgent), flag(t.task.important)),
                t.task.estimate_min.map(|m| format!("{m}m")).unwrap_or_default(),
                truncate(&t.task.title, 50),
            ]
        })
        .collect();
    table(&["ID", "状態", "種別", "急/重", "見積", "タイトル"], &rows)
}

pub fn task_detail(tz: Tz, v: &TaskView) -> String {
    let t = &v.task;
    let mut s = format!(
        "ID:       {}\nタイトル: {}\n状態:     {} ({})\n",
        t.id, t.title, t.status, t.kind
    );
    s += &format!("緊急/重要: {} / {}\n", flag(t.urgent), flag(t.important));
    if let Some(m) = t.estimate_min {
        s += &format!("見積もり: {m}分\n");
    }
    if let Some(kr) = &v.key_result {
        s += &format!(
            "KR:       {} / {} ({})\n",
            kr.objective_title,
            kr.title,
            short(&kr.id)
        );
    }
    if !v.skills.is_empty() {
        s += &format!("スキル:   {}\n", v.skills.join(", "));
    }
    if let Some(u) = &t.external_url {
        s += &format!("URL:      {u}\n");
    }
    if let (Some(a), Some(b)) = (t.planned_start_at, t.planned_end_at) {
        s += &format!("計画:     {} - {}\n", dt(tz, a), dt(tz, b));
    }
    s += &format!("作成:     {}\n", dt(tz, t.created_at));
    if let Some(d) = t.done_at {
        s += &format!("終了:     {}\n", dt(tz, d));
    }
    if !t.body_md.is_empty() {
        s += &format!("\n{}\n", t.body_md);
    }
    s.trim_end().to_string()
}

pub fn task_show(tz: Tz, v: &TaskShow) -> String {
    let mut s = task_detail(tz, &v.task);
    s += &format!(
        "\n\n実績:     {}（{} 区間）",
        dur(v.tracked_ms),
        v.tracked_segments
    );
    if !v.rules.is_empty() {
        s += &format!("\n\n帰属ルール:\n{}", rules_table(&v.rules));
    }
    s
}

pub fn rules_table(rules: &[TaskMatchRule]) -> String {
    if rules.is_empty() {
        return "ルールはありません。".into();
    }
    let rows: Vec<Vec<String>> = rules
        .iter()
        .map(|r| {
            vec![
                short(&r.id),
                short(&r.task_id),
                r.field.to_string(),
                r.pattern_type.to_string(),
                r.priority.to_string(),
                r.pattern.clone(),
            ]
        })
        .collect();
    table(&["ID", "タスク", "フィールド", "種類", "優先", "パターン"], &rows)
}

pub fn rule_list(l: &RuleList) -> String {
    rules_table(&l.rules)
}

fn seg_row(tz: Tz, s: &SegmentView) -> Vec<String> {
    let target = match (&s.task_title, &s.activity_label) {
        (Some(t), Some(a)) => format!("{t} [{a}]"),
        (Some(t), None) => t.clone(),
        (None, Some(a)) => format!("[{a}]"),
        (None, None) => String::new(),
    };
    let basis = match (&s.exe_name, &s.title) {
        (Some(e), Some(t)) => format!("{e} {t}"),
        (Some(e), None) => e.clone(),
        (None, Some(t)) => t.clone(),
        _ => String::new(),
    };
    vec![
        short(&s.id),
        format!("{}-{}", fmt_hm(tz, s.start_at), fmt_hm(tz, s.end_at)),
        dur(s.duration_ms),
        kind_label(s.kind).to_string(),
        truncate(&target, 36),
        s.status.to_string(),
        truncate(&basis, 40),
    ]
}

const SEG_HEADERS: [&str; 7] = ["ID", "時刻", "長さ", "種別", "タスク/活動", "状態", "根拠"];

pub fn segments(tz: Tz, segs: &[SegmentView]) -> String {
    if segs.is_empty() {
        return "区間はありません。".into();
    }
    let rows: Vec<Vec<String>> = segs.iter().map(|s| seg_row(tz, s)).collect();
    table(&SEG_HEADERS, &rows)
}

pub fn review_list(tz: Tz, l: &ReviewList) -> String {
    let hidden = if l.hidden_short > 0 {
        format!(
            "\n{}未満の短い区間が {} 件あります（--all で表示）",
            short_threshold(l.min_sec),
            l.hidden_short
        )
    } else {
        String::new()
    };
    if l.count == 0 {
        return format!("要確認の区間はありません。{hidden}");
    }
    format!(
        "要確認の区間: {} 件\n{}{hidden}",
        l.count,
        segments(tz, &l.segments)
    )
}

/// 「1分」「90秒」のように短い区間のしきい値を書く。
fn short_threshold(sec: u64) -> String {
    if sec >= 60 && sec.is_multiple_of(60) {
        format!("{}分", sec / 60)
    } else {
        format!("{sec}秒")
    }
}

fn rule_line(r: &CreatedRule) -> String {
    format!(
        "ルール{}: {} {} {} ({})",
        if r.reused { "(既存)" } else { "を作成" },
        r.rule_type,
        r.field,
        r.pattern,
        r.pattern_type
    )
}

pub fn review_set(tz: Tz, r: &ReviewSetResult) -> String {
    let mut s = format!(
        "確定しました:\n{}",
        segments(tz, std::slice::from_ref(&r.segment))
    );
    if let Some(rule) = &r.rule {
        s += &format!("\n{}", rule_line(rule));
    }
    s
}

fn gap_line(tz: Tz, g: &hitoiki_core::service::log::GapView) -> String {
    format!(
        "{}-{}  記録なし（収集が止まっていた時間帯・{}）",
        fmt_hm(tz, g.start_at),
        fmt_hm(tz, g.end_at),
        dur(g.duration_ms)
    )
}

pub fn log(tz: Tz, l: &Log) -> String {
    let Some(raw) = &l.raw else {
        let mut s = segments(tz, &l.segments);
        for g in &l.gaps {
            s += &format!(
                "
{}",
                gap_line(tz, g)
            );
        }
        return s;
    };
    // 区間と生データを時系列に混ぜる。
    let mut lines: Vec<(Millis, u8, String)> = Vec::new();
    for s in &l.segments {
        let r = seg_row(tz, s);
        lines.push((
            s.start_at,
            0,
            format!(
                "{}  区間   {}  {}  {}  {}  {}",
                r[1],
                r[3],
                r[2],
                r[4],
                r[5],
                short(&s.id)
            ),
        ));
    }
    for e in raw {
        let end = e.end_at.map(|x| fmt_hm(tz, x)).unwrap_or_else(|| "…".into());
        lines.push((
            e.at,
            1,
            format!(
                "{}-{}  {:<12} {}",
                fmt_hm(tz, e.at),
                end,
                e.source,
                truncate(&e.summary, 70)
            ),
        ));
    }
    for g in &l.gaps {
        lines.push((g.start_at, 1, gap_line(tz, g)));
    }
    lines.sort_by_key(|a| (a.0, a.1));
    if lines.is_empty() {
        return "記録はありません。".into();
    }
    lines.into_iter().map(|l| l.2).collect::<Vec<_>>().join("\n")
}

fn report_rows(rows: &[ReportRow]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| {
            let bar = "█".repeat((r.ratio * 20.0).round() as usize);
            vec![
                truncate(&r.label, 40),
                dur(r.duration_ms),
                format!("{:.0}%", r.ratio * 100.0),
                bar,
            ]
        })
        .collect()
}

pub fn report(tz: Tz, r: &Report) -> String {
    let by = match r.by {
        ReportBy::Task => "タスク",
        ReportBy::Activity => "活動",
        ReportBy::Quadrant => "象限",
        ReportBy::Okr => "OKR",
        ReportBy::Skill => "スキル",
        ReportBy::Agent => "エージェント",
        ReportBy::Kind => "種別",
    };
    let mut s = format!(
        "期間: {} - {}  集計単位: {}\n合計: {}\n",
        dt(tz, r.range.start_at),
        dt(tz, r.range.end_at),
        by,
        dur(r.total_ms)
    );
    if r.rows.is_empty() {
        s += "\n該当する区間はありません。";
    } else {
        s += &format!("\n{}", table(&[by, "時間", "割合", ""], &report_rows(&r.rows)));
    }
    if !r.include_idle && (r.excluded.idle_ms > 0 || r.excluded.break_ms > 0) {
        s += &format!(
            "\n\n合計から除外: 無操作 {} / 休憩 {}",
            dur(r.excluded.idle_ms),
            dur(r.excluded.break_ms)
        );
    }
    if let Some(n) = &r.note {
        s += &format!("\n注: {n}");
    }
    s
}

pub fn status(tz: Tz, st: &Status) -> String {
    let mut s = String::new();
    s += "実行中のタスク:\n";
    if st.doing.is_empty() {
        s += "  (なし)\n";
    }
    for d in &st.doing {
        let since = d
            .since
            .map(|x| format!(" ({} から)", fmt_hm(tz, x)))
            .unwrap_or_default();
        s += &format!("  {} {}{}\n", short(&d.task.task.id), d.task.task.title, since);
    }
    s += "\n直近の区間:\n";
    if st.recent_segments.is_empty() {
        s += "  (なし)\n";
    } else {
        let mut recent = st.recent_segments.clone();
        recent.reverse();
        for line in segments(tz, &recent).lines() {
            s += &format!("  {line}\n");
        }
    }
    s += &format!(
        "\n今日の集計: {}（無操作 {} / 休憩 {} は除外）\n",
        dur(st.today.total_ms),
        dur(st.today.idle_ms),
        dur(st.today.break_ms)
    );
    for r in &st.today.by_kind {
        s += &format!("  {} {}\n", pad(&r.label, 20), dur(r.duration_ms));
    }
    if !st.today.top_tasks.is_empty() {
        s += "  タスク上位:\n";
        for r in &st.today.top_tasks {
            s += &format!("    {}  {}\n", dur(r.duration_ms), truncate(&r.label, 50));
        }
    }
    s += &format!("\n要確認の区間: {} 件", st.needs_review_count);
    if st.needs_review_short_count > 0 {
        s += &format!(
            "（ほかに短い区間 {} 件は仕分けで既定非表示）",
            st.needs_review_short_count
        );
    }
    s += "\n";
    if st.unanswered_pushes.is_empty() {
        s += "未応答のプッシュ: なし";
    } else {
        s += "未応答のプッシュ:";
        for p in &st.unanswered_pushes {
            s += &format!("\n  {} {} ({})", fmt_hm(tz, p.at), p.trigger, short(&p.id));
        }
    }
    if let Some(v) = &st.update_available {
        s += &format!(
            "\n\n新しいバージョンがあります: v{v}（{}）",
            hitoiki_core::service::update::RELEASE_PAGE_URL
        );
    }
    s
}

pub fn blocks(tz: Tz, l: &BlockList) -> String {
    if l.blocks.is_empty() {
        return "ブロックはありません。".into();
    }
    let rows: Vec<Vec<String>> = l
        .blocks
        .iter()
        .map(|b| {
            vec![
                short(&b.block.id),
                format!("{}-{}", fmt_hm(tz, b.block.start_at), fmt_hm(tz, b.block.end_at)),
                if b.block.focus {
                    "集中".into()
                } else {
                    String::new()
                },
                opt(&b.task_title),
                opt(&b.block.label),
            ]
        })
        .collect();
    table(&["ID", "時刻", "集中", "タスク", "ラベル"], &rows)
}

pub fn block_added(tz: Tz, b: &TimeBlock) -> String {
    format!(
        "ブロックを追加しました: {} {}-{}{}",
        short(&b.id),
        fmt_hm(tz, b.start_at),
        fmt_hm(tz, b.end_at),
        if b.focus { "（集中）" } else { "" }
    )
}

pub fn checkin(c: &Checkin) -> String {
    let f = |n: Option<i64>| n.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    format!(
        "記録しました: {} (エネルギー {} / 気分 {} / ストレス {})",
        short(&c.id),
        f(c.energy),
        f(c.mood),
        f(c.stress)
    )
}

pub fn hassle(h: &Hassle) -> String {
    format!("ハッスルを記録しました: {} (強度 {})", short(&h.id), h.intensity)
}

pub fn okr_list(l: &OkrList) -> String {
    if l.objectives.is_empty() {
        return "Objective はありません。".into();
    }
    let mut s = String::new();
    for o in &l.objectives {
        s += &format!(
            "{} {} [{}]\n",
            short(&o.objective.id),
            o.objective.title,
            o.objective.status
        );
        for k in &o.key_results {
            s += &format!(
                "  {} {}{}\n",
                short(&k.id),
                k.title,
                k.target
                    .as_ref()
                    .map(|t| format!(" (目標: {t})"))
                    .unwrap_or_default()
            );
        }
    }
    s.trim_end().to_string()
}

pub fn objective_added(o: &Objective) -> String {
    format!("Objective を追加しました: {} {}", short(&o.id), o.title)
}

pub fn kr_added(k: &KeyResult) -> String {
    format!("KR を追加しました: {} {}", short(&k.id), k.title)
}

pub fn skills(l: &SkillList) -> String {
    if l.skills.is_empty() {
        return "スキルはありません。`hitoiki skill import-preset` でプリセットを取り込めます。".into();
    }
    let rows: Vec<Vec<String>> = l
        .skills
        .iter()
        .map(|s| {
            vec![
                short(&s.skill.id),
                s.skill.name.clone(),
                opt(&s.parent_name),
                if s.skill.preset {
                    "プリセット".into()
                } else {
                    String::new()
                },
            ]
        })
        .collect();
    table(&["ID", "名前", "親", ""], &rows)
}

pub fn preset_import(p: &PresetImport) -> String {
    format!(
        "取り込み: {} 件 ({}) / 既存のためスキップ: {} 件",
        p.imported.len(),
        p.imported.join(", "),
        p.skipped.len()
    )
}

pub fn reflection(tz: Tz, r: &hitoiki_core::model::Reflection) -> String {
    let mut s = format!(
        "# 振り返り {} ({}, {})\n\n{}",
        short(&r.id),
        r.kind,
        dt(tz, r.at),
        r.questions_md
    );
    if let Some(a) = &r.answers_md {
        s += &format!("\n\n## 回答\n\n{a}");
    }
    s
}

pub fn reflection_list(tz: Tz, list: &[hitoiki_core::model::Reflection]) -> String {
    if list.is_empty() {
        return "振り返りはありません。".into();
    }
    let rows = list
        .iter()
        .map(|r| {
            vec![
                short(&r.id),
                dt(tz, r.at),
                r.kind.to_string(),
                if r.answers_md.is_some() { "回答済" } else { "-" }.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    table(&["ID", "日時", "種別", "回答"], &rows)
}

pub fn abc_dispute(a: &hitoiki_core::model::AbcRecord) -> String {
    format!(
        "ABC {} の問いを追記しました。\n\n{}",
        short(&a.id),
        a.ai_dialog_md.as_deref().unwrap_or_default()
    )
}

// ===== ブロックのテンプレート =====

use hitoiki_core::service::block_template::{
    ApplyResult, Preview, PreviewBlock, Suggestion, TemplateList, TemplateView, UndoResult,
};

fn weekdays_label(w: &[u8]) -> String {
    const N: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];
    w.iter()
        .filter_map(|d| N.get((*d as usize).wrapping_sub(1)).copied())
        .collect::<Vec<_>>()
        .join("")
}

pub fn template_list(l: &TemplateList) -> String {
    if l.templates.is_empty() {
        return "テンプレートはありません。".into();
    }
    let rows: Vec<Vec<String>> = l
        .templates
        .iter()
        .map(|t| {
            vec![
                short(&t.id),
                t.name.clone(),
                weekdays_label(&t.weekdays),
                t.blocks.len().to_string(),
                opt(&t.description),
            ]
        })
        .collect();
    table(&["ID", "名前", "曜日", "ブロック", "説明"], &rows)
}

pub fn template_show(t: &TemplateView) -> String {
    let mut s = format!("{}（{}）", t.name, short(&t.id));
    if !t.weekdays.is_empty() {
        s += &format!(" 曜日: {}", weekdays_label(&t.weekdays));
    }
    if let Some(d) = &t.description {
        s += &format!("\n{d}");
    }
    if t.blocks.is_empty() {
        s += "\n（ブロックなし）";
    }
    for b in &t.blocks {
        s += &format!(
            "\n  {}-{} {}{}",
            b.start,
            b.end,
            b.task_title
                .clone()
                .or_else(|| b.label.clone())
                .unwrap_or_default(),
            if b.focus { "（集中）" } else { "" }
        );
    }
    s
}

pub fn template_saved(t: &TemplateView, verb: &str) -> String {
    format!(
        "テンプレートを{verb}しました: {} ({}ブロック)",
        t.name,
        t.blocks.len()
    )
}

fn preview_line(tz: Tz, p: &PreviewBlock) -> String {
    let mut s = format!(
        "  {}-{} {}{}",
        fmt_hm(tz, p.start_at),
        fmt_hm(tz, p.end_at),
        p.task_title
            .clone()
            .or_else(|| p.label.clone())
            .unwrap_or_default(),
        if p.focus { "（集中）" } else { "" }
    );
    for c in &p.conflicts {
        s += &format!(
            "\n      重なり: {} {}-{} {}",
            if c.kind == "calendar" {
                "予定"
            } else {
                "ブロック"
            },
            fmt_hm(tz, c.start_at),
            fmt_hm(tz, c.end_at),
            c.title
        );
    }
    s
}

pub fn template_preview(tz: Tz, p: &Preview) -> String {
    let mut s = format!(
        "{} を {} に適用すると（重なりは{}）:",
        p.template,
        p.date,
        p.overlap_label()
    );
    if p.blocks.is_empty() {
        s += "\n  （作られるブロックはありません）";
    }
    for b in &p.blocks {
        s += &format!("\n{}", preview_line(tz, b));
    }
    s
}

pub fn template_applied(tz: Tz, r: &ApplyResult) -> String {
    let mut s = format!(
        "{} を {} に適用しました: ブロック {} 件を作成",
        r.template,
        r.date,
        r.created.len()
    );
    if !r.skipped.is_empty() {
        s += &format!("、重なりのため {} 件をスキップ", r.skipped.len());
        for p in &r.skipped {
            s += &format!("\n{}", preview_line(tz, p));
        }
    }
    s
}

pub fn template_undone(u: &UndoResult) -> String {
    format!(
        "{} の適用（{}）を取り消しました: ブロック {} 件を削除",
        u.template, u.date, u.removed
    )
}

pub fn template_suggest(s: &Suggestion) -> String {
    if s.templates.is_empty() {
        return format!("{} の曜日に割り当てたテンプレートはありません。", s.date);
    }
    let names: Vec<_> = s.templates.iter().map(|t| t.name.as_str()).collect();
    format!("{} のパターン: {}", s.date, names.join("、"))
}
