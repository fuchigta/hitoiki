//! 人間向けの出力整形。JSON 出力は service 層の構造体をそのまま使う。

use crate::table::{pad, table, truncate};
use chrono_tz::Tz;
use hitoiki_core::model::{Concern, Condition, Goal, KeyResult, PlanBlock, TaskAssignRule};
use hitoiki_core::repo::segment::kind_label;
use hitoiki_core::service::{
    actual::ActualList,
    correction::{
        ApplyResult as CorrApplied, Candidate, CorrectionList, CorrectionView, Preview as CorrPreview,
        UndoResult as CorrUndone,
    },
    goal::{GoalList, GoalView, KeyResultSummary, LinkResult, LinkedTask},
    heatmap::Heatmap,
    key_result_progress::{KeyResultHistory, ProgressRecorded},
    link_suggestion::{DecisionResult, Suggestions},
    plan::BlockList,
    report::{Report, ReportBy, ReportRow},
    review::{ConfirmBatchResult, ConfirmResult, CreatedRule, ReviewList, RuleSuggestion},
    skill::{PresetImport, SkillList},
    status::Status,
    task::{DeletePreview, DeleteResult, RuleList, TaskList, TaskShow, TaskView},
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
    if let Some(key_result) = &v.key_result {
        s += &format!(
            "成果の指標:       {} / {} ({})\n",
            key_result.goal_title,
            key_result.title,
            short(&key_result.id)
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
        s += &format!("\n\nタスクの割り当てルール:\n{}", rules_table(&v.rules));
    }
    s
}

pub fn task_delete_preview(p: &DeletePreview) -> String {
    format!(
        "削除の影響（実行はしていません）: {} {}
{}",
        short(&p.task_id),
        p.title,
        p.summary
    )
}

pub fn task_deleted(tz: Tz, r: &DeleteResult) -> String {
    let d = &r.deleted;
    let mut s = format!(
        "削除しました: {} {}
{}",
        short(&d.task_id),
        d.title,
        d.summary
    );
    if let Some(range) = &r.range {
        s += &format!(
            "
再計算した範囲: {} - {}",
            dt(tz, range.start_at),
            dt(tz, range.end_at)
        );
    }
    s
}

pub fn rules_table(rules: &[TaskAssignRule]) -> String {
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

pub fn task_rule_list(l: &RuleList) -> String {
    rules_table(&l.rules)
}

fn seg_row(tz: Tz, s: &SegmentView) -> Vec<String> {
    let target = match (&s.task_title, &s.activity) {
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

pub fn actual_review(tz: Tz, l: &ReviewList) -> String {
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

pub fn rule_line(r: &CreatedRule) -> String {
    format!(
        "ルール{}: {} {} {} ({})",
        if r.reused { "(既存)" } else { "を作成" },
        r.rule_type,
        r.field,
        r.pattern,
        r.pattern_type
    )
}

pub fn actual_confirm(tz: Tz, r: &ConfirmResult) -> String {
    let mut s = format!(
        "確定しました:\n{}",
        segments(tz, std::slice::from_ref(&r.segment))
    );
    if let Some(rule) = &r.rule {
        s += &format!("\n{}", rule_line(rule));
    }
    s
}

pub fn actual_batch(tz: Tz, r: &ConfirmBatchResult) -> String {
    let mut s = format!(
        "{} 件を確定しました:
{}",
        r.count,
        segments(tz, &r.segments)
    );
    for rule in &r.rules {
        s += &format!(
            "
{}",
            rule_line(rule)
        );
    }
    s
}

pub fn suggestions(list: &[RuleSuggestion]) -> String {
    if list.is_empty() {
        return "ルール化の提案はありません。".into();
    }
    let mut s = String::from("ルール化の提案（--accept N で追加、--dismiss N で断る）:");
    for (i, r) in list.iter().enumerate() {
        s += &format!(
            "
{}. {} はいつも「{}」にしますか？（{} 回確定）",
            i + 1,
            r.target,
            r.task_title.as_deref().unwrap_or("?"),
            r.count
        );
    }
    s
}

fn uncollected_line(tz: Tz, g: &hitoiki_core::service::actual::UncollectedView) -> String {
    format!(
        "{}-{}  記録なし（収集が止まっていた時間帯・{}）",
        fmt_hm(tz, g.start_at),
        fmt_hm(tz, g.end_at),
        dur(g.duration_ms)
    )
}

pub fn actual_list(tz: Tz, l: &ActualList) -> String {
    let Some(raw) = &l.raw else {
        let mut s = segments(tz, &l.segments);
        for g in &l.uncollected {
            s += &format!(
                "
{}",
                uncollected_line(tz, g)
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
    for g in &l.uncollected {
        lines.push((g.start_at, 1, uncollected_line(tz, g)));
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
        ReportBy::UrgentImportant => "緊急・重要",
        ReportBy::KeyResult => "目標",
        ReportBy::Skill => "スキル",
        ReportBy::Agent => "AI エージェント",
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
    if r.excluded.ignored_ms > 0 {
        s += &format!(
            "

無視した区間（集計に含めない）: {}",
            dur(r.excluded.ignored_ms)
        );
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

/// 活動量ヒートマップ。月曜始まり、7 行 × 週の列。濃淡は `·░▒▓█`（0〜4 段階）。
pub fn heatmap(h: &Heatmap) -> String {
    const SHADES: [char; 5] = ['·', '░', '▒', '▓', '█'];
    let lead = h.from_weekday as usize;
    let weeks = (lead + h.days.len()).div_ceil(7);
    let cell = |week: usize, row: usize| -> char {
        match (week * 7 + row).checked_sub(lead).and_then(|i| h.days.get(i)) {
            Some(d) => SHADES[d.level.min(4) as usize],
            None => ' ',
        }
    };
    let mut s = format!("{}（{} - {}）\n\n", h.label, h.from, h.to);

    // 月ラベル: 月が替わる週の列に出す。前のラベルと重なるときは出さない。
    let mut months = vec![' '; weeks];
    let mut next_free = 0usize;
    let mut prev_month: Option<&str> = None;
    for w in 0..weeks {
        let i = (w * 7).saturating_sub(lead);
        let Some(d) = h.days.get(i) else { continue };
        let month = &d.date[5..7];
        if prev_month != Some(month) {
            prev_month = Some(month);
            let label: Vec<char> = month.trim_start_matches('0').chars().collect();
            if w >= next_free {
                for (k, c) in label.iter().enumerate() {
                    if w + k < weeks {
                        months[w + k] = *c;
                    }
                }
                next_free = w + label.len() + 1;
            }
        }
    }
    s += &format!("   {}\n", months.iter().collect::<String>());
    for (row, name) in ["月", "", "水", "", "金", "", ""].iter().enumerate() {
        let label = if name.is_empty() {
            "  ".to_string()
        } else {
            name.to_string()
        };
        let line: String = (0..weeks).map(|w| cell(w, row)).collect();
        s += &format!("{label} {line}\n");
    }
    s += "\n濃淡: · なし  ░ ▒ ▓ █（値のある日を四分位で 4 段階に分けた相対的な濃さ）\n";
    let fmt = |v: i64| {
        if h.unit == "ms" {
            dur(v)
        } else {
            format!("{v} 件")
        }
    };
    s += &format!("合計: {}\n値のあった日: {} 日", fmt(h.total), h.active_days);
    if h.active_days > 0 {
        s += &format!("\n1 日の最大: {}", fmt(h.max));
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
    if st.today.ignored_ms > 0 {
        s += &format!(
            "無視した区間（集計に含めない）: {}
",
            dur(st.today.ignored_ms)
        );
    }
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
            "（ほかに短い区間 {} 件は既定で非表示）",
            st.needs_review_short_count
        );
    }
    s += "\n";
    if st.unanswered_nudges.is_empty() {
        s += "未応答の問いかけ: なし";
    } else {
        s += "未応答の問いかけ（`hitoiki nudge answer <ID>` で答える／--dismiss で見送る）:";
        for p in &st.unanswered_nudges {
            s += &format!(
                "\n  {} [{}] {} ({})",
                fmt_hm(tz, p.at),
                p.trigger_label,
                truncate(&p.title, 40),
                short(&p.id)
            );
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

pub fn block_added(tz: Tz, b: &PlanBlock) -> String {
    format!(
        "ブロックを追加しました: {} {}-{}{}",
        short(&b.id),
        fmt_hm(tz, b.start_at),
        fmt_hm(tz, b.end_at),
        if b.focus { "（集中）" } else { "" }
    )
}

pub fn condition(c: &Condition) -> String {
    let f = |n: Option<i64>| n.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
    format!(
        "記録しました: {} (エネルギー {} / 気分 {} / ストレス {})",
        short(&c.id),
        f(c.energy),
        f(c.mood),
        f(c.stress)
    )
}

pub fn concern(h: &Concern) -> String {
    format!(
        "気になったことを記録しました: {} (強度 {})",
        short(&h.id),
        h.intensity
    )
}

pub fn goal_list(l: &GoalList) -> String {
    if l.goals.is_empty() {
        return "目標はありません。".into();
    }
    let mut s = String::new();
    for o in &l.goals {
        s += &format!("{} {} [{}]\n", short(&o.goal.id), o.goal.title, o.goal.status);
        for k in &o.key_results {
            s += &format!(
                "  {} {}{} [数え方: {}]\n",
                short(&k.id),
                k.title,
                k.target
                    .as_ref()
                    .map(|t| format!(" (目標値: {t})"))
                    .unwrap_or_default(),
                measure_label(&k.measure)
            );
        }
    }
    s.trim_end().to_string()
}

pub fn goal_show(v: &GoalView) -> String {
    goal_list(&GoalList {
        goals: vec![v.clone()],
    })
}

pub fn goal_added(o: &Goal) -> String {
    format!("目標を追加しました: {} {}", short(&o.id), o.title)
}

pub fn key_result_added(k: &KeyResult) -> String {
    format!("成果の指標を追加しました: {} {}", short(&k.id), k.title)
}

pub fn key_result_suggestions(s: &Suggestions) -> String {
    let title = &s.key_result.title;
    if !s.eligible {
        return format!(
            "成果の指標「{title}」にはすでにスキルか紐付けたタスクがあるため、提案はありません。"
        );
    }
    let mut out = String::new();
    if s.candidates.is_empty() {
        out += &format!("成果の指標「{title}」に紐付ける候補は見つかりませんでした。");
    } else {
        out += &format!("成果の指標「{title}」に紐付けるとよさそうな候補です（自動では紐付けません）。");
        for c in &s.candidates {
            let src = match c.source {
                "both" => "名前の近さ+AI",
                "ai" => "AI",
                _ => "名前の近さ",
            };
            out += &format!("\n  {}:{}  {}  [{}]", c.kind.as_str(), short(&c.id), c.name, src);
        }
        out += &format!(
            "\n付けるには `hitoiki key-result suggest-accept \"{title}\" <候補>`、付けないなら `suggest-reject` を使います。"
        );
    }
    if let Some(n) = &s.ai.note {
        out += &format!("\n（{n}）");
    }
    out
}

pub fn key_result_suggest_decided(r: &DecisionResult, accepted: bool) -> String {
    let mut out = if accepted {
        format!("成果の指標「{}」に付けました。", r.key_result.title)
    } else {
        format!(
            "成果の指標「{}」には付けないことにしました（以後提案しません）。",
            r.key_result.title
        )
    };
    for i in &r.items {
        out += &format!("\n  {} {} {}", i.kind.label(), short(&i.id), i.name);
    }
    out
}

fn measure_label(m: &str) -> &'static str {
    match m {
        "active_days" => "取り組んだ日数",
        "done_tasks" => "完了したタスク数",
        _ => "時間",
    }
}

pub fn key_result_linked(r: &LinkResult) -> String {
    let mut s = format!(
        "{} 件のタスクを成果の指標「{}」に紐付けました。",
        r.linked.len(),
        r.key_result.title
    );
    for t in &r.linked {
        s += &format!("\n  {} {}", short(&t.id), t.title);
        if let Some(p) = &t.previous_key_result {
            s += &format!("（成果の指標「{}」から付け替え）", p.title);
        }
    }
    s
}

pub fn key_result_unlinked(l: &[LinkedTask]) -> String {
    if l.is_empty() {
        return "外したタスクはありません（成果の指標に紐付いていませんでした）。".into();
    }
    let mut s = format!("{} 件のタスクの成果の指標との紐付けを外しました。", l.len());
    for t in l {
        s += &format!("\n  {} {}", short(&t.id), t.title);
        if let Some(p) = &t.previous_key_result {
            s += &format!("（成果の指標「{}」）", p.title);
        }
    }
    s
}

/// 成果の指標の集計。選んだ数え方の値を先頭に、残りは補助として並べる。評価や達成率は出さない。
pub fn key_result_summary(s: &KeyResultSummary) -> String {
    let k = &s.key_result;
    let mut out = format!(
        "{} / {} ({})\n期間: {} 〜 {}（{}）\n",
        s.goal.title,
        k.title,
        short(&k.id),
        s.range.start,
        s.range.end,
        match s.range_source {
            "goal" => "目標の期間",
            "given" => "指定",
            _ => "直近 8 週",
        }
    );
    if k.target.is_some() || k.current.is_some() {
        out += &format!(
            "目標値: {}  現在値: {}\n",
            k.target.as_deref().unwrap_or("-"),
            k.current.as_deref().unwrap_or("-")
        );
    }
    let time = format!("時間 {}", dur(s.time_ms));
    let days = format!(
        "取り組んだ日 {} 日（1 日 {} 分以上）",
        s.active_days, s.active_day_min
    );
    let done = format!("完了 {} / {} 件", s.done_count, s.task_count);
    let (main, others) = match s.measure.as_str() {
        "active_days" => (days, [time, done]),
        "done_tasks" => (done, [time, days]),
        _ => (time, [days, done]),
    };
    out += &format!(
        "\n数え方（{}）: {}\n  補助: {} ・ {}\n",
        measure_label(&s.measure),
        main,
        others[0],
        others[1]
    );
    // 内訳は小さく添えるだけ。時間・日数は両方を重ならないように合算した値。
    if !s.key_result_skills.is_empty() || s.from_skill_ms > 0 {
        let names = if s.key_result_skills.is_empty() {
            "(なし)".to_string()
        } else {
            s.key_result_skills.join("、")
        };
        out += &format!(
            "成果の指標のスキル: {}（子スキルを含む）\n内訳: スキルから {} ・ 紐付いたタスクから {}（両方に当てはまる区間はスキルから）\n",
            names,
            dur(s.from_skill_ms),
            dur(s.from_task_ms)
        );
    }
    if s.estimate.estimated_tasks > 0 {
        out += &format!(
            "見積の合計 {}分（{} 件）・ そのタスクの期間内の実績 {}\n",
            s.estimate.estimate_min_total,
            s.estimate.estimated_tasks,
            dur(s.estimate.actual_ms_of_estimated)
        );
    }
    let max = s.weekly.iter().map(|w| w.duration_ms).max().unwrap_or(0);
    if max > 0 {
        out += "\n週ごとの時間（月曜始まり）\n";
        for w in &s.weekly {
            let n = (w.duration_ms * 20 / max) as usize;
            out += &format!(
                "  {} {:<20} {}\n",
                w.week_start,
                "█".repeat(n),
                dur(w.duration_ms)
            );
        }
    }
    if !s.skills.is_empty() {
        out += "\nスキル別（複数のスキルが付いたタスクは各スキルに全時間を計上）\n";
        for k in &s.skills {
            out += &format!("  {}  {}\n", dur(k.duration_ms), k.name);
        }
    }
    if s.tasks.is_empty() {
        out += "\n紐付いたタスクはありません。スキルは `hitoiki key-result edit --skill`、タスクは `hitoiki key-result link` で追加できます。";
    } else {
        out += "\n紐付いたタスク\n";
        for t in &s.tasks {
            out += &format!(
                "  {} [{}] {}  {}\n",
                short(&t.id),
                t.status,
                t.title,
                dur(t.tracked_ms)
            );
        }
    }
    out.trim_end().to_string()
}

fn progress_value(text: Option<&str>) -> &str {
    text.unwrap_or("（消した）")
}

pub fn key_result_progress_recorded(tz: Tz, r: &ProgressRecorded) -> String {
    let e = &r.entry;
    let mut s = format!(
        "現在値を記録しました: {} {}  {} ({})",
        short(&r.key_result.id),
        r.key_result.title,
        progress_value(e.text.as_deref()),
        dt(tz, e.at)
    );
    if let Some(n) = &e.note {
        s += &format!("\n  メモ: {n}");
    }
    s
}

/// 成果の指標の現在値の履歴。週ごとに「その週の終わり時点の現在値」と時間を並べるだけで、評価は付けない。
pub fn key_result_history(tz: Tz, h: &KeyResultHistory) -> String {
    let k = &h.key_result;
    let mut out = format!(
        "{} ({})\n期間: {} 〜 {}（{}）\n",
        k.title,
        short(&k.id),
        h.range.start,
        h.range.end,
        match h.range_source {
            "goal" => "目標の期間",
            "given" => "指定",
            _ => "直近 8 週",
        }
    );
    if let Some(t) = &k.target {
        out += &format!("目標値: {t}\n");
    }
    out += "\n週（月曜始まり）  現在値        時間\n";
    for w in &h.weeks {
        let v = truncate(w.text.as_deref().unwrap_or("-"), 12);
        out += &format!("  {}  {}  {}\n", w.week_start, pad(&v, 12), dur(w.duration_ms));
    }
    if h.entries.is_empty() {
        out += "\n値の履歴はまだありません。`hitoiki key-result progress` で記録できます。";
    } else {
        out += "\n値の履歴\n";
        for e in &h.entries {
            out += &format!("  {}  {}", dt(tz, e.at), progress_value(e.text.as_deref()));
            if let Some(n) = &e.note {
                out += &format!("  {n}");
            }
            out += "\n";
        }
    }
    out.trim_end().to_string()
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

/// 回答済みの問いに保存した記録の中身を「ラベル: 値」の行にする。
fn detail_lines(d: &hitoiki_core::service::reflect::AnswerDetail) -> Vec<String> {
    let mut v = Vec::new();
    let mut add = |label: &str, val: Option<String>| {
        if let Some(x) = val.filter(|x| !x.trim().is_empty()) {
            v.push(format!("{label}: {x}"));
        }
    };
    if let Some(c) = &d.condition {
        add("エネルギー", c.energy.map(|n| n.to_string()));
        add("気分", c.mood.map(|n| n.to_string()));
        add("ストレス", c.stress.map(|n| n.to_string()));
        add("ERG タグ", c.erg_tag.map(|t| t.to_string()));
        add("メモ", c.note.clone());
    }
    if let Some(h) = &d.concern {
        add("気になったこと", Some(h.text.clone()));
        add("強さ", Some(h.intensity.to_string()));
    }
    if let Some(a) = &d.reframe {
        add("A（出来事）", Some(a.event.clone()));
        add("B（受け止め方）", a.belief.clone());
        add("C（感情）", a.emotion.clone());
        add("感情の強さ", a.emotion_intensity.map(|n| n.to_string()));
        add("別の捉え方", a.alternative.clone());
    }
    v
}

pub fn reflection(
    tz: Tz,
    r: &hitoiki_core::model::Reflection,
    details: &[hitoiki_core::service::reflect::AnswerDetail],
) -> String {
    use hitoiki_core::service::reflect::{structured_of, AnswersRecord};
    use hitoiki_core::service::structured::{QuestionKind, Scale};
    let head = format!("# 振り返り {} ({}, {})\n\n", short(&r.id), r.kind, dt(tz, r.at));
    // 構造化されていない（または読めない）場合は従来の Markdown を出す。
    let Some(out) = structured_of(r).ok().flatten() else {
        let mut s = head + &r.questions_md;
        if let Some(a) = &r.answers_md {
            s += &format!("\n\n## 回答\n\n{a}");
        }
        return s;
    };
    let answered: AnswersRecord = r
        .answers_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    let mut s = head;
    if let Some(sum) = &out.summary {
        s += &format!("## 要約\n\n{sum}\n\n");
    }
    s += "## 問い\n";
    for (i, q) in out.questions.iter().enumerate() {
        let mark = if answered.items.iter().any(|a| a.index == i) {
            "回答済"
        } else {
            "未回答"
        };
        s += &format!("\n[{i}] ({}・{mark}) {}\n", q.kind.label(), q.prompt);
        match q.kind {
            QuestionKind::Condition => {
                let names: Vec<&str> = q.scales.iter().map(Scale::label).collect();
                s += &format!("    尺度: {}（1〜5）\n", names.join("・"));
            }
            QuestionKind::Reframe => {
                if let Some(a) = &q.event {
                    s += &format!("    A（出来事）の候補: {a}\n");
                }
                if let Some(b) = &q.belief_prompt {
                    s += &format!("    B を探る問い: {b}\n");
                }
            }
            _ => {}
        }
        if !q.examples.is_empty() {
            s += &format!("    例: {}\n", q.examples.join(" / "));
        }
    }
    if answered.items.is_empty() {
        s += &format!(
            "\n回答するには `hitoiki reflect answer {} --file F`（`-` で標準入力）に JSON を渡します。\
             例: {{\"answers\":[{{\"index\":0,\"energy\":3,\"mood\":4}},{{\"index\":1,\"text\":\"...\",\"intensity\":3}}]}}\n",
            short(&r.id)
        );
    } else {
        s += "\n## 回答\n";
        for a in &answered.items {
            let label = out.questions[a.index.min(out.questions.len() - 1)].kind.label();
            match (&a.record_id, &a.text) {
                (Some(id), _) => {
                    s += &format!("\n[{}] {label} → 記録 {}", a.index, short(id));
                    if let Some(d) = details.iter().find(|d| d.index == a.index) {
                        if d.missing {
                            s += "（この回答の記録は削除されています）";
                        }
                        for l in detail_lines(d) {
                            s += &format!("\n    {l}");
                        }
                    }
                }
                (None, Some(t)) => s += &format!("\n[{}] {label}: {t}", a.index),
                _ => {}
            }
        }
        s += "\n";
    }
    s.trim_end().to_string()
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
                if r.answered() { "回答済" } else { "-" }.to_string(),
            ]
        })
        .collect::<Vec<_>>();
    table(&["ID", "日時", "種別", "回答"], &rows)
}

pub fn reframe_deepen(a: &hitoiki_core::model::Reframe) -> String {
    format!(
        "捉え直し {} の問いを追記しました。\n\n{}",
        short(&a.id),
        a.deepening_md.as_deref().unwrap_or_default()
    )
}

// ===== テンプレート =====

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

// ===== 実績の補正 =====

fn candidate_row(tz: Tz, c: &Candidate) -> Vec<String> {
    let what = if c.uncollected {
        "記録なし".to_string()
    } else {
        let kind = c.kind.map(kind_label).unwrap_or("-");
        match (&c.task_title, &c.activity) {
            (Some(t), Some(a)) => format!("{kind} {} / {a}", truncate(t, 24)),
            (Some(t), None) => format!("{kind} {}", truncate(t, 24)),
            (None, Some(a)) => format!("{kind} / {a}"),
            (None, None) => kind.to_string(),
        }
    };
    vec![
        if c.checked { "[x]".into() } else { "[ ]".into() },
        if c.uncollected { c.id.clone() } else { short(&c.id) },
        format!("{}-{}", fmt_hm(tz, c.start_at), fmt_hm(tz, c.end_at)),
        dur(c.duration_ms),
        what,
        if c.becomes_off_pc {
            "PC 外".into()
        } else {
            String::new()
        },
        c.status.map(|s| s.to_string()).unwrap_or_default(),
    ]
}

pub fn correction_preview(tz: Tz, p: &CorrPreview) -> String {
    let rows: Vec<Vec<String>> = p.candidates.iter().map(|c| candidate_row(tz, c)).collect();
    let body = if rows.is_empty() {
        "範囲に含まれる区間はありません。".to_string()
    } else {
        table(&["", "ID", "時刻", "長さ", "内容", "", "状態"], &rows)
    };
    format!(
        "{body}
{}
（確定していません。--dry-run を外すと適用します）",
        p.summary
    )
}

pub fn correction_applied(tz: Tz, r: &CorrApplied) -> String {
    format!(
        "補正しました（ID {}）: {}
取り消すには `hitoiki actual undo-correction {}`",
        short(&r.correction.id),
        r.preview.summary,
        short(&r.correction.id)
    ) + &format!(
        "
対象 {} 件、{} 〜 {}",
        r.preview.selected_count,
        dt(tz, r.correction.start_at),
        fmt_hm(tz, r.correction.end_at)
    )
}

fn correction_target(c: &CorrectionView) -> String {
    let base = match (&c.task_title, c.kind) {
        (Some(t), _) => t.clone(),
        (None, Some(_)) => "休憩".to_string(),
        (None, None) => "タスクなし".to_string(),
    };
    match &c.activity {
        Some(a) => format!("{base} / {a}"),
        None => base,
    }
}

pub fn corrections(tz: Tz, l: &CorrectionList) -> String {
    if l.corrections.is_empty() {
        return "補正はありません。".into();
    }
    let rows: Vec<Vec<String>> = l
        .corrections
        .iter()
        .map(|c| {
            vec![
                short(&c.id),
                format!("{} - {}", dt(tz, c.start_at), fmt_hm(tz, c.end_at)),
                dur(c.total_ms),
                if c.off_pc_ms > 0 {
                    dur(c.off_pc_ms)
                } else {
                    String::new()
                },
                c.segment_count.to_string(),
                truncate(&correction_target(c), 40),
            ]
        })
        .collect();
    table(&["ID", "時間帯", "合計", "PC 外", "区間", "割り当て先"], &rows)
}

pub fn correction_undone(tz: Tz, r: &CorrUndone) -> String {
    format!(
        "補正を取り消しました: {} {}（確定 {} 件を削除して再計算）",
        short(&r.correction.id),
        dt(tz, r.correction.start_at),
        r.confirmations_removed
    )
}

pub fn ignore_list(l: &hitoiki_core::service::ignore::IgnoreList) -> String {
    let mut s = format!(
        "無視するアプリ（collect.ignore_exes、{}）:",
        if l.customized { "変更済み" } else { "既定" }
    );
    for e in &l.exes {
        s += &format!(
            "
  {e}"
        );
    }
    s
}

pub fn ignore_change(c: &hitoiki_core::service::ignore::IgnoreChange, added: bool) -> String {
    let verb = if added {
        "無視リストに追加"
    } else {
        "無視リストから削除"
    };
    if !c.changed {
        return format!("{} はすでに無視リストにあります。", c.exe);
    }
    let days = hitoiki_core::service::ignore::RECOMPUTE_DAYS;
    format!("{}: {}（直近 {} 日を再計算しました）", verb, c.exe, days)
}

pub fn ignore_suggestions(list: &[hitoiki_core::service::ignore::IgnoreSuggestion], days: i64) -> String {
    if list.is_empty() {
        return format!("無視の提案はありません（直近 {days} 日）。");
    }
    let mut s =
        format!("無視の提案（直近 {days} 日。--accept <exe> で無視リストに追加、--dismiss <exe> で断る）:");
    for r in list {
        s += &format!(
            "
- {}（要確認 {} 件）",
            r.summary, r.review_count
        );
    }
    s
}

pub fn unignored(r: &hitoiki_core::service::review::UnignoreResult) -> String {
    format!(
        "{} 件の無視を取り消しました（確定 {} 件を削除して再計算）。",
        r.count, r.confirmations_removed
    )
}

/// `hitoiki nudge list` の人間向け表示。
pub fn nudge_list(tz: Tz, l: &hitoiki_core::service::nudgelog::NudgeList) -> String {
    use hitoiki_core::service::nudgelog::NudgeState;
    if l.nudges.is_empty() {
        return "通知はありません".into();
    }
    let mut out = Vec::new();
    for p in &l.nudges {
        let state = match p.state {
            NudgeState::Unanswered => "未回答",
            NudgeState::Answered => "回答済み",
            NudgeState::Dismissed => "見送り",
        };
        let mut line = format!(
            "{} {} [{}] {} ({})",
            fmt_hm(tz, p.at),
            state,
            p.trigger_label,
            p.title,
            short(&p.id)
        );
        if let Some((a, b)) = p.payload.as_ref().and_then(|x| x.idle_range) {
            line += &format!("\n    対象: {}〜{}", fmt_hm(tz, a), fmt_hm(tz, b));
        }
        if !p.body.is_empty() {
            line += &format!("\n    {}", p.body);
        }
        out.push(line);
    }
    out.join("\n")
}

/// `hitoiki nudge answer` の人間向け表示。
pub fn nudge_answered(r: &hitoiki_core::service::nudgelog::AnswerResult) -> String {
    use hitoiki_core::service::nudgelog::NudgeState;
    match r.state {
        NudgeState::Dismissed => format!("見送りました: {}", short(&r.id)),
        _ => {
            let mut parts = Vec::new();
            if r.condition_id.is_some() {
                parts.push("調子".to_string());
            }
            if r.concern_id.is_some() {
                parts.push("気になったこと".to_string());
            }
            if r.idle_labeled > 0 {
                parts.push(format!("無操作の分類（{} 区間）", r.idle_labeled));
            }
            if r.started_task.is_some() {
                parts.push("タスクを開始".to_string());
            }
            format!("答えました: {} ({})", short(&r.id), parts.join("・"))
        }
    }
}

fn erg_name(t: hitoiki_core::model::ErgChoice) -> &'static str {
    use hitoiki_core::model::ErgChoice::*;
    match t {
        Existence => "存在",
        Relatedness => "関係",
        Growth => "成長",
        None => "なし",
    }
}

fn erg_source(s: hitoiki_core::model::ErgSource) -> &'static str {
    use hitoiki_core::model::ErgSource::*;
    match s {
        User => "ユーザー",
        Ai => "AI",
        Unconfirmed => "未確定",
    }
}

fn erg_conf(c: Option<f64>) -> String {
    c.map(|v| format!("{v:.2}")).unwrap_or_else(|| "-".into())
}

/// `hitoiki erg classify` の人間向け表示。
pub fn erg_classify(tz: Tz, o: &hitoiki_core::service::erg::ClassifyOutcome) -> String {
    if let Some(why) = &o.skipped {
        return why.clone();
    }
    if o.targets == 0 {
        return "判定する記録はありません（未判定のものがありません）。".into();
    }
    let mut s = format!(
        "{}対象 {} 件: AI の判定を採用 {} 件 / 未確定 {} 件{}\n",
        if o.dry_run {
            "（dry-run・保存していません）"
        } else {
            ""
        },
        o.targets,
        o.adopted,
        o.unconfirmed,
        if o.unread > 0 {
            format!(" / 判定が得られず未判定のまま {} 件", o.unread)
        } else {
            String::new()
        }
    );
    if !o.items.is_empty() {
        let rows: Vec<Vec<String>> = o
            .items
            .iter()
            .map(|i| {
                vec![
                    short(&i.record_id),
                    dt(tz, i.at),
                    erg_name(i.tag).to_string(),
                    erg_source(i.source).to_string(),
                    erg_conf(i.confidence),
                    truncate(&i.excerpt, 30),
                    truncate(&i.reason, 30),
                ]
            })
            .collect();
        s += &table(&["ID", "時刻", "ERG", "出所", "確信度", "記録", "理由"], &rows);
    }
    s
}

/// `hitoiki erg review` の人間向け表示。
pub fn erg_review(tz: Tz, r: &hitoiki_core::service::erg::ErgReview) -> String {
    if r.items.is_empty() {
        return "未確定はありません。".into();
    }
    let rows: Vec<Vec<String>> = r
        .items
        .iter()
        .map(|i| {
            vec![
                short(&i.label.record_id),
                i.label.record_kind.to_string(),
                dt(tz, i.record_at),
                erg_name(i.label.tag).to_string(),
                erg_conf(i.label.confidence),
                truncate(&i.excerpt, 30),
                truncate(&opt(&i.label.reason), 30),
            ]
        })
        .collect();
    let mut s = table(&["ID", "種類", "時刻", "提案", "確信度", "記録", "理由"], &rows);
    s += "\n\n`hitoiki erg confirm <ID>` で提案のまま確定、`hitoiki erg set <ID> <existence|relatedness|growth|none>` で変えられます。";
    s
}

/// `hitoiki erg set` / `confirm` の人間向け表示。
pub fn erg_label(l: &hitoiki_core::model::ErgLabel) -> String {
    format!(
        "ERG を確定しました: {} {} → {}",
        l.record_kind,
        short(&l.record_id),
        erg_name(l.tag)
    )
}

/// `hitoiki erg show` の人間向け表示。
pub fn erg_overview(tz: Tz, o: &hitoiki_core::service::erg::ErgOverview) -> String {
    let d = &o.distribution;
    let mut s = format!(
        "{} 〜 {} の記録 {} 件\n  存在 {} / 関係 {} / 成長 {} / どれにも関わらない {}\n  未確定 {} / 未判定 {}（うち AI の判定を採用 {}）\n  （{}）",
        dt(tz, o.range.start_at),
        dt(tz, o.range.end_at),
        d.total,
        d.existence,
        d.relatedness,
        d.growth,
        d.none,
        d.unconfirmed,
        d.unlabeled,
        d.ai_decided,
        d.legend
    );
    if o.unconfirmed_total > 0 {
        s += &format!(
            "\n未確定は全期間で {} 件あります（`hitoiki erg review`）。",
            o.unconfirmed_total
        );
    }
    s
}
