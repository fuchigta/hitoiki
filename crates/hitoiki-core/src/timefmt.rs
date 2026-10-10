//! 日時の解釈と表示（12.1: ISO 8601 / ローカル日付 / today 等のエイリアス）。
//! 保存は UTC ミリ秒、表示と日付境界は設定のタイムゾーンで扱う。

use crate::{Error, Millis, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// 半開区間 [start, end)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Range {
    pub start: Millis,
    pub end: Millis,
}

impl Range {
    pub fn new(start: Millis, end: Millis) -> Self {
        Self { start, end }
    }
    pub fn len(&self) -> Millis {
        (self.end - self.start).max(0)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// 重なり部分。重ならなければ None。
    pub fn intersect(&self, other: &Range) -> Option<Range> {
        let s = self.start.max(other.start);
        let e = self.end.min(other.end);
        (s < e).then_some(Range { start: s, end: e })
    }
}

pub fn parse_tz(name: &str) -> Result<Tz> {
    name.parse::<Tz>()
        .map_err(|_| Error::InvalidArgument(format!("不明なタイムゾーン: {name}")))
}

fn local_midnight(tz: Tz, d: NaiveDate) -> Millis {
    if let Some(t) = tz.from_local_datetime(&d.and_time(NaiveTime::MIN)).earliest() {
        return t.timestamp_millis();
    }
    // DST で 0 時が存在しない（America/Santiago 等）場合は、その日の最初の有効なローカル時刻。
    for m in 1..=24 * 60 {
        let t = d.and_time(NaiveTime::MIN) + Duration::minutes(m);
        if let Some(t) = tz.from_local_datetime(&t).earliest() {
            return t.timestamp_millis();
        }
    }
    // 到達しない想定。UTC 0 時で代用する。
    Utc.from_utc_datetime(&d.and_time(NaiveTime::MIN))
        .timestamp_millis()
}

pub fn local_date_of(tz: Tz, ms: Millis) -> NaiveDate {
    to_local(tz, ms).date_naive()
}

pub fn to_local(tz: Tz, ms: Millis) -> DateTime<Tz> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .expect("valid ms")
        .with_timezone(&tz)
}

/// ローカル日付 1 日分の範囲。
pub fn day_range(tz: Tz, d: NaiveDate) -> Range {
    Range {
        start: local_midnight(tz, d),
        end: local_midnight(tz, d + Duration::days(1)),
    }
}

/// 月曜始まりの週の範囲。
pub fn week_range(tz: Tz, d: NaiveDate) -> Range {
    let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
    Range {
        start: local_midnight(tz, monday),
        end: local_midnight(tz, monday + Duration::days(7)),
    }
}

/// `today` / `yesterday` / `this-week` / `last-week` / `YYYY-MM-DD` を範囲に変換する。
pub fn parse_period(tz: Tz, s: &str, now: Millis) -> Result<Range> {
    let today = local_date_of(tz, now);
    match s {
        "this-week" => Ok(week_range(tz, today)),
        "last-week" => Ok(week_range(tz, today - Duration::days(7))),
        _ => Ok(day_range(tz, parse_date(tz, s, now)?)),
    }
}

/// 日付指定（エイリアス today/yesterday も可）。
pub fn parse_date(tz: Tz, s: &str, now: Millis) -> Result<NaiveDate> {
    let today = local_date_of(tz, now);
    match s {
        "today" => Ok(today),
        "yesterday" => Ok(today - Duration::days(1)),
        _ => NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| Error::InvalidArgument(format!("日付を解釈できません: {s}"))),
    }
}

