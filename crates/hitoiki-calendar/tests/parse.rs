//! フィクスチャ ICS による解析・展開のテスト（ネットワーク不使用）。

use chrono::TimeZone;
use chrono_tz::Asia::Tokyo;
use hitoiki_calendar::parse_ics;
use hitoiki_core::model::CalendarEvent;
use hitoiki_core::timefmt::Range;

fn jst(y: i32, m: u32, d: u32, h: u32, mi: u32) -> i64 {
    Tokyo
        .with_ymd_and_hms(y, m, d, h, mi, 0)
        .unwrap()
        .timestamp_millis()
}

fn range(from: (i32, u32, u32), to: (i32, u32, u32)) -> Range {
    Range::new(jst(from.0, from.1, from.2, 0, 0), jst(to.0, to.1, to.2, 0, 0))
}

fn parse(text: &str, r: Range) -> Vec<CalendarEvent> {
    parse_ics(text, "src", Tokyo, r, 123).unwrap()
}

fn wrap(body: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\n{body}END:VCALENDAR\r\n")
}

const GOOGLE: &str = "\
BEGIN:VEVENT
DTSTART:20250310T010000Z
DTEND:20250310T020000Z
UID:single@google.com
SUMMARY:単発の打ち合わせ
LOCATION:会議室A\\, 3F
TRANSP:OPAQUE
STATUS:CONFIRMED
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=Asia/Tokyo:20250303T100000
DTEND;TZID=Asia/Tokyo:20250303T110000
RRULE:FREQ=WEEKLY;BYDAY=MO;UNTIL=20250331T145959Z
EXDATE;TZID=Asia/Tokyo:20250317T100000
UID:weekly@google.com
SUMMARY:週次定例
TRANSP:OPAQUE
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=Asia/Tokyo:20250324T150000
DTEND;TZID=Asia/Tokyo:20250324T160000
RECURRENCE-ID;TZID=Asia/Tokyo:20250324T100000
UID:weekly@google.com
SUMMARY:週次定例 (移動)
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=Asia/Tokyo:20250331T100000
DTEND;TZID=Asia/Tokyo:20250331T110000
RECURRENCE-ID;TZID=Asia/Tokyo:20250331T100000
UID:weekly@google.com
STATUS:CANCELLED
SUMMARY:週次定例 (中止)
END:VEVENT
BEGIN:VEVENT
DTSTART:20250312T000000Z
DTEND:20250312T010000Z
UID:free@google.com
SUMMARY:空き
TRANSP:TRANSPARENT
END:VEVENT
BEGIN:VEVENT
DTSTART:20250313T000000Z
DTEND:20250313T010000Z
UID:cancelled@google.com
SUMMARY:キャンセル
STATUS:CANCELLED
END:VEVENT
";

#[test]
fn google_single_and_recurrence() {
    let evs = parse(&wrap(GOOGLE), range((2025, 3, 1), (2025, 4, 30)));
    let single = evs.iter().find(|e| e.external_id == "single@google.com").unwrap();
    assert_eq!(single.start_at, jst(2025, 3, 10, 10, 0));
    assert_eq!(single.end_at, jst(2025, 3, 10, 11, 0));
    assert_eq!(single.title, "単発の打ち合わせ");
    assert_eq!(single.location.as_deref(), Some("会議室A, 3F"));
    assert!(single.is_busy && !single.all_day);
    assert_eq!(single.source, "src");
    assert_eq!(single.fetched_at, 123);

    // 毎週月曜: 3/3, (3/10), 3/17 は EXDATE, 3/24 は 15:00 に移動, 3/31 は中止
    let weekly: Vec<_> = evs
        .iter()
        .filter(|e| e.external_id == "weekly@google.com")
        .collect();
    let starts: Vec<_> = weekly.iter().map(|e| e.start_at).collect();
    assert_eq!(
        starts,
        vec![
            jst(2025, 3, 3, 10, 0),
            jst(2025, 3, 10, 10, 0),
            jst(2025, 3, 24, 15, 0)
        ]
    );
    assert_eq!(weekly[2].title, "週次定例 (移動)");
    assert_eq!(weekly[2].end_at, jst(2025, 3, 24, 16, 0));

    let free = evs.iter().find(|e| e.external_id == "free@google.com").unwrap();
    assert!(!free.is_busy);
    assert!(evs.iter().all(|e| e.external_id != "cancelled@google.com"));
}

#[test]
fn range_limits_expansion() {
    // 3/9〜3/12 だけ見る → 週次の 3/10 と単発、空きのみ
    let evs = parse(&wrap(GOOGLE), range((2025, 3, 9), (2025, 3, 13)));
    let ids: Vec<_> = evs.iter().map(|e| e.external_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["single@google.com", "weekly@google.com", "free@google.com"]
    );
    // 開始が範囲より前でも重なれば含む
    let r = Range::new(jst(2025, 3, 10, 10, 30), jst(2025, 3, 10, 10, 31));
    let evs = parse(&wrap(GOOGLE), r);
    assert_eq!(evs.len(), 2);
}

