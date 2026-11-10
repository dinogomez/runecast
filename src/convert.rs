//! Conversions typed into the launcher: units (`5 km in mi`, `72f to c`), currency
//! (`100 usd to php`) and time zones (`3pm PST in Manila`, `time in Tokyo`).

use std::collections::HashMap;
use std::time::Duration;

use jiff::Zoned;
use jiff::tz::{Offset, TimeZone};
use serde::{Deserialize, Serialize};

/// An answer: what to show, and the plain value ↩ copies.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub title: String,
    pub subtitle: String,
    pub copy: String,
}

/// The first conversion `query` reads as.
pub fn convert(query: &str, rates: &Rates, now: &Zoned) -> Option<Answer> {
    let q = query.trim().to_lowercase();
    units(&q).or_else(|| currency(&q, rates)).or_else(|| time(&q, now))
}

/// `lhs in rhs` or `lhs to rhs`, split at the last separator.
fn split(q: &str) -> Option<(&str, &str)> {
    [" in ", " to ", " = "].iter().filter_map(|sep| q.rfind(sep).map(|i| (i, sep.len()))).max()
        .map(|(i, n)| (q[..i].trim(), q[i + n..].trim()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
}

/// A leading number and what follows it: "5km" → (5, "km"), "1,200 usd" → (1200, "usd").
fn amount(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-')).unwrap_or(s.len());
    let n: f64 = s[..end].replace(',', "").parse().ok()?;
    Some((n, s[end..].trim()))
}

/// At most `places` decimals, grouped thousands.
fn round(v: f64, places: i32) -> String {
    let p = 10f64.powi(places);
    crate::calc::format((v * p).round() / p)
}

// ------------------------------------------------------------------------------------------
// Units

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dim {
    Length,
    Mass,
    Volume,
    Data,
    Duration,
    Temperature,
}

/// Every unit: its names, what it measures, and its size in that dimension's base unit
/// (metres, grams, litres, bytes, seconds; temperature is handled apart).
const UNITS: &[(&[&str], Dim, f64)] = &[
    (&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], Dim::Length, 0.001),
    (&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], Dim::Length, 0.01),
    (&["m", "meter", "meters", "metre", "metres"], Dim::Length, 1.0),
    (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], Dim::Length, 1000.0),
    (&["in", "inch", "inches", "\""], Dim::Length, 0.0254),
    (&["ft", "foot", "feet", "'"], Dim::Length, 0.3048),
    (&["yd", "yard", "yards"], Dim::Length, 0.9144),
    (&["mi", "mile", "miles"], Dim::Length, 1609.344),
    (&["mg", "milligram", "milligrams"], Dim::Mass, 0.001),
    (&["g", "gram", "grams"], Dim::Mass, 1.0),
    (&["kg", "kilo", "kilos", "kilogram", "kilograms"], Dim::Mass, 1000.0),
    (&["oz", "ounce", "ounces"], Dim::Mass, 28.349523125),
    (&["lb", "lbs", "pound", "pounds"], Dim::Mass, 453.59237),
    (&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], Dim::Volume, 0.001),
    (&["l", "liter", "liters", "litre", "litres"], Dim::Volume, 1.0),
    (&["floz", "fl oz", "fluid ounce", "fluid ounces"], Dim::Volume, 0.0295735295625),
    (&["cup", "cups"], Dim::Volume, 0.2365882365),
    (&["gal", "gallon", "gallons"], Dim::Volume, 3.785411784),
    (&["b", "byte", "bytes"], Dim::Data, 1.0),
    (&["kb", "kilobyte", "kilobytes"], Dim::Data, 1e3),
    (&["mb", "megabyte", "megabytes"], Dim::Data, 1e6),
    (&["gb", "gigabyte", "gigabytes"], Dim::Data, 1e9),
    (&["tb", "terabyte", "terabytes"], Dim::Data, 1e12),
    (&["kib"], Dim::Data, 1024.0),
    (&["mib"], Dim::Data, 1048576.0),
    (&["gib"], Dim::Data, 1073741824.0),
    (&["ms", "millisecond", "milliseconds"], Dim::Duration, 0.001),
    (&["s", "sec", "secs", "second", "seconds"], Dim::Duration, 1.0),
    (&["min", "mins", "minute", "minutes"], Dim::Duration, 60.0),
    (&["h", "hr", "hrs", "hour", "hours"], Dim::Duration, 3600.0),
    (&["d", "day", "days"], Dim::Duration, 86400.0),
    (&["wk", "week", "weeks"], Dim::Duration, 604800.0),
    (&["c", "°c", "celsius"], Dim::Temperature, 0.0),
    (&["f", "°f", "fahrenheit"], Dim::Temperature, 1.0),
    (&["k", "kelvin"], Dim::Temperature, 2.0),
];

