//! TZID の解決。IANA 名はそのまま、Windows タイムゾーン名（Outlook）は対応表で変換する。
//! VTIMEZONE からの推定は行わない。

use chrono_tz::Tz;

/// Windows タイムゾーン名 → IANA（主要なもの）。
const WINDOWS_TO_IANA: &[(&str, &str)] = &[
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Israel Standard Time", "Asia/Jerusalem"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
    ("UTC", "UTC"),
    ("GMT Standard Time", "Europe/London"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("Romance Standard Time", "Europe/Paris"),
    ("E. Europe Standard Time", "Europe/Chisinau"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("FLE Standard Time", "Europe/Kiev"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Eastern Standard Time", "America/New_York"),
    ("Central Standard Time", "America/Chicago"),
    ("Mountain Standard Time", "America/Denver"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("Canada Central Standard Time", "America/Regina"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Mexico Standard Time", "America/Mexico_City"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Buenos_Aires"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
];

/// TZID 文字列を `Tz` に解決する。解決できなければ None。
pub fn resolve_tzid(tzid: &str) -> Option<Tz> {
    let tzid = tzid.trim().trim_matches('"');
    if let Ok(tz) = tzid.parse::<Tz>() {
        return Some(tz);
    }
    if let Some((_, iana)) = WINDOWS_TO_IANA.iter().find(|(w, _)| w.eq_ignore_ascii_case(tzid)) {
        return iana.parse().ok();
    }
    // "/mozilla.org/20050126_1/Asia/Tokyo" のような接頭辞付き: 先頭の区切りを順に削って試す
    let mut rest = tzid.trim_start_matches('/');
    while let Some((_, tail)) = rest.split_once('/') {
        if let Ok(tz) = tail.parse::<Tz>() {
            return Some(tz);
        }
        rest = tail;
    }
    // "(UTC+09:00) Osaka, Sapporo, Tokyo" のような表示名
    let lower = tzid.to_ascii_lowercase();
    if lower.contains("tokyo") || lower.contains("osaka") {
        return Some(chrono_tz::Asia::Tokyo);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves() {
        assert_eq!(resolve_tzid("Asia/Tokyo"), Some(chrono_tz::Asia::Tokyo));
        assert_eq!(resolve_tzid("Tokyo Standard Time"), Some(chrono_tz::Asia::Tokyo));
        assert_eq!(
            resolve_tzid("/mozilla.org/20050126_1/America/New_York"),
            Some(chrono_tz::America::New_York)
        );
        assert_eq!(
            resolve_tzid("(UTC+09:00) Osaka, Sapporo, Tokyo"),
            Some(chrono_tz::Asia::Tokyo)
        );
        assert_eq!(resolve_tzid("Nowhere Standard Time"), None);
    }
}
