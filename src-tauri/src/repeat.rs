//! Repeat rules for recurring reminders and scheduled agent jobs.
//! Pure local-wall-time schedule math (`next_after`) and Russian formatting
//! (`describe_rule`, `format_next_local`).

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Minimum allowed interval between occurrences in minutes.
pub const MIN_INTERVAL_MINUTES: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Day {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Day {
    pub fn as_code(self) -> &'static str {
        match self {
            Day::Mon => "mon",
            Day::Tue => "tue",
            Day::Wed => "wed",
            Day::Thu => "thu",
            Day::Fri => "fri",
            Day::Sat => "sat",
            Day::Sun => "sun",
        }
    }

    pub fn short_ru(self) -> &'static str {
        match self {
            Day::Mon => "пн",
            Day::Tue => "вт",
            Day::Wed => "ср",
            Day::Thu => "чт",
            Day::Fri => "пт",
            Day::Sat => "сб",
            Day::Sun => "вс",
        }
    }

    /// «каждый понедельник», «каждую среду», «каждое воскресенье».
    pub fn every_ru(self) -> &'static str {
        match self {
            Day::Mon => "каждый понедельник",
            Day::Tue => "каждый вторник",
            Day::Wed => "каждую среду",
            Day::Thu => "каждый четверг",
            Day::Fri => "каждую пятницу",
            Day::Sat => "каждую субботу",
            Day::Sun => "каждое воскресенье",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let s = raw.trim().to_lowercase().replace('ё', "е");
        match s.as_str() {
            "mon" | "monday" | "mo" | "пн" | "пон" | "понедельник" | "1" => Some(Day::Mon),
            "tue" | "tues" | "tuesday" | "tu" | "вт" | "вторник" | "2" => Some(Day::Tue),
            "wed" | "wednesday" | "we" | "ср" | "среда" | "среду" | "3" => Some(Day::Wed),
            "thu" | "thur" | "thurs" | "thursday" | "th" | "чт" | "четверг" | "4" => Some(Day::Thu),
            "fri" | "friday" | "fr" | "пт" | "пятница" | "пятницу" | "5" => Some(Day::Fri),
            "sat" | "saturday" | "sa" | "сб" | "суббота" | "субботу" | "6" => Some(Day::Sat),
            "sun" | "sunday" | "su" | "вс" | "воскресенье" | "воскресение" | "7" | "0" => Some(Day::Sun),
            _ => None,
        }
    }

    pub fn from_weekday(w: Weekday) -> Self {
        match w {
            Weekday::Mon => Day::Mon,
            Weekday::Tue => Day::Tue,
            Weekday::Wed => Day::Wed,
            Weekday::Thu => Day::Thu,
            Weekday::Fri => Day::Fri,
            Weekday::Sat => Day::Sat,
            Weekday::Sun => Day::Sun,
        }
    }
}

impl Serialize for Day {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_code())
    }
}

impl<'de> Deserialize<'de> for Day {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DayVisitor;
        impl serde::de::Visitor<'_> for DayVisitor {
            type Value = Day;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a weekday code like mon..sun or пн..вс")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Day, E> {
                Day::parse(v).ok_or_else(|| E::custom(format!("неизвестный день недели: {v}")))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Day, E> {
                match v {
                    1 => Ok(Day::Mon),
                    2 => Ok(Day::Tue),
                    3 => Ok(Day::Wed),
                    4 => Ok(Day::Thu),
                    5 => Ok(Day::Fri),
                    6 => Ok(Day::Sat),
                    7 | 0 => Ok(Day::Sun),
                    _ => Err(E::custom(format!("день недели должен быть от 1 до 7, получено {v}"))),
                }
            }
        }
        deserializer.deserialize_any(DayVisitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatKind {
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Interval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeatRule {
    pub kind: RepeatKind,
    /// Local wall time "HH:MM" (required for daily, weekdays, weekly, monthly).
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "time")]
    pub at: Option<String>,
    /// Weekdays (required for weekly; optional filter for interval).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<Day>>,
    /// Day of month 1..=31 (required for monthly; clamped to month length).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
    /// Interval in minutes, minimum 5 (required for interval).
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "every_minutes")]
    pub every_minutes: Option<u32>,
    /// Optional daily window start "HH:MM" for interval.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "window_start", alias = "windowStart")]
    pub from: Option<String>,
    /// Optional daily window end "HH:MM" for interval (may be < from to cross midnight).
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "window_end", alias = "windowEnd")]
    pub to: Option<String>,
    /// Optional inclusive end date "YYYY-MM-DD" in local wall time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// Optional maximum total number of occurrences (>= 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// Parse "HH:MM" or "H:MM" into `NaiveTime`.