fn unit(name: &str) -> Option<(&'static str, Dim, f64)> {
    let name = name.trim().trim_end_matches('.');
    UNITS.iter().find(|(names, _, _)| names.contains(&name)).map(|(names, dim, size)| (names[0], *dim, *size))
}

fn units(q: &str) -> Option<Answer> {
    let (lhs, rhs) = split(q)?;
    let (n, from) = amount(lhs)?;
    let ((from, dim, a), (to, dim2, b)) = (unit(from)?, unit(rhs)?);
    if dim != dim2 || from == to {
        return None;
    }
    let v = if dim == Dim::Temperature {
        let kelvin = match a as u8 { 0 => n + 273.15, 1 => (n - 32.0) * 5.0 / 9.0 + 273.15, _ => n };
        match b as u8 { 0 => kelvin - 273.15, 1 => (kelvin - 273.15) * 9.0 / 5.0 + 32.0, _ => kelvin }
    } else {
        n * a / b
    };
    let label = |u: &str| match u { "c" => "°C".to_string(), "f" => "°F".to_string(), "k" => "K".to_string(), u => u.to_string() };
    let value = round(v, 4);
    Some(Answer { title: format!("{value} {}", label(to)), subtitle: format!("{} {} =", round(n, 4), label(from)), copy: value.replace(',', "") })
}

// ------------------------------------------------------------------------------------------
// Currency

/// Units of each currency per US dollar, saved beside the config and refreshed twice a day.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Rates {
    pub fetched: u64,
    pub rates: HashMap<String, f64>,
}

const RATES_URL: &str = "https://open.er-api.com/v6/latest/USD";
const STALE: u64 = 12 * 3600;

impl Rates {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn is_stale(&self, now: u64) -> bool {
        self.rates.is_empty() || now.saturating_sub(self.fetched) > STALE
    }

    /// Fresh rates from the network (with curl, which macOS and most Linux systems have), saved.
    pub fn fetch(now: u64) -> Result<Self, String> {
        let out = std::process::Command::new("curl").args(["-sf", "--max-time", "10", RATES_URL]).output().map_err(|e| e.to_string())?;
        #[derive(Deserialize)]
        struct Reply {
            result: String,
            rates: HashMap<String, f64>,
        }
        let reply: Reply = serde_json::from_slice(&out.stdout).map_err(|e| format!("unexpected reply: {e}"))?;
        if reply.result != "success" {
            return Err(format!("the rates service said {}", reply.result));
        }
        let rates = Rates { fetched: now, rates: reply.rates.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect() };
        let _ = std::fs::create_dir_all(crate::config::dir());
        let _ = std::fs::write(path(), serde_json::to_string(&rates).unwrap());
        Ok(rates)
    }

    /// Fixed rates for headless scenes and tests.
    pub fn demo() -> Self {
        let rates = [("usd", 1.0), ("php", 58.0), ("eur", 0.92), ("jpy", 150.0), ("gbp", 0.79)];
        Rates { fetched: 0, rates: rates.iter().map(|(k, v)| (k.to_string(), *v)).collect() }
    }
}

fn path() -> std::path::PathBuf {
    crate::config::dir().join("rates.json")
}

/// Fetches rates on a thread when they're stale, then hands them to `on_rates`.
pub fn refresh_in_background(stale: bool, on_rates: impl FnOnce(Rates) + Send + 'static) {
    if !stale {
        return;
    }
    std::thread::spawn(move || {
        // Give the launch a moment first; nothing waits on this.
        std::thread::sleep(Duration::from_secs(2));
        match Rates::fetch(crate::usage::now()) {
            Ok(rates) => on_rates(rates),
            Err(e) => eprintln!("runecast: currency rates aren't available: {e}"),
        }
    });
}