const OUTLOOK: &str = "\
BEGIN:VTIMEZONE
TZID:Tokyo Standard Time
BEGIN:STANDARD
DTSTART:16010101T000000
TZOFFSETFROM:+0900
TZOFFSETTO:+0900
END:STANDARD
END:VTIMEZONE
BEGIN:VEVENT
DTSTART;TZID=Tokyo Standard Time:20250305T130000
DTEND;TZID=Tokyo Standard Time:20250305T140000
RRULE:FREQ=DAILY;COUNT=3
UID:040000008200E00074C5B7101A82E008-outlook
SUMMARY:Outlook 定例
X-MICROSOFT-CDO-BUSYSTATUS:FREE
BEGIN:VALARM
TRIGGER:-PT15M
ACTION:DISPLAY
DESCRIPTION:Reminder
END:VALARM
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=\"Tokyo Standard Time\":20250306T090000
DURATION:PT30M
UID:outlook-2
SUMMARY:Duration only
X-MICROSOFT-CDO-BUSYSTATUS:BUSY
END:VEVENT
";

#[test]
fn outlook_windows_tzid() {
    let evs = parse(&wrap(OUTLOOK), range((2025, 3, 1), (2025, 3, 31)));
    let o: Vec<_> = evs
        .iter()
        .filter(|e| e.external_id.ends_with("-outlook"))
        .collect();
    assert_eq!(o.len(), 3);
    assert_eq!(o[0].start_at, jst(2025, 3, 5, 13, 0));
    assert_eq!(o[2].start_at, jst(2025, 3, 7, 13, 0));
    assert!(o.iter().all(|e| !e.is_busy));
    let d = evs.iter().find(|e| e.external_id == "outlook-2").unwrap();
    assert_eq!(d.start_at, jst(2025, 3, 6, 9, 0));
    assert_eq!(d.end_at, jst(2025, 3, 6, 9, 30));
    assert!(d.is_busy);
}

#[test]
fn all_day_events() {
    let body = "\
BEGIN:VEVENT
DTSTART;VALUE=DATE:20250320
DTEND;VALUE=DATE:20250321
UID:holiday
SUMMARY:春分の日
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20250325
DTEND;VALUE=DATE:20250328
UID:trip
SUMMARY:出張
TRANSP:TRANSPARENT
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20250401
UID:daily-allday
SUMMARY:毎日
RRULE:FREQ=DAILY;UNTIL=20250403
END:VEVENT
";
    let evs = parse(&wrap(body), range((2025, 3, 1), (2025, 4, 30)));
    let h = evs.iter().find(|e| e.external_id == "holiday").unwrap();
    assert!(h.all_day && h.is_busy);
    assert_eq!(h.start_at, jst(2025, 3, 20, 0, 0));
    assert_eq!(h.end_at, jst(2025, 3, 21, 0, 0));
    let t = evs.iter().find(|e| e.external_id == "trip").unwrap();
    assert_eq!(t.end_at, jst(2025, 3, 28, 0, 0));
    assert!(!t.is_busy);
    let d: Vec<_> = evs.iter().filter(|e| e.external_id == "daily-allday").collect();
    assert_eq!(d.len(), 3);
    assert_eq!(d[2].start_at, jst(2025, 4, 3, 0, 0));
    assert_eq!(d[2].end_at, jst(2025, 4, 4, 0, 0));
}

#[test]
fn folded_lines_floating_and_rdate() {
    let body = "\
BEGIN:VEVENT
DTSTART:20250307T090000
DTEND:20250307T100000
UID:float
SUMMARY:とても長いタイトルが折り返されて
  いる場合 and more
 text
DESCRIPTION:ignored
END:VEVENT
BEGIN:VEVENT
DTSTART:20250310T090000Z
DTEND:20250310T093000Z
RDATE:20250312T090000Z,20250314T090000Z
EXDATE:20250314T090000Z
UID:rdate
SUMMARY:RDATE
END:VEVENT
";
    let evs = parse(&wrap(body), range((2025, 3, 1), (2025, 3, 31)));
    let f = evs.iter().find(|e| e.external_id == "float").unwrap();
    // 浮動時刻は設定のタイムゾーン (JST)
    assert_eq!(f.start_at, jst(2025, 3, 7, 9, 0));
    assert_eq!(f.title, "とても長いタイトルが折り返されて いる場合 and moretext");
    let r: Vec<_> = evs
        .iter()
        .filter(|e| e.external_id == "rdate")
        .map(|e| e.start_at)
        .collect();
    assert_eq!(r, vec![jst(2025, 3, 10, 18, 0), jst(2025, 3, 12, 18, 0)]);
}

#[test]
fn dst_recurrence_keeps_wall_clock() {
    let body = "\
BEGIN:VEVENT
DTSTART;TZID=America/New_York:20250306T090000
DTEND;TZID=America/New_York:20250306T100000
RRULE:FREQ=DAILY;COUNT=7
UID:ny
SUMMARY:NY
END:VEVENT
";
    let evs = parse(&wrap(body), range((2025, 3, 1), (2025, 3, 31)));
    assert_eq!(evs.len(), 7);
    // 3/9 に夏時間開始。壁時計 9:00 は UTC 14:00 -> 13:00
    let utc = |e: &CalendarEvent| (e.start_at / 3_600_000) % 24;
    assert_eq!(utc(&evs[0]), 14);
    assert_eq!(utc(&evs[6]), 13);
}

#[test]
fn garbage_is_tolerated() {
    let evs = parse("not an ics", range((2025, 3, 1), (2025, 3, 31)));
    assert!(evs.is_empty());
}
