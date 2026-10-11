//! 無視するアプリ（`collect.ignore_exes`）の管理と、無視の提案（docs/dev/architecture.md「無視するアプリ」）。
//!
//! 設定ファイルの該当キーだけを書き換え、直近の期間を再計算する。生データは変えない。
//! 提案は直近の `window_span` と `activity_segment` から、無視の候補を理由付きで返す。

use super::{range_view, Ctx, RangeView};
use crate::config::{self, Config};
use crate::model::{SegmentKind, SegmentStatus};
use crate::pipeline::{SegmentJudge, BROWSER_EXES};
use crate::timefmt::Range;
use crate::{repo, Error, Millis, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// 追加・削除のあとに再計算する直近の日数。
pub const RECOMPUTE_DAYS: i64 = 7;
/// 提案の既定の遡り日数。
pub const SUGGEST_DAYS: i64 = 7;
/// 提案を断ったあと、同じアプリを再び提案しない日数。
pub const DISMISS_DAYS: i64 = 30;
/// 「短い区間」とみなす長さ。
pub const SHORT_MS: Millis = 10_000;
/// 短い区間の割合で提案する、最小の区間数と割合。
const FLICKER_MIN_SPANS: u32 = 20;
const FLICKER_RATIO: f64 = 0.7;
/// 既知のシステム系 exe で提案する最小の区間数。
const SYSTEM_MIN_SPANS: u32 = 3;
/// 記録時間が長いシステム系とみなす合計。
const SYSTEM_LONG_MS: Millis = 30 * 60_000;
/// 要確認に何度も出ているとみなす件数と、その平均の長さの上限。
const REVIEW_MIN_COUNT: u32 = 10;
const REVIEW_MAX_AVG_MS: Millis = 60_000;
/// ユーザーが仕分けで「無視」「タスクなし」にした回数の下限。
const USER_IGNORED_MIN: u32 = 2;
const USER_NO_TASK_MIN: u32 = 5;

const DISMISSED_KEY: &str = "review.ignore_suggest_dismissed";

/// 既定の無視リストに無い、既知のシステム系 exe（`ApplicationFrameHost.exe` は UWP の実体なので含めない）。
const SYSTEM_EXES: &[&str] = &[
    "explorer.exe",
    "ShellExperienceHost.exe",
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "SearchApp.exe",
    "SearchUI.exe",
    "ShellHost.exe",
    "LockApp.exe",
    "TextInputHost.exe",
    "dwm.exe",
    "LogonUI.exe",
    "SecurityHealthSystray.exe",
    "ScreenClippingHost.exe",
    "SndVol.exe",
    "CredentialUIBroker.exe",
    "consent.exe",
    "sihost.exe",
    "RuntimeBroker.exe",
    "Widgets.exe",
    "WidgetService.exe",
    "msrdc.exe",
    "msrdcw.exe",
    "CompPkgSrv.exe",
    "PickerHost.exe",
    "ctfmon.exe",
    "taskhostw.exe",
];

/// 無視リストの現在値。
#[derive(Debug, Clone, Serialize)]
pub struct IgnoreList {
    pub exes: Vec<String>,
    /// 既定の無視リスト。
    pub default_exes: Vec<String>,
    /// 既定から変更されているか。
    pub customized: bool,
}

pub fn list(cfg: &Config) -> IgnoreList {
    let default_exes = Config::default().collect.ignore_exes;
    IgnoreList {
        customized: cfg.collect.ignore_exes != default_exes,
        exes: cfg.collect.ignore_exes.clone(),
        default_exes,
    }
}

/// 追加・削除の結果。
#[derive(Debug, Clone, Serialize)]
pub struct IgnoreChange {
    pub exe: String,
    /// 設定を書き換えたか（すでに入っている・入っていないときは false）。
    pub changed: bool,
    pub exes: Vec<String>,
    /// 再計算した範囲（変更が無ければ None）。
    pub recomputed: Option<RangeView>,
}

/// exe 名の正規化。パスが付いていればファイル名だけにする。
fn normalize(exe: &str) -> Result<String> {
    let name = exe.trim().rsplit(['\\', '/']).next().unwrap_or("").trim();
    if name.is_empty() {
        return Err(Error::InvalidArgument("exe 名を指定してください".into()));
    }
    Ok(name.to_string())
}

fn write_list(path: &Path, exes: &[String]) -> Result<()> {
    let arr = toml::Value::Array(exes.iter().cloned().map(toml::Value::String).collect());
    config::set_value(path, "collect.ignore_exes", &arr.to_string())
}

/// 設定を読み直し、直近 [`RECOMPUTE_DAYS`] 日を再計算する。
fn recompute(ctx: &Ctx, path: &Path, judge: Option<&dyn SegmentJudge>) -> Result<RangeView> {
    let cfg = Config::load_from(path)?;
    let tz = ctx.tz()?;
    let now = ctx.now();
    let range = Range::new(now - RECOMPUTE_DAYS * 86_400_000, now);
    let stats = crate::rebuild::rebuild_with_judge(ctx.conn, &cfg, range, now, judge)?;
    Ok(range_view(tz, stats.range))
}

/// 無視リストにアプリを追加する（既存の値に追記。既定のままなら既定＋追加の形で書く）。
/// 変更したら直近 [`RECOMPUTE_DAYS`] 日を再計算する。`config_path` は設定ファイル。
pub fn add(
    ctx: &Ctx,
    config_path: &Path,
    exe: &str,
    judge: Option<&dyn SegmentJudge>,
) -> Result<IgnoreChange> {
    let exe = normalize(exe)?;
    let mut exes = Config::load_from(config_path)?.collect.ignore_exes;
    if exes.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
        return Ok(IgnoreChange {
            exe,
            changed: false,
            exes,
            recomputed: None,
        });
    }
    exes.push(exe.clone());
    write_list(config_path, &exes)?;
    let recomputed = Some(recompute(ctx, config_path, judge)?);
    Ok(IgnoreChange {
        exe,
        changed: true,
        exes,
        recomputed,
    })
}

