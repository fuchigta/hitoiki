//! ICS テキスト → `CalendarEvent` の純粋な解析・繰り返し展開（ネットワークを使わない）。

use crate::ics::{unescape_text, vevents, Prop};
use crate::tzmap::resolve_tzid;
use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use hitoiki_core::model::CalendarEvent;
use hitoiki_core::timefmt::Range;
use hitoiki_core::Millis;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::str::FromStr;

const DAY_MS: Millis = 86_400_000;
/// 1 予定あたりの展開上限（暴走防止）。
const MAX_INSTANCES: usize = 5000;

/// 解釈済みの日時値。
#[derive(Debug, Clone, Copy)]
struct When {
    all_day: bool,
    /// `tz` での壁時計時刻（終日は 0:00）。
    naive: NaiveDateTime,
    tz: Tz,
}

impl When {
    fn ms(&self) -> Millis {
        local_to_ms(self.tz, self.naive)
    }
}

/// 壁時計時刻 → UTC ミリ秒。曖昧なら早い方、存在しない時刻（DST の隙間）は 1 時間進める。
fn local_to_ms(tz: Tz, naive: NaiveDateTime) -> Millis {
    tz.from_local_datetime(&naive)
        .earliest()
        .or_else(|| tz.from_local_datetime(&(naive + Duration::hours(1))).earliest())
        .map(|d| d.timestamp_millis())
        .unwrap_or_else(|| naive.and_utc().timestamp_millis())
}

fn ms_to_dt(tz: Tz, ms: Millis) -> DateTime<Tz> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_default()
        .with_timezone(&tz)
}

fn parse_naive(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
        .ok()
        .or_else(|| {
            // 秒なし (YYYYMMDDTHHMM) も許容
            NaiveDateTime::parse_from_str(&format!("{s}00"), "%Y%m%dT%H%M%S").ok()
        })
}

/// 1 つの値（カンマ区切りの 1 要素）を解釈する。
fn parse_one(value: &str, p: &Prop, default_tz: Tz) -> Option<When> {
    let v = value.trim();
    let is_date = p.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE"))
        || (v.len() == 8 && !v.contains('T'));
    if is_date {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        return Some(When {
            all_day: true,
            naive: d.and_time(NaiveTime::MIN),
            tz: default_tz,
        });
    }
    if let Some(utc) = v.strip_suffix(['Z', 'z']) {
        return Some(When {
            all_day: false,
            naive: parse_naive(utc)?,
            tz: chrono_tz::UTC,
        });
    }
    let naive = parse_naive(v)?;
    let tz = match p.param("TZID") {
        Some(id) => resolve_tzid(id).unwrap_or_else(|| {
            tracing::warn!("未知の TZID を設定のタイムゾーンで解釈します: {id}");
            default_tz
        }),
        None => default_tz,
    };
    Some(When {
        all_day: false,
        naive,
        tz,
    })
}

fn parse_when(p: &Prop, default_tz: Tz) -> Option<When> {
    parse_one(&p.value, p, default_tz)
}

fn parse_list(p: &Prop, default_tz: Tz) -> Vec<When> {
    p.value
        .split(',')
        .filter_map(|v| parse_one(v, p, default_tz))
        .collect()
}