pub fn parse_hm(s: &str) -> Option<NaiveTime> {
    let s = s.trim();
    NaiveTime::parse_from_str(s, "%H:%M").ok()
}

fn format_hm(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

pub fn parse_ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

impl RepeatRule {
    /// Validate fields and normalize times ("9:00" → "09:00") and weekday sets.
    pub fn validate_and_normalize(&mut self) -> Result<(), String> {
        if let Some(ref mut until) = self.until {
            let d = parse_ymd(until).ok_or_else(|| "Дата until нужна в виде «ГГГГ-ММ-ДД»".to_string())?;
            *until = d.format("%Y-%m-%d").to_string();
        }
        if let Some(c) = self.count {
            if c == 0 {
                return Err("Количество повторений (count) должно быть не меньше 1".into());
            }
        }
        if let Some(ref mut days) = self.days {
            days.sort_unstable();
            days.dedup();
            if days.is_empty() {
                self.days = None;
            }
        }
        let norm_time = |opt: &mut Option<String>, label: &str| -> Result<Option<NaiveTime>, String> {
            match opt {
                Some(raw) => {
                    let t = parse_hm(raw).ok_or_else(|| format!("Поле {label} нужно в виде «ЧЧ:ММ»"))?;
                    *raw = format_hm(t);
                    Ok(Some(t))
                }
                None => Ok(None),
            }
        };

        match self.kind {
            RepeatKind::Daily | RepeatKind::Weekdays => {
                norm_time(&mut self.at, "at")?
                    .ok_or_else(|| "Для ежедневного повтора укажи время at («ЧЧ:ММ»)".to_string())?;
                self.days = None;
                self.day = None;
                self.every_minutes = None;
                self.from = None;
                self.to = None;
            }
            RepeatKind::Weekly => {
                norm_time(&mut self.at, "at")?
                    .ok_or_else(|| "Для еженедельного повтора укажи время at («ЧЧ:ММ»)".to_string())?;
                if self.days.as_ref().is_none_or(Vec::is_empty) {
                    return Err("Для еженедельного повтора укажи дни недели (days)".into());
                }
                self.day = None;
                self.every_minutes = None;
                self.from = None;
                self.to = None;
            }
            RepeatKind::Monthly => {
                norm_time(&mut self.at, "at")?
                    .ok_or_else(|| "Для ежемесячного повтора укажи время at («ЧЧ:ММ»)".to_string())?;
                match self.day {
                    Some(d) if (1..=31).contains(&d) => {}
                    _ => return Err("Для ежемесячного повтора укажи число месяца day от 1 до 31".into()),
                }
                self.days = None;
                self.every_minutes = None;
                self.from = None;
                self.to = None;
            }
            RepeatKind::Interval => {
                let mins = self.every_minutes.ok_or_else(|| "Для интервала укажи every_minutes".to_string())?;
                if mins < MIN_INTERVAL_MINUTES {
                    return Err(format!("Минимальный интервал повторения — {MIN_INTERVAL_MINUTES} минут"));
                }
                if mins > 7 * 24 * 60 {
                    return Err("Интервал слишком большой (максимум 7 дней)".into());
                }
                let from_t = norm_time(&mut self.from, "from")?;
                let to_t = norm_time(&mut self.to, "to")?;
                match (from_t, to_t) {
                    (Some(f), Some(t)) if f == t => {
                        return Err("Начало (from) и конец (to) окна не могут совпадать".into());
                    }
                    (Some(_), None) | (None, Some(_)) => {
                        return Err("Для окна интервала нужны оба поля: from и to".into());
                    }
                    _ => {}
                }
                self.at = None;
                self.day = None;
            }
        }
        Ok(())
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

fn clamped_month_date(year: i32, month: u32, day: u8) -> Option<NaiveDate> {
    let max_d = days_in_month(year, month);
    NaiveDate::from_ymd_opt(year, month, (day as u32).min(max_d))
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 { (year + 1, 1) } else { (year, month + 1) }
}

/// Pure local-wall-time calculation of the strictly next occurrence after `after`.
fn next_after_naive(rule: &RepeatRule, after: NaiveDateTime) -> Option<NaiveDateTime> {
    match rule.kind {
        RepeatKind::Daily => {
            let t = parse_hm(rule.at.as_deref()?)?;
            let cand = after.date().and_time(t);
            if cand > after {
                Some(cand)
            } else {
                Some((after.date() + Duration::days(1)).and_time(t))
            }
        }
        RepeatKind::Weekdays => {
            let t = parse_hm(rule.at.as_deref()?)?;
            const WEEKDAYS: [Day; 5] = [Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri];
            next_weekly_naive(&WEEKDAYS, t, after)
        }
        RepeatKind::Weekly => {
            let t = parse_hm(rule.at.as_deref()?)?;
            let days = rule.days.as_deref()?;
            if days.is_empty() {
                return None;
            }
            next_weekly_naive(days, t, after)
        }
        RepeatKind::Monthly => {
            let t = parse_hm(rule.at.as_deref()?)?;
            let day = rule.day?;
            if !(1..=31).contains(&day) {
                return None;
            }
            let (y, m) = (after.year(), after.month());
            let cand = clamped_month_date(y, m, day)?.and_time(t);
            if cand > after {
                Some(cand)
            } else {
                let (ny, nm) = next_month(y, m);
                Some(clamped_month_date(ny, nm, day)?.and_time(t))
            }
        }
        RepeatKind::Interval => {
            let mins = rule.every_minutes?;
            if mins < MIN_INTERVAL_MINUTES {
                return None;
            }
            let step = Duration::minutes(mins as i64);
            let allowed = |d: NaiveDate| -> bool {
                rule.days
                    .as_deref()
                    .is_none_or(|ds| ds.is_empty() || ds.contains(&Day::from_weekday(d.weekday())))
            };

            if let (Some(from_s), Some(to_s)) = (rule.from.as_deref(), rule.to.as_deref()) {
                let from_t = parse_hm(from_s)?;
                let to_t = parse_hm(to_s)?;
                if from_t == to_t {
                    return None;
                }
                let crosses_midnight = to_t < from_t;
                let start_offset = if crosses_midnight { -1 } else { 0 };
                let step_secs = step.num_seconds();
                for offset in start_offset..=8 {
                    let base_date = after.date() + Duration::days(offset);
                    if !allowed(base_date) {
                        continue;
                    }
                    let win_start = base_date.and_time(from_t);
                    let win_end = if crosses_midnight {
                        (base_date + Duration::days(1)).and_time(to_t)
                    } else {
                        base_date.and_time(to_t)
                    };
                    if after >= win_end {
                        continue;
                    }
                    if after < win_start {
                        return Some(win_start);
                    }
                    // win_start <= after < win_end: advance to the first grid slot strictly > after.
                    let elapsed_secs = (after - win_start).num_seconds();
                    let k = (elapsed_secs / step_secs) + 1;
                    let cand = win_start + Duration::seconds(k * step_secs);
                    if cand <= win_end {
                        return Some(cand);
                    }
                }
                None
            } else {
                let cand = after + step;
                if allowed(cand.date()) {
                    return Some(cand);
                }
                for offset in 1..=7 {
                    let d = cand.date() + Duration::days(offset);
                    if allowed(d) {
                        return d.and_hms_opt(0, 0, 0);
                    }
                }
                None
            }
        }
    }
}

fn next_weekly_naive(days: &[Day], t: NaiveTime, after: NaiveDateTime) -> Option<NaiveDateTime> {
    for offset in 0..=7 {
        let d = after.date() + Duration::days(offset);
        if days.contains(&Day::from_weekday(d.weekday())) {
            let cand = d.and_time(t);
            if cand > after {
                return Some(cand);
            }
        }
    }
    None
}

fn resolve_local<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> Option<DateTime<Tz>> {
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt),
        chrono::LocalResult::Ambiguous(early, _) => Some(early),
        chrono::LocalResult::None => {
            // Spring-forward DST gap: pick the first valid local minute after the gap.
            for mins in 1..=180 {
                let shifted = naive + Duration::minutes(mins);
                match tz.from_local_datetime(&shifted) {
                    chrono::LocalResult::Single(dt) => return Some(dt),
                    chrono::LocalResult::Ambiguous(early, _) => return Some(early),
                    chrono::LocalResult::None => continue,
                }
            }
            None
        }
    }
}

/// Strictly next occurrence of `rule` after `after_local`, respecting `until`
/// and `count` (given `done_count` occurrences already completed/skipped).
pub fn next_after<Tz: TimeZone>(
    rule: &RepeatRule,
    after_local: DateTime<Tz>,
    done_count: u32,
) -> Option<DateTime<Tz>> {
    if rule.count.is_some_and(|limit| done_count >= limit) {
        return None;
    }
    let cand = next_after_naive(rule, after_local.naive_local())?;
    if let Some(until_s) = rule.until.as_deref() {
        let until_date = parse_ymd(until_s)?;
        if cand.date() > until_date {
            return None;
        }
    }
    resolve_local(&after_local.timezone(), cand)
}

fn is_weekdays(days: &[Day]) -> bool {
    days == [Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri]
}

fn is_weekend(days: &[Day]) -> bool {
    days == [Day::Sat, Day::Sun]
}

fn format_days_list(days: &[Day]) -> String {
    let mut sorted = days.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if is_weekdays(&sorted) {
        return "по будням".into();
    }
    if is_weekend(&sorted) {
        return "по выходным".into();
    }
    match sorted.len() {
        0 => String::new(),
        1 => format!("по {}", sorted[0].short_ru()),
        2 => format!("по {} и {}", sorted[0].short_ru(), sorted[1].short_ru()),
        _ => {
            let head: Vec<&str> = sorted[..sorted.len() - 1].iter().map(|d| d.short_ru()).collect();
            format!("по {} и {}", head.join(", "), sorted[sorted.len() - 1].short_ru())
        }
    }
}

fn plural_ru(n: u32, one: &str, few: &str, many: &str) -> String {
    let d = n % 10;
    let h = n % 100;
    let word = if d == 1 && h != 11 {
        one
    } else if (2..=4).contains(&d) && !(12..=14).contains(&h) {
        few
    } else {
        many
    };
    format!("{n} {word}")
}

/// Human-readable Russian description of a repeat rule.
/// Examples:
/// - «каждый день в 08:00»
/// - «по будням в 09:00»
/// - «по пн и ср в 10:30»
/// - «каждые 2 часа с 09:00 до 21:00»
/// - «15-го числа каждого месяца в 12:00»
pub fn describe_rule(rule: &RepeatRule) -> String {
    let mut s = match rule.kind {
        RepeatKind::Daily => {
            let at = rule.at.as_deref().unwrap_or("00:00");
            format!("каждый день в {at}")
        }
        RepeatKind::Weekdays => {
            let at = rule.at.as_deref().unwrap_or("00:00");
            format!("по будням в {at}")
        }
        RepeatKind::Weekly => {
            let at = rule.at.as_deref().unwrap_or("00:00");
            let mut days = rule.days.clone().unwrap_or_default();
            days.sort_unstable();
            days.dedup();
            if days.len() == 7 {
                format!("каждый день в {at}")
            } else if is_weekdays(&days) {
                format!("по будням в {at}")
            } else if is_weekend(&days) {
                format!("по выходным в {at}")
            } else if days.len() == 1 {
                format!("{} в {at}", days[0].every_ru())
            } else {
                format!("{} в {at}", format_days_list(&days))
            }
        }
        RepeatKind::Monthly => {
            let at = rule.at.as_deref().unwrap_or("00:00");
            let day = rule.day.unwrap_or(1);
            format!("{day}-го числа каждого месяца в {at}")
        }
        RepeatKind::Interval => {
            let mins = rule.every_minutes.unwrap_or(MIN_INTERVAL_MINUTES);
            let freq = if mins % 60 == 0 {
                let h = mins / 60;
                if h == 1 {
                    "каждый час".to_string()
                } else {
                    let prefix = if h % 10 == 1 && h % 100 != 11 { "каждый" } else { "каждые" };
                    format!("{prefix} {}", plural_ru(h, "час", "часа", "часов"))
                }
            } else {
                let prefix = if mins % 10 == 1 && mins % 100 != 11 { "каждую" } else { "каждые" };
                format!("{prefix} {}", plural_ru(mins, "минуту", "минуты", "минут"))
            };
            let mut out = freq;
            if let (Some(f), Some(t)) = (rule.from.as_deref(), rule.to.as_deref()) {
                out.push_str(&format!(" с {f} до {t}"));
            }
            if let Some(days) = rule.days.as_deref() {
                if !days.is_empty() && days.len() < 7 {
                    out.push(' ');
                    out.push_str(&format_days_list(days));
                }
            }
            out
        }
    };

    if let Some(until) = rule.until.as_deref().and_then(parse_ymd) {
        s.push_str(&format!(", до {}", until.format("%d.%m.%Y")));
    }
    if let Some(count) = rule.count {
        s.push_str(&format!(", {}", plural_ru(count, "раз", "раза", "раз")));
    }
    s
}

/// «сегодня в 18:00», «завтра в 09:00», «27.11 в 10:00».
pub fn format_next_local<Tz: TimeZone>(due: DateTime<Tz>, now: DateTime<Tz>) -> String {
    let due_date = due.date_naive();
    let now_date = now.date_naive();
    let hm = due.time().format("%H:%M");
    if due_date == now_date {
        format!("сегодня в {hm}")
    } else if due_date == now_date + Duration::days(1) {
        format!("завтра в {hm}")
    } else {
        format!("{} в {hm}", due_date.format("%d.%m"))
    }
}

/// Capitalize the first Unicode character of a Russian sentence.
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600).unwrap()
    }

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<FixedOffset> {
        tz().with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn base_rule(kind: RepeatKind) -> RepeatRule {
        RepeatRule {
            kind,
            at: None,
            days: None,
            day: None,
            every_minutes: None,
            from: None,
            to: None,
            until: None,
            count: None,
        }
    }

    #[test]
    fn daily_before_exact_and_after() {
        let rule = RepeatRule { at: Some("09:00".into()), ..base_rule(RepeatKind::Daily) };
        // Before 09:00 -> same day
        assert_eq!(next_after(&rule, dt(2026, 11, 24, 8, 59), 0), Some(dt(2026, 11, 24, 9, 0)));
        // Exactly on 09:00 -> next day
        assert_eq!(next_after(&rule, dt(2026, 11, 24, 9, 0), 0), Some(dt(2026, 11, 25, 9, 0)));
        // After 09:00 -> next day
        assert_eq!(next_after(&rule, dt(2026, 11, 24, 9, 1), 0), Some(dt(2026, 11, 25, 9, 0)));
    }

    #[test]
    fn weekdays_skips_weekend() {
        let rule = RepeatRule { at: Some("09:00".into()), ..base_rule(RepeatKind::Weekdays) };
        // 2026-11-27 is Friday
        assert_eq!(next_after(&rule, dt(2026, 11, 27, 8, 0), 0), Some(dt(2026, 11, 27, 9, 0)));
        // Friday 09:00 exact -> Monday 2026-11-30 09:00
        assert_eq!(next_after(&rule, dt(2026, 11, 27, 9, 0), 0), Some(dt(2026, 11, 30, 9, 0)));
        // Saturday -> Monday
        assert_eq!(next_after(&rule, dt(2026, 11, 28, 12, 0), 0), Some(dt(2026, 11, 30, 9, 0)));
    }

    #[test]
    fn weekly_custom_days() {
        let rule = RepeatRule {
            at: Some("10:30".into()),
            days: Some(vec![Day::Mon, Day::Wed]),
            ..base_rule(RepeatKind::Weekly)
        };
        // 2026-11-23 is Monday
        assert_eq!(next_after(&rule, dt(2026, 11, 23, 10, 30), 0), Some(dt(2026, 11, 25, 10, 30)));
        // Wednesday 10:30 exact -> next Monday 2026-11-30
        assert_eq!(next_after(&rule, dt(2026, 11, 25, 10, 30), 0), Some(dt(2026, 11, 30, 10, 30)));
    }

    #[test]
    fn monthly_handles_short_months_and_leap_years() {
        let rule = RepeatRule {
            at: Some("12:00".into()),
            day: Some(31),
            ..base_rule(RepeatKind::Monthly)
        };
        // Jan 31 exact -> Feb 28 (2027 is not leap)
        assert_eq!(next_after(&rule, dt(2027, 1, 31, 12, 0), 0), Some(dt(2027, 2, 28, 12, 0)));
        // Feb 28 exact -> Mar 31
        assert_eq!(next_after(&rule, dt(2027, 2, 28, 12, 0), 0), Some(dt(2027, 3, 31, 12, 0)));
        // Mar 31 exact -> Apr 30
        assert_eq!(next_after(&rule, dt(2027, 3, 31, 12, 0), 0), Some(dt(2027, 4, 30, 12, 0)));
        // Leap year 2028: Jan 31 exact -> Feb 29
        assert_eq!(next_after(&rule, dt(2028, 1, 31, 12, 0), 0), Some(dt(2028, 2, 29, 12, 0)));
        // Leap year Feb 29 exact -> Mar 31
        assert_eq!(next_after(&rule, dt(2028, 2, 29, 12, 0), 0), Some(dt(2028, 3, 31, 12, 0)));
        // Year rollover: Dec 31 exact -> Jan 31 next year
        assert_eq!(next_after(&rule, dt(2026, 12, 31, 12, 0), 0), Some(dt(2027, 1, 31, 12, 0)));
    }

    #[test]
    fn interval_unwindowed_and_windowed() {
        let plain = RepeatRule { every_minutes: Some(120), ..base_rule(RepeatKind::Interval) };
        assert_eq!(next_after(&plain, dt(2026, 11, 24, 10, 15), 0), Some(dt(2026, 11, 24, 12, 15)));

        let win = RepeatRule {
            every_minutes: Some(120),
            from: Some("09:00".into()),
            to: Some("21:00".into()),
            ..base_rule(RepeatKind::Interval)
        };
        // Before window -> 09:00
        assert_eq!(next_after(&win, dt(2026, 11, 24, 7, 30), 0), Some(dt(2026, 11, 24, 9, 0)));
        // Exactly on 09:00 -> 11:00
        assert_eq!(next_after(&win, dt(2026, 11, 24, 9, 0), 0), Some(dt(2026, 11, 24, 11, 0)));
        // Mid-slot 10:15 -> 11:00
        assert_eq!(next_after(&win, dt(2026, 11, 24, 10, 15), 0), Some(dt(2026, 11, 24, 11, 0)));
        // 19:00 exact -> 21:00 (inclusive end)
        assert_eq!(next_after(&win, dt(2026, 11, 24, 19, 0), 0), Some(dt(2026, 11, 24, 21, 0)));
        // 21:00 exact -> next day 09:00
        assert_eq!(next_after(&win, dt(2026, 11, 24, 21, 0), 0), Some(dt(2026, 11, 25, 9, 0)));
    }

    #[test]
    fn interval_window_crossing_midnight_and_weekday_filter() {
        let night = RepeatRule {
            every_minutes: Some(60),
            from: Some("22:00".into()),
            to: Some("02:00".into()),
            days: Some(vec![Day::Fri]),
            ..base_rule(RepeatKind::Interval)
        };
        // Friday 2026-11-27 21:00 -> Fri 22:00
        assert_eq!(next_after(&night, dt(2026, 11, 27, 21, 0), 0), Some(dt(2026, 11, 27, 22, 0)));
        // Friday 23:30 -> Saturday 00:00 (still inside Friday's overnight window)
        assert_eq!(next_after(&night, dt(2026, 11, 27, 23, 30), 0), Some(dt(2026, 11, 28, 0, 0)));
        // Saturday 01:00 -> Saturday 02:00
        assert_eq!(next_after(&night, dt(2026, 11, 28, 1, 0), 0), Some(dt(2026, 11, 28, 2, 0)));
        // Saturday 02:00 -> next Friday 2026-12-04 22:00
        assert_eq!(next_after(&night, dt(2026, 11, 28, 2, 0), 0), Some(dt(2026, 12, 4, 22, 0)));
    }

    #[test]
    fn until_and_count_stop_the_series() {
        let with_until = RepeatRule {
            at: Some("09:00".into()),
            until: Some("2026-11-25".into()),
            ..base_rule(RepeatKind::Daily)
        };
        assert_eq!(next_after(&with_until, dt(2026, 11, 24, 9, 0), 0), Some(dt(2026, 11, 25, 9, 0)));
        assert_eq!(next_after(&with_until, dt(2026, 11, 25, 9, 0), 0), None);

        let with_count = RepeatRule {
            at: Some("09:00".into()),
            count: Some(3),
            ..base_rule(RepeatKind::Daily)
        };
        assert!(next_after(&with_count, dt(2026, 11, 24, 9, 0), 2).is_some());
        assert_eq!(next_after(&with_count, dt(2026, 11, 24, 9, 0), 3), None);
    }

    #[test]
    fn describe_rule_russian_examples() {
        let daily = RepeatRule { at: Some("08:00".into()), ..base_rule(RepeatKind::Daily) };
        assert_eq!(describe_rule(&daily), "каждый день в 08:00");

        let wd = RepeatRule { at: Some("09:00".into()), ..base_rule(RepeatKind::Weekdays) };
        assert_eq!(describe_rule(&wd), "по будням в 09:00");

        let wk = RepeatRule {
            at: Some("10:30".into()),
            days: Some(vec![Day::Wed, Day::Mon]),
            ..base_rule(RepeatKind::Weekly)
        };
        assert_eq!(describe_rule(&wk), "по пн и ср в 10:30");

        let mon = RepeatRule {
            at: Some("10:00".into()),
            days: Some(vec![Day::Mon]),
            ..base_rule(RepeatKind::Weekly)
        };
        assert_eq!(describe_rule(&mon), "каждый понедельник в 10:00");

        let weekend = RepeatRule {
            at: Some("11:00".into()),
            days: Some(vec![Day::Sun, Day::Sat]),
            ..base_rule(RepeatKind::Weekly)
        };
        assert_eq!(describe_rule(&weekend), "по выходным в 11:00");

        let interval = RepeatRule {
            every_minutes: Some(120),
            from: Some("09:00".into()),
            to: Some("21:00".into()),
            ..base_rule(RepeatKind::Interval)
        };
        assert_eq!(describe_rule(&interval), "каждые 2 часа с 09:00 до 21:00");

        let interval_min = RepeatRule {
            every_minutes: Some(30),
            from: Some("09:00".into()),
            to: Some("18:00".into()),
            days: Some(vec![Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri]),
            ..base_rule(RepeatKind::Interval)
        };
        assert_eq!(describe_rule(&interval_min), "каждые 30 минут с 09:00 до 18:00 по будням");

        let monthly = RepeatRule {
            at: Some("12:00".into()),
            day: Some(15),
            ..base_rule(RepeatKind::Monthly)
        };
        assert_eq!(describe_rule(&monthly), "15-го числа каждого месяца в 12:00");
    }

    #[test]
    fn validation_and_day_aliases() {
        let mut r: RepeatRule = serde_json::from_value(serde_json::json!({
            "kind": "weekly",
            "at": "9:05",
            "days": ["ср", "пн", "Mon"]
        }))
        .unwrap();
        r.validate_and_normalize().unwrap();
        assert_eq!(r.at.as_deref(), Some("09:05"));
        assert_eq!(r.days, Some(vec![Day::Mon, Day::Wed]));

        let mut too_fast = RepeatRule { every_minutes: Some(3), ..base_rule(RepeatKind::Interval) };
        assert!(too_fast.validate_and_normalize().is_err());

        let mut half_window = RepeatRule {
            every_minutes: Some(30),
            from: Some("09:00".into()),
            ..base_rule(RepeatKind::Interval)
        };
        assert!(half_window.validate_and_normalize().is_err());
    }

    #[test]
    fn format_next_local_today_tomorrow_and_date() {
        let now = dt(2026, 11, 24, 13, 0);
        assert_eq!(format_next_local(dt(2026, 11, 24, 18, 30), now), "сегодня в 18:30");
        assert_eq!(format_next_local(dt(2026, 11, 25, 9, 0), now), "завтра в 09:00");
        assert_eq!(format_next_local(dt(2026, 11, 30, 10, 0), now), "30.11 в 10:00");
    }
}
