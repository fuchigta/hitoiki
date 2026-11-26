//! 日時の解釈と表示（docs/dev/architecture.md「CLI」: ISO 8601 / ローカル日付 / today 等のエイリアス）。
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

/// 【一時】ミューテーションテストのゲートの確認用（テストなし。PR から取り除く）。
pub fn gate_probe_is_long(min: i64) -> bool {
    min >= 90
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

/// ローカル日付 `d` の「0 時から `min` 分」の時刻（UTC ミリ秒）。`min` は 0〜1440（1440 は翌日 0 時）。
///
/// DST で存在しない時刻（春の切り替えの空白）は、存在する最初の時刻まで進める。
/// 2 回現れる時刻（秋の切り替え）は早い方を採る。
pub fn local_minute_to_ms(tz: Tz, d: NaiveDate, min: u32) -> Millis {
    if min >= 24 * 60 {
        return day_range(tz, d).end;
    }
    let base = d.and_time(NaiveTime::MIN) + Duration::minutes(min as i64);
    for extra in 0..=24 * 60 {
        let t = base + Duration::minutes(extra);
        if let Some(t) = tz.from_local_datetime(&t).earliest() {
            return t.timestamp_millis();
        }
    }
    // 到達しない想定。
    Utc.from_utc_datetime(&base).timestamp_millis()
}

/// UTC ミリ秒を、ローカル日付 `d` の 0 時からの分に変換する（`d` の範囲外は 0〜1440 に丸める）。
pub fn ms_to_local_minute(tz: Tz, d: NaiveDate, ms: Millis) -> u32 {
    let r = day_range(tz, d);
    if ms <= r.start {
        return 0;
    }
    if ms >= r.end {
        return 24 * 60;
    }
    let l = to_local(tz, ms);
    use chrono::Timelike;
    l.hour() * 60 + l.minute()
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

    /// proptest によるプロパティテスト（往復変換と範囲の包含関係）。
    ///
    /// タイムゾーンは UTC と Asia/Tokyo に固定する。どちらも対象期間（2000〜2099 年）に
    /// サマータイムが無く、オフセットが時単位なので、1 日は常に 24 時間で、
    /// ローカルの分と UTC ミリ秒が 1 対 1 に対応する。サマータイムのあるタイムゾーンでは
    /// 春の切り替えで存在しない時刻を次の有効な時刻へ進める仕様（`local_minute_to_ms`）のため、
    /// 分 → ミリ秒 → 分 の往復は成り立たない（不具合ではなく仕様）。
    mod props {
        use super::*;
        use proptest::prelude::*;

        /// 1 日のミリ秒。
        const DAY_MS: Millis = 86_400_000;
        /// 1 分のミリ秒。
        const MIN_MS: Millis = 60_000;
        /// 対象期間の始まり（2000-01-01T00:00:00Z）。
        const START_MS: Millis = 946_684_800_000;
        /// 対象期間の終わり（2100-01-01T00:00:00Z、含まない）。
        const END_MS: Millis = 4_102_444_800_000;

        fn tz() -> impl Strategy<Value = Tz> {
            prop_oneof![Just(Tz::UTC), Just(chrono_tz::Asia::Tokyo)]
        }

        /// 2000-01-01〜2099-12-31 のローカル日付。
        fn date() -> impl Strategy<Value = NaiveDate> {
            let base = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
            let last = NaiveDate::from_ymd_opt(2099, 12, 31).unwrap();
            (0..=(last - base).num_days()).prop_map(move |n| base + Duration::days(n))
        }

        /// 対象期間内の UTC ミリ秒。
        fn instant() -> impl Strategy<Value = Millis> {
            START_MS..END_MS
        }

        /// 分単位に切り捨てる（UTC / Asia/Tokyo はオフセットが時単位なので、ローカルでも同じ）。
        fn floor_minute(ms: Millis) -> Millis {
            ms - ms.rem_euclid(MIN_MS)
        }

        proptest! {
            // CI の実行時間を抑えるため、既定の 256 ケースから 64 ケースに減らす。
            // with_cases で明示した値は環境変数 PROPTEST_CASES に左右されないので、
            // ローカルと CI で同じケース数になる。
            #![proptest_config(ProptestConfig::with_cases(64))]

            /// 分 → ミリ秒 → 分 で元に戻る（0〜1440 の全域）。
            #[test]
            fn local_minute_roundtrip(tz in tz(), d in date(), min in 0u32..=24 * 60) {
                let ms = local_minute_to_ms(tz, d, min);
                prop_assert_eq!(ms_to_local_minute(tz, d, ms), min);
            }

            /// ミリ秒 → 分 → ミリ秒 で、その日の範囲内の時刻が分単位に切り捨てた値に戻る。
            #[test]
            fn ms_to_local_minute_roundtrip(tz in tz(), ms in instant()) {
                let d = local_date_of(tz, ms);
                let min = ms_to_local_minute(tz, d, ms);
                prop_assert!(min < 24 * 60);
                prop_assert_eq!(local_minute_to_ms(tz, d, min), floor_minute(ms));
            }

            /// 日の範囲は 24 時間で、始まりと終わり直前がその日、終わりが翌日の始まりになる。
            #[test]
            fn day_range_covers_the_day(tz in tz(), d in date()) {
                let r = day_range(tz, d);
                prop_assert_eq!(r.len(), DAY_MS);
                prop_assert_eq!(local_date_of(tz, r.start), d);
                prop_assert_eq!(local_date_of(tz, r.end - 1), d);
                prop_assert_eq!(local_date_of(tz, r.start - 1), d - Duration::days(1));
                prop_assert_eq!(r.end, day_range(tz, d + Duration::days(1)).start);
                prop_assert_eq!(r.start, local_minute_to_ms(tz, d, 0));
                prop_assert_eq!(r.end, local_minute_to_ms(tz, d, 24 * 60));
            }

            /// 任意の時刻は、そのローカル日付の日の範囲と週の範囲に含まれる。
            #[test]
            fn ranges_contain_instant(tz in tz(), ms in instant()) {
                let d = local_date_of(tz, ms);
                let day = day_range(tz, d);
                prop_assert!(day.start <= ms && ms < day.end);
                let week = week_range(tz, d);
                prop_assert!(week.start <= ms && ms < week.end);
            }

            /// 週の範囲は月曜 0 時から 7 日間で、その日の範囲を含み、同じ週のどの日から求めても同じになる。
            #[test]
            fn week_range_contains_day(tz in tz(), d in date(), other in 0i64..7) {
                let w = week_range(tz, d);
                let day = day_range(tz, d);
                prop_assert_eq!(w.len(), 7 * DAY_MS);
                prop_assert!(w.start <= day.start && day.end <= w.end);
                let monday = local_date_of(tz, w.start);
                prop_assert_eq!(monday.weekday(), chrono::Weekday::Mon);
                prop_assert_eq!(w.start, day_range(tz, monday).start);
                prop_assert_eq!(week_range(tz, monday + Duration::days(other)), w);
            }

            /// 表示した長さを解釈し直すと、分単位に切り捨てた長さに戻る。
            #[test]
            fn duration_roundtrip(min in 0i64..=1_000_000, sub in 0i64..MIN_MS) {
                let s = fmt_duration(min * MIN_MS + sub);
                prop_assert_eq!(parse_duration_min(&s).unwrap(), min);
            }

            /// `XhYm` / `Xh` / `Ym` / 数字だけ の形式は、時と分を合計した分になる。
            #[test]
            fn parse_duration_forms(h in 0i64..=10_000, m in 0i64..=59) {
                prop_assert_eq!(parse_duration_min(&format!("{h}h{m}m")).unwrap(), h * 60 + m);
                prop_assert_eq!(parse_duration_min(&format!("{h}h")).unwrap(), h * 60);
                prop_assert_eq!(parse_duration_min(&format!("{m}m")).unwrap(), m);
                prop_assert_eq!(parse_duration_min(&format!("{}", h * 60 + m)).unwrap(), h * 60 + m);
            }

            /// RFC 3339 の表示を解釈し直すと、ミリ秒まで元の時刻に戻る（`now` には依存しない）。
            #[test]
            fn rfc3339_roundtrip(tz in tz(), ms in instant(), now in instant()) {
                let s = fmt_rfc3339(tz, ms);
                prop_assert_eq!(parse_instant(tz, &s, now).unwrap(), ms);
            }

            /// `HH:MM` の表示を同じ日の `now` で解釈し直すと、分単位に切り捨てた時刻に戻る。
            #[test]
            fn hm_roundtrip(tz in tz(), ms in instant(), now_offset in 0i64..DAY_MS) {
                // `HH:MM` は `now` のローカル日付で解釈されるので、`now` は `ms` と同じ日に取る。
                let day = day_range(tz, local_date_of(tz, ms));
                let now = day.start + now_offset;
                let s = fmt_hm(tz, ms);
                prop_assert_eq!(parse_instant(tz, &s, now).unwrap(), floor_minute(ms));
            }
        }
    }
}