/// ISO 8601 の期間 (`PT1H30M`, `P1D`, `P1W` 等) → ミリ秒。
fn parse_duration_ms(s: &str) -> Option<Millis> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in s.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (c, in_time) {
                    ('W', _) => n * 7 * DAY_MS,
                    ('D', _) => n * DAY_MS,
                    ('H', true) => n * 3_600_000,
                    ('M', true) => n * 60_000,
                    ('S', true) => n * 1000,
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

struct Ev {
    uid: String,
    start: When,
    end: Option<When>,
    duration: Option<Millis>,
    title: String,
    location: Option<String>,
    cancelled: bool,
    is_busy: bool,
    rrules: Vec<String>,
    rdates: Vec<When>,
    exdates: Vec<When>,
    recurrence_id: Option<When>,
}

fn build_ev(props: &[Prop], default_tz: Tz) -> Option<Ev> {
    let get = |n: &str| props.iter().find(|p| p.name == n);
    let uid = get("UID")?.value.trim().to_string();
    if uid.is_empty() {
        return None;
    }
    let start = parse_when(get("DTSTART")?, default_tz)?;
    let end = get("DTEND").and_then(|p| parse_when(p, default_tz));
    let duration = get("DURATION").and_then(|p| parse_duration_ms(&p.value));
    let title = get("SUMMARY")
        .map(|p| unescape_text(&p.value).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(無題)".to_string());
    let location = get("LOCATION")
        .map(|p| unescape_text(&p.value).trim().to_string())
        .filter(|s| !s.is_empty());
    let cancelled = get("STATUS").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("CANCELLED"));
    let transparent = get("TRANSP").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("TRANSPARENT"));
    let free = get("X-MICROSOFT-CDO-BUSYSTATUS").is_some_and(|p| p.value.trim().eq_ignore_ascii_case("FREE"));
    Some(Ev {
        uid,
        start,
        end,
        duration,
        title,
        location,
        cancelled,
        is_busy: !(transparent || free),
        rrules: props
            .iter()
            .filter(|p| p.name == "RRULE")
            .map(|p| p.value.trim().to_string())
            .collect(),
        rdates: props
            .iter()
            .filter(|p| p.name == "RDATE")
            .flat_map(|p| parse_list(p, default_tz))
            .collect(),
        exdates: props
            .iter()
            .filter(|p| p.name == "EXDATE")
            .flat_map(|p| parse_list(p, default_tz))
            .collect(),
        recurrence_id: get("RECURRENCE-ID").and_then(|p| parse_when(p, default_tz)),
    })
}

impl Ev {
    /// 開始 `s`（`start.tz` での時刻）に対応する終了時刻（ミリ秒）。
    fn end_for(&self, s: DateTime<Tz>) -> Millis {
        let s_ms = s.timestamp_millis();
        if self.start.all_day {
            let days = match self.end {
                Some(e) if e.all_day => (e.naive.date() - self.start.naive.date()).num_days().max(1),
                _ => self.duration.map_or(1, |d| (d / DAY_MS).max(1)),
            };
            let end_date = s.date_naive() + Duration::days(days);
            local_to_ms(self.start.tz, end_date.and_time(NaiveTime::MIN))
        } else {
            let dur = match (self.end, self.duration) {
                (Some(e), _) => e.ms() - self.start.ms(),
                (None, Some(d)) => d,
                (None, None) => 0,
            };
            s_ms + dur.max(0)
        }
    }