fn currency_code(s: &str) -> Option<String> {
    let code = match s.trim() {
        "$" | "dollar" | "dollars" => "usd",
        "€" | "euro" | "euros" => "eur",
        "£" | "pound sterling" => "gbp",
        "¥" | "yen" => "jpy",
        "₱" | "peso" | "pesos" => "php",
        s if s.len() == 3 && s.chars().all(|c| c.is_ascii_alphabetic()) => s,
        _ => return None,
    };
    Some(code.to_string())
}

fn currency(q: &str, rates: &Rates) -> Option<Answer> {
    let (lhs, rhs) = split(q)?;
    // "$100" as well as "100 usd".
    let (n, from) = match amount(lhs) {
        Some((n, unit)) => (n, unit.to_string()),
        None => {
            let symbol = lhs.chars().next()?;
            (amount(lhs[symbol.len_utf8()..].trim())?.0, symbol.to_string())
        }
    };
    let (from, to) = (currency_code(&from)?, currency_code(rhs)?);
    let (a, b) = (rates.rates.get(&from)?, rates.rates.get(&to)?);
    let value = round(n / a * b, 2);
    Some(Answer {
        title: format!("{value} {}", to.to_uppercase()),
        subtitle: format!("{} {} =", round(n, 2), from.to_uppercase()),
        copy: value.replace(',', ""),
    })
}

// ------------------------------------------------------------------------------------------
// Time zones