/// 無視リストからアプリを外す。変更したら直近 [`RECOMPUTE_DAYS`] 日を再計算し、元に戻す。
pub fn remove(
    ctx: &Ctx,
    config_path: &Path,
    exe: &str,
    judge: Option<&dyn SegmentJudge>,
) -> Result<IgnoreChange> {
    let exe = normalize(exe)?;
    let mut exes = Config::load_from(config_path)?.collect.ignore_exes;
    let before = exes.len();
    exes.retain(|e| !e.eq_ignore_ascii_case(&exe));
    if exes.len() == before {
        return Err(Error::NotFound(format!("無視リストの exe: {exe}")));
    }
    write_list(config_path, &exes)?;
    let recomputed = Some(recompute(ctx, config_path, judge)?);
    Ok(IgnoreChange {
        exe,
        changed: true,
        exes,
        recomputed,
    })
}

// ===== 無視の提案 =====

#[derive(Debug, Clone, Serialize)]
pub struct IgnoreReason {
    /// `system` | `flicker` | `review` | `user_ignored` | `user_no_task`
    pub code: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IgnoreSuggestion {
    pub exe: String,
    pub reasons: Vec<IgnoreReason>,
    /// 直近の前面ウインドウの区間数（`window_span`）。
    pub span_count: u32,
    pub total_ms: Millis,
    /// 10 秒未満の区間数と割合。
    pub short_count: u32,
    pub short_ratio: f64,
    /// 要確認の区間数。
    pub review_count: u32,
    /// ユーザーが「無視」にした回数、「タスクなし」にした回数。
    pub user_ignored_count: u32,
    pub user_no_task_count: u32,
    /// 表示用の説明。
    pub summary: String,
}

#[derive(Debug, Default)]
struct Stat {
    exe: String,
    spans: u32,
    total: Millis,
    short: u32,
    review: u32,
    review_ms: Millis,
    user_ignored: u32,
    user_no_task: u32,
}

fn fmt_total(ms: Millis) -> String {
    if ms < 60_000 {
        format!("{} 秒", ms / 1000)
    } else {
        format!("{:.1} 分", ms as f64 / 60_000.0)
    }
}

fn in_list(list: &[String], exe: &str) -> bool {
    list.iter().any(|e| e.eq_ignore_ascii_case(exe))
}

/// 作業の本体になりやすいアプリ（提案しない）。
fn is_work_app(cfg: &Config, exe: &str) -> bool {
    let c = &cfg.collect;
    in_list(&c.terminal_exes, exe)
        || in_list(&c.agent_host_exes, exe)
        || in_list(&c.communication_exes, exe)
        || in_list(&c.meeting_exes, exe)
        || BROWSER_EXES.iter().any(|b| b.eq_ignore_ascii_case(exe))
        || exe.eq_ignore_ascii_case("hitoiki-app.exe")
        || exe.eq_ignore_ascii_case("ApplicationFrameHost.exe")
}

fn load_dismissed(conn: &rusqlite::Connection) -> Result<BTreeMap<String, Millis>> {
    Ok(super::update::get(conn, DISMISSED_KEY)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

/// 直近 `days` 日の記録から、無視の候補を理由付きで返す（多い順）。
/// すでに無視リストにあるもの、作業アプリ、断って [`DISMISS_DAYS`] 日以内のものは除く。
pub fn suggest(ctx: &Ctx, days: i64) -> Result<Vec<IgnoreSuggestion>> {
    if days <= 0 {
        return Err(Error::InvalidArgument(
            "--days は 1 以上で指定してください".into(),
        ));
    }
    let now = ctx.now();
    let range = Range::new(now - days * 86_400_000, now);
    let dismissed = load_dismissed(ctx.conn)?;
    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    for w in repo::raw::window_spans(ctx.conn, range)? {
        let Some(exe) = w.exe_name.as_deref().filter(|e| !e.is_empty()) else {
            continue;
        };
        if w.excluded {
            continue;
        }
        let ms = (w.end_at.unwrap_or(now).min(range.end) - w.start_at.max(range.start)).max(0);
        if ms == 0 {
            continue;
        }
        let e = stats.entry(exe.to_ascii_lowercase()).or_default();
        if e.exe.is_empty() {
            e.exe = exe.to_string();
        }
        e.spans += 1;
        e.total += ms;
        if ms < SHORT_MS {
            e.short += 1;
        }
    }
    for s in repo::segment::list(ctx.conn, range)? {
        let Some(exe) = s.exe_name.as_deref().filter(|e| !e.is_empty()) else {
            continue;
        };
        let ms = s.end_at - s.start_at;
        let e = stats.entry(exe.to_ascii_lowercase()).or_default();
        if e.exe.is_empty() {
            e.exe = exe.to_string();
        }
        let user = s.decided_by.as_deref() == Some("user") && s.status == SegmentStatus::Confirmed;
        match s.kind {
            SegmentKind::Work | SegmentKind::Interrupt if s.status == SegmentStatus::NeedsReview => {
                e.review += 1;
                e.review_ms += ms;
            }
            SegmentKind::Ignored if user => e.user_ignored += 1,
            SegmentKind::Work
                if user && s.task_id.is_none() && !s.offline && s.agent_session_id.is_none() =>
            {
                e.user_no_task += 1
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    for (key, st) in stats {
        let exe = st.exe.clone();
        if in_list(&ctx.cfg.collect.ignore_exes, &exe)
            || is_work_app(ctx.cfg, &exe)
            || dismissed.get(&key).is_some_and(|&until| until > now)
        {
            continue;
        }
        let ratio = if st.spans > 0 {
            f64::from(st.short) / f64::from(st.spans)
        } else {
            0.0
        };
        let mut reasons = Vec::new();
        if in_list(
            &SYSTEM_EXES.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &exe,
        ) && st.spans >= SYSTEM_MIN_SPANS
        {
            let text = if st.total >= SYSTEM_LONG_MS {
                format!("システム系のアプリで、記録時間が長い（{}）", fmt_total(st.total))
            } else {
                "システム系のアプリ".to_string()
            };
            reasons.push(IgnoreReason { code: "system", text });
        }
        if st.spans >= FLICKER_MIN_SPANS && ratio >= FLICKER_RATIO {
            reasons.push(IgnoreReason {
                code: "flicker",
                text: format!(
                    "10 秒未満の区間が {} 件（{}%）",
                    st.short,
                    (ratio * 100.0).round() as u32
                ),
            });
        }
        if st.review >= REVIEW_MIN_COUNT && st.review_ms / i64::from(st.review) < REVIEW_MAX_AVG_MS {
            reasons.push(IgnoreReason {
                code: "review",
                text: format!("要確認に短い区間が {} 件出ている", st.review),
            });
        }
        if st.user_ignored >= USER_IGNORED_MIN {
            reasons.push(IgnoreReason {
                code: "user_ignored",
                text: format!("仕分けで {} 回「無視」にした", st.user_ignored),
            });
        }
        if st.user_no_task >= USER_NO_TASK_MIN {
            reasons.push(IgnoreReason {
                code: "user_no_task",
                text: format!("仕分けで {} 回「タスクなし」にした", st.user_no_task),
            });
        }
        if reasons.is_empty() {
            continue;
        }
        let summary = format!(
            "{exe} は区間が {} 件（合計 {}）、うち 10 秒未満が {} 件。{}",
            st.spans,
            fmt_total(st.total),
            st.short,
            reasons
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("、")
        );
        out.push(IgnoreSuggestion {
            exe,
            reasons,
            span_count: st.spans,
            total_ms: st.total,
            short_count: st.short,
            short_ratio: ratio,
            review_count: st.review,
            user_ignored_count: st.user_ignored,
            user_no_task_count: st.user_no_task,
            summary,
        });
    }
    out.sort_by(|a, b| {
        b.reasons
            .len()
            .cmp(&a.reasons.len())
            .then_with(|| b.span_count.cmp(&a.span_count))
            .then_with(|| a.exe.cmp(&b.exe))
    });
    Ok(out)
}

/// 提案を断る。同じアプリは [`DISMISS_DAYS`] 日のあいだ提案しない。
pub fn dismiss(ctx: &Ctx, exe: &str) -> Result<()> {
    let exe = normalize(exe)?;
    let now = ctx.now();
    let mut map = load_dismissed(ctx.conn)?;
    map.retain(|_, until| *until > now);
    map.insert(exe.to_ascii_lowercase(), now + DISMISS_DAYS * 86_400_000);
    super::update::put(
        ctx.conn,
        DISMISSED_KEY,
        &serde_json::to_string(&map).map_err(anyhow::Error::from)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::new_id;
    use crate::service::testutil::{insert_segment, Env};

    const MIN: Millis = 60_000;

    fn span(env: &Env, exe: &str, start: Millis, len: Millis) {
        env.conn
            .execute(
                "INSERT INTO window_span (id, start_at, end_at, exe_name, title, excluded) VALUES (?1,?2,?3,?4,'t',0)",
                rusqlite::params![new_id(), start, start + len, exe],
            )
            .unwrap();
    }

    fn spans(env: &Env, exe: &str, n: i64, len: Millis) {
        let base = env.ctx().now() - 3 * 24 * 60 * MIN;
        for i in 0..n {
            span(env, exe, base + i * 2 * MIN, len);
        }
    }

    fn names(env: &Env) -> Vec<String> {
        suggest(&env.ctx(), 7)
            .unwrap()
            .into_iter()
            .map(|s| s.exe)
            .collect()
    }

    fn codes(env: &Env, exe: &str) -> Vec<&'static str> {
        suggest(&env.ctx(), 7)
            .unwrap()
            .into_iter()
            .find(|s| s.exe == exe)
            .map(|s| s.reasons.iter().map(|r| r.code).collect())
            .unwrap_or_default()
    }

    fn cfg_path() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        (d, p)
    }

    #[test]
    fn flicker_apps_are_suggested_with_stats() {
        let env = Env::new();
        spans(&env, "noisy.exe", 30, 5_000);
        spans(&env, "calm.exe", 30, 120_000);
        let s = suggest(&env.ctx(), 7).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].exe, "noisy.exe");
        assert_eq!((s[0].span_count, s[0].short_count), (30, 30));
        assert_eq!(s[0].total_ms, 150_000);
        assert!(s[0].summary.contains("noisy.exe"));
        // 区間数が少なければ提案しない
        spans(&env, "few.exe", 5, 1_000);
        assert!(!names(&env).contains(&"few.exe".to_string()));
    }

    #[test]
    fn known_system_exe_is_suggested_but_not_frame_host() {
        let env = Env::new();
        spans(&env, "dwm.exe", 3, 40 * MIN);
        spans(&env, "ApplicationFrameHost.exe", 40, 1_000);
        let s = suggest(&env.ctx(), 7).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].reasons[0].code, "system");
        assert!(s[0].reasons[0].text.contains("長い"));
    }

    #[test]
    fn review_and_user_signals() {
        let env = Env::new();
        let ctx = env.ctx();
        let t0 = ctx.now() - 2 * 24 * 60 * MIN;
        for i in 0..10 {
            insert_segment(
                ctx.conn,
                &format!("R{i}"),
                t0 + i * MIN,
                t0 + i * MIN + 20_000,
                SegmentKind::Work,
                None,
                SegmentStatus::NeedsReview,
            );
        }
        ctx.conn
            .execute("UPDATE activity_segment SET exe_name = 'popup.exe'", [])
            .unwrap();
        assert_eq!(codes(&env, "popup.exe"), vec!["review"]);

        for i in 0..2 {
            insert_segment(
                ctx.conn,
                &format!("U{i}"),
                t0 + (20 + i) * MIN,
                t0 + (20 + i) * MIN + 30_000,
                SegmentKind::Ignored,
                None,
                SegmentStatus::Confirmed,
            );
        }
        for i in 0..5 {
            insert_segment(
                ctx.conn,
                &format!("N{i}"),
                t0 + (30 + i) * MIN,
                t0 + (30 + i) * MIN + 30_000,
                SegmentKind::Work,
                None,
                SegmentStatus::Confirmed,
            );
        }
        ctx.conn
            .execute(
                "UPDATE activity_segment SET exe_name = 'popup.exe', decided_by = 'user' WHERE status = 'confirmed'",
                [],
            )
            .unwrap();
        assert_eq!(
            codes(&env, "popup.exe"),
            vec!["review", "user_ignored", "user_no_task"]
        );
    }

    #[test]
    fn work_apps_listed_and_dismissed_are_excluded() {
        let env = Env::new();
        spans(&env, "WindowsTerminal.exe", 30, 1_000);
        spans(&env, "msedge.exe", 30, 1_000);
        spans(&env, "explorer.exe", 30, 1_000); // 既定の無視リストにある
        spans(&env, "noisy.exe", 30, 1_000);
        assert_eq!(names(&env), vec!["noisy.exe"]);
        dismiss(&env.ctx(), "NOISY.exe").unwrap();
        assert!(names(&env).is_empty());
        // 30 日後には再び提案される
        env.clock.set(env.ctx().now() + (DISMISS_DAYS + 1) * 86_400_000);
        spans(&env, "noisy.exe", 30, 1_000);
        assert!(names(&env).contains(&"noisy.exe".to_string()));
    }

    #[test]
    fn add_appends_to_defaults_and_recomputes_then_remove_restores() {
        let env = Env::new();
        let ctx = env.ctx();
        let (_d, p) = cfg_path();
        let now = ctx.now();
        // 12 時の 3 日前に、作業の合間に popup.exe を 3 分前面にした記録
        let t0 = now - 3 * 24 * 60 * MIN;
        let w = |id: &str, s: Millis, e: Millis, exe: &str| {
            ctx.conn
                .execute(
                    "INSERT INTO window_span (id, start_at, end_at, hwnd, exe_name, title, excluded) \
                     VALUES (?1,?2,?3,?1,?4,'t',0)",
                    rusqlite::params![id, s, e, exe],
                )
                .unwrap();
        };
        w("A", t0, t0 + 10 * MIN, "devenv.exe");
        w("B", t0 + 10 * MIN, t0 + 13 * MIN, "popup.exe");
        w("C", t0 + 13 * MIN, t0 + 20 * MIN, "devenv.exe");
        let kinds = |ctx: &Ctx| {
            repo::segment::list(ctx.conn, Range::new(t0, t0 + 30 * MIN))
                .unwrap()
                .iter()
                .map(|s| s.kind)
                .collect::<Vec<_>>()
        };
        crate::rebuild::rebuild(ctx.conn, ctx.cfg, Range::new(t0, t0 + 30 * MIN), now).unwrap();
        assert!(!kinds(&ctx).contains(&SegmentKind::Ignored));

        let r = add(&ctx, &p, "C:\\x\\Popup.EXE", None).unwrap();
        assert!(r.changed && r.recomputed.is_some());
        // 既定の値に追記した形で保存される
        let saved = Config::load_from(&p).unwrap().collect.ignore_exes;
        assert_eq!(saved.len(), Config::default().collect.ignore_exes.len() + 1);
        assert!(saved.contains(&"Popup.EXE".to_string()));
        assert!(kinds(&ctx).contains(&SegmentKind::Ignored));
        // 生データは変わらない
        let n: i64 = ctx
            .conn
            .query_row("SELECT COUNT(*) FROM window_span", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
        // 二重追加は何もしない
        assert!(!add(&ctx, &p, "popup.exe", None).unwrap().changed);

        remove(&ctx, &p, "popup.exe", None).unwrap();
        assert!(!kinds(&ctx).contains(&SegmentKind::Ignored));
        assert!(matches!(
            remove(&ctx, &p, "popup.exe", None),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn list_reports_customization() {
        let mut cfg = Config::default();
        assert!(!list(&cfg).customized);
        cfg.collect.ignore_exes.push("x.exe".into());
        assert!(list(&cfg).customized);
    }
}