    /// 繰り返しルールの UNTIL を UTC 形式に正規化する。
    fn normalize_rrule(&self, rule: &str) -> String {
        rule.split(';')
            .map(|part| {
                let Some((k, v)) = part.split_once('=') else {
                    return part.to_string();
                };
                if !k.eq_ignore_ascii_case("UNTIL") {
                    return part.to_string();
                }
                let utc = if v.len() == 8 {
                    // 日付のみ: 終日は当日 0:00、時刻付きは当日終わりまで含める
                    NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(|d| {
                        let t = if self.start.all_day {
                            NaiveTime::MIN
                        } else {
                            NaiveTime::from_hms_opt(23, 59, 59).expect("valid time")
                        };
                        local_to_ms(self.start.tz, d.and_time(t))
                    })
                } else if let Some(z) = v.strip_suffix(['Z', 'z']) {
                    parse_naive(z).map(|n| n.and_utc().timestamp_millis())
                } else {
                    parse_naive(v).map(|n| local_to_ms(self.start.tz, n))
                };
                match utc.and_then(|ms| Utc.timestamp_millis_opt(ms).single()) {
                    Some(d) => format!("UNTIL={}", d.format("%Y%m%dT%H%M%SZ")),
                    None => part.to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    /// 範囲に関係しうる全開始時刻（ミリ秒 → `start.tz` の日時）。EXDATE 適用済み。
    fn starts(&self, range: Range) -> BTreeMap<Millis, DateTime<Tz>> {
        let tz = self.start.tz;
        let mut out: BTreeMap<Millis, DateTime<Tz>> = BTreeMap::new();
        out.insert(self.start.ms(), ms_to_dt(tz, self.start.ms()));
        for r in &self.rdates {
            out.insert(r.ms(), ms_to_dt(tz, r.ms()));
        }
        if !self.rrules.is_empty() {
            self.expand_rrules(range, &mut out);
        }
        let ex_ms: HashSet<Millis> = self.exdates.iter().map(|e| e.ms()).collect();
        // 時刻付き予定への日付のみの EXDATE はその日の回すべてを除く
        let ex_dates: HashSet<NaiveDate> = if self.start.all_day {
            HashSet::new()
        } else {
            self.exdates
                .iter()
                .filter(|e| e.all_day)
                .map(|e| e.naive.date())
                .collect()
        };
        out.retain(|ms, dt| !ex_ms.contains(ms) && !ex_dates.contains(&dt.date_naive()));
        out
    }

    fn expand_rrules(&self, range: Range, out: &mut BTreeMap<Millis, DateTime<Tz>>) {
        let tz = self.start.tz;
        let rtz = rrule::Tz::Tz(tz);
        let dt_start = ms_to_dt(tz, self.start.ms()).with_timezone(&rtz);
        let dur_guess = (self.end_for(ms_to_dt(tz, self.start.ms())) - self.start.ms()).max(0);
        let after = ms_to_dt(tz, range.start - dur_guess - DAY_MS).with_timezone(&rtz);
        let before = ms_to_dt(tz, range.end).with_timezone(&rtz);
        let mut set = rrule::RRuleSet::new(dt_start).after(after).before(before);
        for rule in &self.rrules {
            let norm = self.normalize_rrule(rule);
            match rrule::RRule::<rrule::Unvalidated>::from_str(&norm).and_then(|r| r.validate(dt_start)) {
                Ok(r) => set = set.rrule(r),
                Err(_) => tracing::warn!("解釈できない RRULE を無視しました"),
            }
        }
        for d in set.all_unchecked().into_iter().take(MAX_INSTANCES) {
            let ms = d.timestamp_millis();
            out.insert(ms, ms_to_dt(tz, ms));
        }
    }
}

fn overlaps(s: Millis, e: Millis, range: Range) -> bool {
    s < range.end && e.max(s + 1) > range.start
}

/// ICS テキストを解析し、`range` と重なる予定（繰り返しは展開済み）を返す。
///
/// - `source`: 予定の取得元名。
/// - `default_tz`: 浮動時刻・終日・未知の TZID を解釈するタイムゾーン。
/// - 解釈できない VEVENT は読み飛ばす。
pub fn parse_ics(
    text: &str,
    source: &str,
    default_tz: Tz,
    range: Range,
    fetched_at: Millis,
) -> Result<Vec<CalendarEvent>> {
    let evs: Vec<Ev> = vevents(text)
        .iter()
        .filter_map(|p| build_ev(p, default_tz))
        .collect();

    // 個別上書きされた回（元の展開から除く）
    let overridden: HashSet<(String, Millis)> = evs
        .iter()
        .filter_map(|e| e.recurrence_id.map(|r| (e.uid.clone(), r.ms())))
        .collect();

    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, Millis)> = BTreeSet::new();
    let mut push = |ev: &Ev, s: DateTime<Tz>, out: &mut Vec<CalendarEvent>| {
        let s_ms = s.timestamp_millis();
        let e_ms = ev.end_for(s);
        if !overlaps(s_ms, e_ms, range) || !seen.insert((ev.uid.clone(), s_ms)) {
            return;
        }
        out.push(CalendarEvent {
            id: hitoiki_core::id::new_id(),
            source: source.to_string(),
            external_id: ev.uid.clone(),
            start_at: s_ms,
            end_at: e_ms,
            all_day: ev.start.all_day,
            title: ev.title.clone(),
            location: ev.location.clone(),
            is_busy: ev.is_busy,
            fetched_at,
        });
    };

    for ev in &evs {
        if ev.cancelled {
            continue;
        }
        if ev.recurrence_id.is_some() {
            push(ev, ms_to_dt(ev.start.tz, ev.start.ms()), &mut out);
            continue;
        }
        for (ms, dt) in ev.starts(range) {
            if overridden.contains(&(ev.uid.clone(), ms)) {
                continue;
            }
            push(ev, dt, &mut out);
        }
    }
    out.sort_by(|a, b| (a.start_at, a.end_at, &a.external_id).cmp(&(b.start_at, b.end_at, &b.external_id)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration_ms("PT1H30M"), Some(5_400_000));
        assert_eq!(parse_duration_ms("P1DT2H"), Some(DAY_MS + 7_200_000));
        assert_eq!(parse_duration_ms("P2W"), Some(14 * DAY_MS));
        assert_eq!(parse_duration_ms("x"), None);
    }
}