/// A zone by abbreviation (fixed offsets: PST is always UTC−8), UTC offset ("utc+5", "gmt-3:30"),
/// city, country or IANA name. Any city with its own IANA zone works ("tashkent", "buenos aires").
fn zone(name: &str) -> Option<(TimeZone, String)> {
    let name = name.trim();
    let fixed = |hours: f64| {
        let offset = Offset::from_seconds((hours * 3600.0) as i32).ok()?;
        Some((TimeZone::fixed(offset), name.to_uppercase()))
    };
    // Where an abbreviation means several zones, the one people most likely mean: IST is India,
    // CST and AST are North American, BST is British, AMT is Armenia.
    let hours = match name {
        "utc" | "gmt" | "z" | "ut" | "wet" | "azost" | "egst" => Some(0.0),
        "lint" => Some(14.0), "tot" | "nzdt" | "phot" | "tkt" => Some(13.0), "chast" => Some(12.75),
        "nzst" | "fjt" | "mht" | "tvt" | "gilt" | "pett" | "anat" | "wakt" | "nrt" => Some(12.0),
        "sbt" | "nct" | "magt" | "vut" | "sakt" | "srett" | "nft" | "kost" | "pont" => Some(11.0),
        "lhdt" | "aedt" => Some(11.0), "acdt" | "lhst" => Some(10.5),
        "aest" | "chst" | "vlat" | "pgt" | "chut" => Some(10.0), "acst" => Some(9.5),
        "jst" | "kst" | "wit" | "yakt" | "tlt" | "pwt" => Some(9.0), "acwst" => Some(8.75),
        "pht" | "phst" | "sgt" | "hkt" | "awst" | "wita" | "myt" | "ulat" | "irkt" | "bnt" => Some(8.0),
        "ict" | "wib" | "novt" | "krat" | "hovt" | "cxt" | "davt" => Some(7.0),
        "mmt" | "cct" => Some(6.5), "btt" | "kgt" | "omst" | "iot" | "vost" => Some(6.0),
        "npt" => Some(5.75), "ist" | "slst" => Some(5.5),
        "pkt" | "uzt" | "tjt" | "tmt" | "yekt" | "mvt" | "almt" | "aqtt" | "orat" | "tft" | "mawt" => Some(5.0),
        "aft" | "irdt" => Some(4.5),
        "gst" | "azt" | "get" | "amt" | "mut" | "ret" | "sct" | "samt" => Some(4.0), "irst" => Some(3.5),
        "msk" | "trt" | "eat" | "eest" | "idt" | "fet" | "syot" => Some(3.0),
        "eet" | "cest" | "mest" | "cat" | "sast" => Some(2.0),
        "cet" | "met" | "bst" | "west" | "wat" => Some(1.0),
        "azot" | "cvt" | "egt" => Some(-1.0), "fnt" | "brst" | "pmdt" | "uyst" | "wgt" => Some(-2.0),
        "ndt" => Some(-2.5),
        "art" | "brt" | "adt" | "clst" | "pyst" | "uyt" | "gft" | "srt" | "fkst" | "pmst" | "rott" => Some(-3.0),
        "nst" => Some(-3.5),
        "ast" | "edt" | "clt" | "vet" | "bot" | "pyt" | "gyt" | "fkt" => Some(-4.0),
        "est" | "cdt" | "cot" | "pet" | "ect" | "acrt" | "easst" => Some(-5.0),
        "cst" | "mdt" | "galt" | "east" => Some(-6.0),
        "mst" | "pdt" => Some(-7.0), "pst" | "akdt" => Some(-8.0), "akst" | "hdt" | "gamt" => Some(-9.0),
        "mart" => Some(-9.5), "hst" | "hast" | "ckt" | "taht" => Some(-10.0), "sst" | "nut" => Some(-11.0),
        _ => None,
    };
    if let Some(h) = hours {
        return fixed(h);
    }
    if let Some(found) = utc_offset(name) {
        return Some(found);
    }
    let iana = match name {
        "pt" | "pacific" | "los angeles" | "la" | "san francisco" | "sf" | "seattle" | "vancouver" => "America/Los_Angeles",
        "et" | "eastern" | "new york" | "nyc" | "ny" | "toronto" | "boston" | "miami" => "America/New_York",
        "ct" | "central" | "chicago" | "austin" | "dallas" | "houston" | "mexico city" => "America/Chicago",
        "mt" | "mountain" | "denver" => "America/Denver",
        "manila" | "philippines" | "ph" | "cebu" | "davao" => "Asia/Manila",
        "tokyo" | "japan" => "Asia/Tokyo",
        "seoul" | "korea" => "Asia/Seoul",
        "singapore" => "Asia/Singapore",
        "hong kong" => "Asia/Hong_Kong",
        "shanghai" | "beijing" | "china" => "Asia/Shanghai",
        "bangkok" => "Asia/Bangkok",
        "jakarta" => "Asia/Jakarta",
        "dubai" => "Asia/Dubai",
        "india" | "mumbai" | "delhi" | "bangalore" | "bengaluru" => "Asia/Kolkata",
        "london" | "uk" => "Europe/London",
        "paris" | "france" => "Europe/Paris",
        "berlin" | "germany" => "Europe/Berlin",
        "amsterdam" => "Europe/Amsterdam",
        "madrid" => "Europe/Madrid",
        "sydney" | "melbourne" => "Australia/Sydney",
        "auckland" | "new zealand" => "Pacific/Auckland",
        "sao paulo" | "são paulo" => "America/Sao_Paulo",
        _ => "",
    };
    let label = |s: &str| s.split(' ').map(|w| w[..1].to_uppercase() + &w[1..]).collect::<Vec<_>>().join(" ");
    if !iana.is_empty() {
        return Some((TimeZone::get(iana).ok()?, label(name)));
    }
    if let Some((city, iana)) = cities().iter().find(|(city, _)| city.eq_ignore_ascii_case(name)) {
        return Some((TimeZone::get(iana).ok()?, city.clone()));
    }
    // An IANA name typed as such: "asia/manila", "america/new_york".
    let typed: String = name.split('/').map(|part| part.split('_').map(|w| w.get(..1).map_or(String::new(), |c| c.to_uppercase()) + w.get(1..).unwrap_or(""))
        .collect::<Vec<_>>().join("_")).collect::<Vec<_>>().join("/");
    name.contains('/').then(|| TimeZone::get(&typed).ok().map(|tz| (tz, typed.clone()))).flatten()
}

/// The place a World Clock entry or query names, by the label to save and show it under: "Tokyo",
/// "PST", "UTC+5:30", "Buenos Aires". The label reads back as the same zone.
pub fn clock_zone(name: &str) -> Option<(TimeZone, String)> {
    zone(&name.trim().to_lowercase())
}