/// 時刻指定。RFC 3339、ローカル日時（`YYYY-MM-DD HH:MM[:SS]` / `YYYY-MM-DDTHH:MM[:SS]`）、
/// 当日の時刻（`HH:MM`）、日付（その日の 0 時）、`now` を受け付ける。
pub fn parse_instant(tz: Tz, s: &str, now: Millis) -> Result<Millis> {
    if s == "now" {
        return Ok(now);
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp_millis());
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return local_naive_to_ms(tz, ndt);
        }
    }
    for fmt in ["%H:%M", "%H:%M:%S"] {
        if let Ok(t) = NaiveTime::parse_from_str(s, fmt) {
            return local_naive_to_ms(tz, local_date_of(tz, now).and_time(t));
        }
    }
    parse_date(tz, s, now).map(|d| local_midnight(tz, d))
}

fn local_naive_to_ms(tz: Tz, ndt: NaiveDateTime) -> Result<Millis> {
    tz.from_local_datetime(&ndt)
        .earliest()
        .map(|d| d.timestamp_millis())
        .ok_or_else(|| Error::InvalidArgument(format!("存在しないローカル時刻: {ndt}")))
}

/// `90m` / `1h30m` / `2h` / `45`（分）を分に変換する。
pub fn parse_duration_min(s: &str) -> Result<i64> {
    let err = || Error::InvalidArgument(format!("時間を解釈できません: {s}"));
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n);
    }
    let mut total = 0i64;
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        let unit = match c {
            'h' => 60,
            'm' => 1,
            _ => return Err(err()),
        };
        total += num.parse::<i64>().map_err(|_| err())? * unit;
        num.clear();
    }
    if !num.is_empty() || total == 0 && s.is_empty() {
        return Err(err());
    }
    Ok(total)
}

/// 表示用: `HH:MM`。
pub fn fmt_hm(tz: Tz, ms: Millis) -> String {
    to_local(tz, ms).format("%H:%M").to_string()
}

/// 表示用: RFC 3339（ローカルオフセット付き）。JSON 出力に使う。
pub fn fmt_rfc3339(tz: Tz, ms: Millis) -> String {
    to_local(tz, ms).to_rfc3339()
}

/// 表示用: `1h23m`。
pub fn fmt_duration(ms: Millis) -> String {
    let min = ms / 60_000;
    if min >= 60 {
        format!("{}h{:02}m", min / 60, min % 60)
    } else {
        format!("{min}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midnight_missing_in_dst_does_not_panic() {
        // America/Santiago は 2026-09-06 の 0:00 に時計が 1:00 へ進み、0 時が存在しない。
        let tz = parse_tz("America/Santiago").unwrap();
        let d = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        let r = day_range(tz, d);
        assert_eq!(
            to_local(tz, r.start).time(),
            NaiveTime::from_hms_opt(1, 0, 0).unwrap()
        );
        assert_eq!(r.len(), 23 * 3_600_000);
        let _ = week_range(tz, d);
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration_min("90m").unwrap(), 90);
        assert_eq!(parse_duration_min("1h30m").unwrap(), 90);
        assert_eq!(parse_duration_min("2h").unwrap(), 120);
        assert_eq!(parse_duration_min("45").unwrap(), 45);
        assert!(parse_duration_min("abc").is_err());
    }

    #[test]
    fn day_range_tokyo() {
        let tz = parse_tz("Asia/Tokyo").unwrap();
        let r = day_range(tz, NaiveDate::from_ymd_opt(2026, 10, 10).unwrap());
        assert_eq!(r.len(), 86_400_000);
        assert_eq!(fmt_rfc3339(tz, r.start), "2026-10-10T00:00:00+09:00");
    }

    #[test]
    fn instants() {
        let tz = parse_tz("Asia/Tokyo").unwrap();
        let now = parse_instant(tz, "2026-10-10T12:00:00+09:00", 0).unwrap();
        let t = parse_instant(tz, "09:30", now).unwrap();
        assert_eq!(fmt_rfc3339(tz, t), "2026-10-10T09:30:00+09:00");
        let d = parse_instant(tz, "2026-10-09", now).unwrap();
        assert_eq!(fmt_rfc3339(tz, d), "2026-10-09T00:00:00+09:00");
    }
}