/// Whether a root query asks for the World Clock, so the panel pins its times above the other
/// rows: a start of "time" (three letters or more), "clock" or "clocks", or "world clock" typed past
/// "world". "time in tokyo" is a conversion, not this.
pub fn asks_for_clocks(query: &str) -> bool {
    let q = query.trim().to_lowercase();
    (q.len() >= 3 && "time".starts_with(&q)) || q == "clock" || q == "clocks" || (q.len() >= 7 && "world clock".starts_with(&q))
}

/// What a World Clock row shows for `zone` at `now`: the time there, and its day and offset next
/// to here, such as ("4:05 PM", "Tomorrow · 16h ahead").
pub fn world_clock(zone: &TimeZone, now: &Zoned) -> (String, String) {
    let there = now.with_time_zone(zone.clone());
    // Across the date line two places can be two days apart; those say the date.
    let day = match there.date().since(now.date()).map(|d| d.get_days()).unwrap_or(0) {
        0 => "Today".to_string(),
        1 => "Tomorrow".to_string(),
        -1 => "Yesterday".to_string(),
        _ => there.strftime("%a %-d %b").to_string(),
    };
    let diff = (there.offset().seconds() - now.offset().seconds()) / 60;
    let (h, m) = (diff.abs() / 60, diff.abs() % 60);
    let span = match (h, m) {
        (0, 0) => return (there.strftime("%-I:%M %p").to_string(), format!("{day} · same time as here")),
        (_, 0) => format!("{h}h"),
        (0, _) => format!("{m}m"),
        _ => format!("{h}h {m}m"),
    };
    (there.strftime("%-I:%M %p").to_string(), format!("{day} · {span} {}", if diff > 0 { "ahead" } else { "behind" }))
}

/// "utc+5", "gmt-3:30", "utc+0545", "+8" as a fixed zone labelled like "UTC+5:30".
fn utc_offset(name: &str) -> Option<(TimeZone, String)> {
    let rest = name.strip_prefix("utc").or_else(|| name.strip_prefix("gmt")).unwrap_or(name).trim();
    let sign = match rest.chars().next()? {
        '+' => 1,
        '-' | '−' => -1,
        _ => return None,
    };
    let digits = rest[rest.chars().next()?.len_utf8()..].trim();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return None;
    }
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) => (h, m),
        None if digits.len() > 2 => digits.split_at(digits.len() - 2),
        None => (digits, "0"),
    };
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    if h > 14 || m >= 60 || (h == 14 && m > 0) {
        return None;
    }
    let offset = Offset::from_seconds(sign * (h * 3600 + m * 60)).ok()?;
    let label = format!("UTC{}{h}{}", if sign > 0 { '+' } else { '−' }, if m > 0 { format!(":{m:02}") } else { String::new() });
    Some((TimeZone::fixed(offset), label))
}

/// Every city in the time zone database with its IANA name, such as ("Buenos Aires",
/// "America/Argentina/Buenos_Aires"), read once. Fixed "Etc/GMT+5"-style zones are left out:
/// their signs run backwards, and `utc_offset` reads offsets anyway.
fn cities() -> &'static [(String, String)] {
    static CITIES: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    CITIES.get_or_init(|| {
        jiff::tz::db().available().map(|n| n.as_str().to_string())
            .filter(|n| n.contains('/') && !n.chars().any(|c| c.is_ascii_digit()) && !n.starts_with("Etc/")
                && !n.starts_with("posix/") && !n.starts_with("right/"))
            .map(|n| (n.rsplit('/').next().unwrap_or(&n).replace('_', " "), n))
            .collect()
    })
}

/// "3pm", "3:30 pm", "15:00", "noon", "midnight" as hour and minute.
fn clock(s: &str) -> Option<(i8, i8)> {
    let s = s.trim().replace(' ', "");
    match s.as_str() {
        "noon" => return Some((12, 0)),
        "midnight" => return Some((0, 0)),
        _ => {}
    }
    let (digits, meridiem) = match s.strip_suffix("am").or_else(|| s.strip_suffix("a")) {
        Some(d) => (d, Some(false)),
        None => match s.strip_suffix("pm").or_else(|| s.strip_suffix("p")) {
            Some(d) => (d, Some(true)),
            None => (s.as_str(), None),
        },
    };
    let (h, m) = digits.split_once(':').unwrap_or((digits, "0"));
    let (mut h, m): (i8, i8) = (h.parse().ok()?, m.parse().ok()?);
    if !(0..60).contains(&m) {
        return None;
    }
    match meridiem {
        Some(pm) if (1..=12).contains(&h) => h = h % 12 + if pm { 12 } else { 0 },
        Some(_) => return None,
        // A bare number isn't a time ("5 in km" is a unit question); 15:00 is.
        None if !digits.contains(':') || !(0..24).contains(&h) => return None,
        None => {}
    }
    Some((h, m))
}

fn time(q: &str, now: &Zoned) -> Option<Answer> {
    let show = |z: &Zoned| z.strftime("%-I:%M %p").to_string();
    let day = |z: &Zoned| z.strftime("%a %-d %b").to_string();
    // "time in tokyo", "now in london", "tokyo time".
    let place = q.strip_prefix("time in ").or_else(|| q.strip_prefix("now in ")).or_else(|| q.strip_suffix(" time"));
    if let Some(place) = place {
        let (tz, label) = zone(place)?;
        let there = now.with_time_zone(tz);
        return Some(Answer { title: show(&there), subtitle: format!("Now in {label} · {}", day(&there)), copy: show(&there) });
    }
    let (lhs, rhs) = split(q)?;
    let (to, to_label) = zone(rhs)?;
    // "3pm pst in manila": the time, then its zone; without one, the time here.
    let zoned_time = lhs.match_indices(' ').find_map(|(i, _)| Some((clock(&lhs[..i])?, zone(&lhs[i + 1..])?)));
    let (at, from, from_label) = match zoned_time {
        Some((at, (tz, label))) => (at, tz, label),
        None => (clock(lhs)?, now.time_zone().clone(), "here".to_string()),
    };
    let start = now.with_time_zone(from.clone()).date().at(at.0, at.1, 0, 0).to_zoned(from).ok()?;
    let there = start.with_time_zone(to);
    let shift = match there.date().since(start.date()).ok()?.get_days() {
        0 => String::new(),
        1 => " (next day)".into(),
        -1 => " (previous day)".into(),
        _ => String::new(),
    };
    Some(Answer {
        title: format!("{}{shift}", show(&there)),
        subtitle: format!("{} {from_label} in {to_label} · {}", show(&start), day(&there)),
        copy: show(&there),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Zoned {
        // A Monday in November: no DST in Manila, standard time in the US.
        "2026-11-09T10:00:00+08:00[Asia/Manila]".parse().unwrap()
    }

    fn title(q: &str) -> Option<String> {
        convert(q, &Rates::demo(), &now()).map(|a| a.title)
    }

    #[test]
    fn units_convert_within_a_dimension_only() {
        assert_eq!(title("5 km in mi").as_deref(), Some("3.1069 mi"));
        assert_eq!(title("5km to miles").as_deref(), Some("3.1069 mi"));
        assert_eq!(title("72f in c").as_deref(), Some("22.2222 °C"));
        assert_eq!(title("-40 c to f").as_deref(), Some("-40 °F"));
        assert_eq!(title("1 gib in mb").as_deref(), Some("1,073.7418 mb"));
        assert_eq!(title("2 kg to lb").as_deref(), Some("4.4092 lb"));
        assert_eq!(title("5 km in kg"), None, "length isn't mass");
        assert_eq!(title("safari in chrome"), None);
    }

    #[test]
    fn currency_uses_rates_through_the_dollar() {
        assert_eq!(title("100 usd to php").as_deref(), Some("5,800 PHP"));
        assert_eq!(title("1,000 php in eur").as_deref(), Some("15.86 EUR"));
        assert_eq!(title("$20 in jpy").as_deref(), Some("3,000 JPY"));
        assert_eq!(convert("100 usd to php", &Rates::default(), &now()), None, "no rates, no answer");
        assert_eq!(title("100 usd to xyz"), None);
    }

    #[test]
    fn times_convert_between_zones_and_say_when_the_day_changes() {
        // 3pm PST is 23:00 UTC, 7am the next day in Manila.
        assert_eq!(title("3pm pst in manila").as_deref(), Some("7:00 AM (next day)"));
        assert_eq!(title("9:30 am manila to london").as_deref(), Some("1:30 AM"));
        assert_eq!(title("time in tokyo").as_deref(), Some("11:00 AM"));
        assert_eq!(title("tokyo time").as_deref(), Some("11:00 AM"));
        assert_eq!(title("15:00 in new york").as_deref(), Some("2:00 AM"));
        assert_eq!(title("8pm new york to manila").as_deref(), Some("9:00 AM (next day)"));
        assert_eq!(title("25:00 in tokyo"), None);
        assert_eq!(title("time in atlantis"), None);
    }

    #[test]
    fn only_time_clock_or_world_clock_asks_for_the_clocks() {
        for q in ["tim", "Time", " time ", "clock", "clocks", "world c", "World Clock"] {
            assert!(asks_for_clocks(q), "{q:?}");
        }
        for q in ["ti", "timer", "time in tokyo", "3pm pst in manila", "clo", "world", "word"] {
            assert!(!asks_for_clocks(q), "{q:?}");
        }
    }

    #[test]
    fn world_clock_labels_read_back_and_say_the_day_and_offset() {
        for typed in ["tokyo", "PST", "utc+5:30", "buenos aires", "asia/manila", "new york"] {
            let (tz, label) = clock_zone(typed).unwrap();
            assert_eq!(clock_zone(&label).unwrap().0, tz, "{typed} saved as {label}");
        }
        assert_eq!(clock_zone("narnia"), None);
        // 22:30 in Manila (UTC+8).
        let now: Zoned = "2026-11-10T22:30:00+08:00[Asia/Manila]".parse().unwrap();
        let at = |name: &str| world_clock(&clock_zone(name).unwrap().0, &now);
        assert_eq!(at("tokyo"), ("11:30 PM".into(), "Today · 1h ahead".into()));
        assert_eq!(at("utc+9:30"), ("12:00 AM".into(), "Tomorrow · 1h 30m ahead".into()));
        assert_eq!(at("pst"), ("6:30 AM".into(), "Today · 16h behind".into()));
        assert_eq!(at("singapore"), ("10:30 PM".into(), "Today · same time as here".into()));
        let early: Zoned = "2026-11-10T01:00:00+08:00[Asia/Manila]".parse().unwrap();
        assert_eq!(world_clock(&clock_zone("london").unwrap().0, &early).1, "Yesterday · 8h behind");
        // Across the date line: 11:30 PM on the 10th in Pago Pago is 12:30 AM on the 12th in Kiritimati.
        let pago: Zoned = "2026-11-10T23:30:00-11:00[Pacific/Pago_Pago]".parse().unwrap();
        assert_eq!(world_clock(&clock_zone("kiritimati").unwrap().0, &pago), ("12:30 AM".into(), "Thu 12 Nov · 25h ahead".into()));
    }

    #[test]
    fn any_offset_abbreviation_or_database_city_is_a_zone() {
        // 2:45 PM in Manila is 06:45 UTC.
        assert_eq!(title("2:45pm to utc+5").as_deref(), Some("11:45 AM"));
        assert_eq!(title("2:45PM to UZT").as_deref(), Some("11:45 AM"));
        assert_eq!(title("2:45pm to tashkent").as_deref(), Some("11:45 AM"));
        assert_eq!(title("2:45pm to gmt-3").as_deref(), Some("3:45 AM"));
        assert_eq!(title("2:45pm to npt").as_deref(), Some("12:30 PM"));
        assert_eq!(title("noon egst to utc").as_deref(), Some("12:00 PM"));
        assert_eq!(title("noon wgt to utc").as_deref(), Some("2:00 PM"));
        let answer = convert("2:45pm to utc+5:30", &Rates::demo(), &now()).unwrap();
        assert_eq!((answer.title.as_str(), answer.subtitle.as_str()), ("12:15 PM", "2:45 PM here in UTC+5:30 · Mon 9 Nov"));
        // An offset on the time's side too; 9:00 at +5:45 is 03:15 UTC.
        assert_eq!(title("9am utc+0545 to manila").as_deref(), Some("11:15 AM"));
        // A multi-word city from the database, behind DST-free Argentina.
        assert_eq!(title("noon to buenos aires").as_deref(), Some("1:00 AM"));
        assert_eq!(title("2:45pm to utc+15"), None);
        assert_eq!(title("2:45pm to utc+5x"), None);
    }
}
