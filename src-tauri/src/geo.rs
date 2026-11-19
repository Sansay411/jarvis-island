//! Where the user is and what the weather is there.
//!
//! Location: the city of the PC's public IP (ipwho.is, no key) — city-level,
//! which is what weather and «что рядом» need. Cached for an hour.
//! Weather: Open-Meteo (no key): current conditions, the next hours and
//! today/tomorrow. A named place is found with Open-Meteo geocoding.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub city: String,
    pub region: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
    /// Found from the PC's internet address (approximate), not named by the user.
    pub approximate: bool,
}

impl Place {
    pub fn label(&self) -> String {
        if self.city.is_empty() { self.country.clone() } else { self.city.clone() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourPoint {
    /// «18:00»
    pub time: String,
    pub temp: f64,
    pub kind: &'static str,
    pub precip: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub max: f64,
    pub min: f64,
    pub kind: &'static str,
    pub text: &'static str,
    pub precip: u32,
    /// Strongest wind of the day, m/s.
    pub wind: f64,
    /// «08:12», empty when the service did not say.
    pub sunrise: String,
    pub sunset: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Weather {
    pub place: Place,
    pub temp: f64,
    pub feels: f64,
    /// clear, partly, cloud, fog, rain, snow, storm
    pub kind: &'static str,
    pub text: &'static str,
    pub is_day: bool,
    pub wind: f64,
    pub humidity: u32,
    pub hours: Vec<HourPoint>,
    pub today: DayPoint,
    pub tomorrow: Option<DayPoint>,
    /// Tomorrow from morning to night, every three hours (06:00 … 21:00).
    pub tomorrow_hours: Vec<HourPoint>,
}

fn http() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .user_agent("Jarvis-desktop")
        .build()
        .map_err(|e| AppError::internal(e.to_string()))
}

fn get_json(url: &str, query: &[(&str, String)]) -> AppResult<Value> {
    let url = reqwest::Url::parse_with_params(url, query).map_err(|e| AppError::internal(e.to_string()))?;
    http()?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::platform(format!("Сервис погоды недоступен: {e}")))?
        .json()
        .map_err(|e| AppError::platform(format!("Сервис погоды: {e}")))
}

static HERE: Mutex<Option<(Instant, Place)>> = Mutex::new(None);

/// The user's city by the PC's internet address (cached for an hour).
pub fn here() -> AppResult<Place> {
    if let Some((at, p)) = HERE.lock().as_ref() {
        if at.elapsed() < Duration::from_secs(3600) {
            return Ok(p.clone());
        }
    }
    let v = get_json("https://ipwho.is/", &[("lang", "ru".into())])?;
    if v["success"].as_bool() != Some(true) {
        return Err(AppError::platform("Не удалось определить город по сети"));
    }
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    let p = Place {
        city: s("city"),
        region: s("region"),
        country: s("country"),
        lat: v["latitude"].as_f64().unwrap_or(0.0),
        lon: v["longitude"].as_f64().unwrap_or(0.0),
        approximate: true,
    };
    *HERE.lock() = Some((Instant::now(), p.clone()));
    Ok(p)
}

/// Already known location, without waiting for the network.
pub fn here_cached() -> Option<Place> {
    HERE.lock().as_ref().map(|(_, p)| p.clone())
}

/// A place the user named («погода в Астане»).
pub fn find(name: &str) -> AppResult<Place> {
    let v = get_json(
        "https://geocoding-api.open-meteo.com/v1/search",
        &[("name", name.trim().to_string()), ("count", "1".into()), ("language", "ru".into())],
    )?;
    let r = v["results"].get(0).ok_or_else(|| AppError::not_found(format!("Не нашёл место «{}»", name.trim())))?;
    let s = |k: &str| r[k].as_str().unwrap_or_default().to_string();
    Ok(Place {
        city: s("name"),
        region: s("admin1"),
        country: s("country"),
        lat: r["latitude"].as_f64().unwrap_or(0.0),
        lon: r["longitude"].as_f64().unwrap_or(0.0),
        approximate: false,
    })
}

/// WMO weather code → (kind, Russian text).
pub fn describe(code: i64) -> (&'static str, &'static str) {
    match code {
        0 => ("clear", "Ясно"),
        1 => ("clear", "Почти ясно"),
        2 => ("partly", "Переменная облачность"),
        3 => ("cloud", "Пасмурно"),
        45 | 48 => ("fog", "Туман"),
        51 | 53 | 55 | 56 | 57 => ("rain", "Морось"),
        61 | 63 | 66 => ("rain", "Дождь"),
        65 | 67 => ("rain", "Сильный дождь"),
        71 | 73 | 77 => ("snow", "Снег"),
        75 => ("snow", "Сильный снег"),
        80 | 81 => ("rain", "Ливень"),
        82 => ("rain", "Сильный ливень"),
        85 | 86 => ("snow", "Снегопад"),
        95 | 96 | 99 => ("storm", "Гроза"),
        _ => ("cloud", "Облачно"),
    }
}

static CACHE: Mutex<Option<(Instant, Weather)>> = Mutex::new(None);

/// Weather for `place`, or for where the user is.
pub fn weather(place: Option<&str>) -> AppResult<Weather> {
    let place = match place.map(str::trim).filter(|p| !p.is_empty()) {
        Some(name) => find(name)?,
        None => here()?,
    };
    if let Some((at, w)) = CACHE.lock().as_ref() {
        if at.elapsed() < Duration::from_secs(600) && (w.place.lat - place.lat).abs() < 0.05 && (w.place.lon - place.lon).abs() < 0.05 {
            return Ok(w.clone());
        }
    }
    let v = get_json(
        "https://api.open-meteo.com/v1/forecast",
        &[
            ("latitude", format!("{:.4}", place.lat)),
            ("longitude", format!("{:.4}", place.lon)),
            ("current", "temperature_2m,apparent_temperature,weather_code,wind_speed_10m,relative_humidity_2m,is_day".into()),
            ("hourly", "temperature_2m,weather_code,precipitation_probability".into()),
            ("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,wind_speed_10m_max,sunrise,sunset".into()),
            ("timezone", "auto".into()),
            ("forecast_days", "2".into()),
            ("wind_speed_unit", "ms".into()),
        ],
    )?;
    let w = parse(place, &v)?;
    *CACHE.lock() = Some((Instant::now(), w.clone()));
    Ok(w)
}

fn parse(place: Place, v: &Value) -> AppResult<Weather> {
    let c = &v["current"];
    let num = |x: &Value| x.as_f64().unwrap_or(0.0);
    let (kind, text) = describe(c["weather_code"].as_i64().unwrap_or(3));
    let now = c["time"].as_str().unwrap_or_default().to_string();
    let h = &v["hourly"];
    let times = h["time"].as_array().cloned().unwrap_or_default();
    // The next six points, every two hours, from the current hour on.
    let start = times.iter().position(|t| t.as_str().unwrap_or_default() >= &now[..now.len().min(13)]).unwrap_or(0);
    let point = |i: usize| HourPoint {
        time: times[i].as_str().unwrap_or_default().get(11..16).unwrap_or_default().to_string(),
        temp: num(&h["temperature_2m"][i]),
        kind: describe(h["weather_code"][i].as_i64().unwrap_or(3)).0,
        precip: h["precipitation_probability"][i].as_u64().unwrap_or(0) as u32,
    };
    let hours = (0..6).map(|i| start + 1 + i * 2).filter(|&i| i < times.len()).map(point).collect();
    // Tomorrow's date is the second daily entry; its hours 06…21 every three.
    let tomorrow_date = v["daily"]["time"][1].as_str().unwrap_or_default().to_string();
    let tomorrow_hours = if tomorrow_date.is_empty() {
        Vec::new()
    } else {
        times
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let t = t.as_str().unwrap_or_default();
                t.starts_with(&tomorrow_date) && matches!(t.get(11..13), Some("06" | "09" | "12" | "15" | "18" | "21"))
            })
            .map(|(i, _)| point(i))
            .collect()
    };
    let d = &v["daily"];
    let day = |i: usize| {
        d["temperature_2m_max"].get(i)?.as_f64().map(|max| {
            let (kind, text) = describe(d["weather_code"][i].as_i64().unwrap_or(3));
            DayPoint {
                max,
                min: num(&d["temperature_2m_min"][i]),
                kind,
                text,
                precip: d["precipitation_probability_max"][i].as_u64().unwrap_or(0) as u32,
                wind: num(&d["wind_speed_10m_max"][i]),
                sunrise: d["sunrise"][i].as_str().and_then(|t| t.get(11..16)).unwrap_or_default().to_string(),
                sunset: d["sunset"][i].as_str().and_then(|t| t.get(11..16)).unwrap_or_default().to_string(),
            }
        })
    };
    Ok(Weather {
        place,
        temp: num(&c["temperature_2m"]),
        feels: num(&c["apparent_temperature"]),
        kind,
        text,
        is_day: c["is_day"].as_i64() == Some(1),
        wind: num(&c["wind_speed_10m"]),
        humidity: c["relative_humidity_2m"].as_u64().unwrap_or(0) as u32,
        hours,
        today: day(0).ok_or_else(|| AppError::platform("Сервис погоды вернул пустой прогноз"))?,
        tomorrow: day(1),
        tomorrow_hours,
    })
}

/// «−7°»
pub fn deg(t: f64) -> String {
    let r = t.round() as i64;
    if r < 0 { format!("−{}°", -r) } else { format!("{r}°") }
}

/// One line for the model and the user.
pub fn summary(w: &Weather) -> String {
    let mut s = format!(
        "{}{}: {}, {}, ощущается как {}. Ветер {:.0} м/с, влажность {}%. Сегодня от {} до {}",
        w.place.label(),
        if w.place.approximate { " (определено по сети)" } else { "" },
        deg(w.temp),
        w.text.to_lowercase(),
        deg(w.feels),
        w.wind,
        w.humidity,
        deg(w.today.min),
        deg(w.today.max),
    );
    if w.today.precip >= 30 {
        s.push_str(&format!(", осадки {}%", w.today.precip));
    }
    if let Some(t) = &w.tomorrow {
        s.push_str(&format!(". Завтра {}, от {} до {}", t.text.to_lowercase(), deg(t.min), deg(t.max)));
        if t.precip >= 30 {
            s.push_str(&format!(", осадки {}%", t.precip));
        }
    }
    s.push('.');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_meteo_answer_is_read() {
        let v: Value = serde_json::from_str(r#"{
            "current":{"time":"2026-11-18T18:15","temperature_2m":-1.3,"apparent_temperature":-5.2,"weather_code":71,"wind_speed_10m":2.2,"relative_humidity_2m":81,"is_day":0},
            "hourly":{"time":["2026-11-18T17:00","2026-11-18T18:00","2026-11-18T19:00","2026-11-18T20:00","2026-11-18T21:00"],
                      "temperature_2m":[-1,-1.3,-2,-2.6,-3],"weather_code":[3,71,71,3,0],"precipitation_probability":[10,60,70,20,0]},
            "daily":{"time":["2026-11-18","2026-11-19"],"weather_code":[71,3],"temperature_2m_max":[0.4,-2],"temperature_2m_min":[-6,-9.6],"precipitation_probability_max":[70,10]}}"#).unwrap();
        let place = Place { city: "Караганда".into(), region: String::new(), country: "Казахстан".into(), lat: 49.8, lon: 73.1, approximate: true };
        let w = parse(place, &v).unwrap();
        assert_eq!((w.kind, w.text), ("snow", "Снег"));
        assert_eq!(w.hours[0].time, "19:00");
        assert_eq!(w.hours.len(), 2);
        assert!(w.tomorrow_hours.is_empty(), "no hourly points for tomorrow in this sample");
        assert_eq!(deg(w.temp), "−1°");
        let s = summary(&w);
        assert!(s.starts_with("Караганда (определено по сети): −1°, снег, ощущается как −5°."), "{s}");
        assert!(s.contains("Завтра пасмурно, от −10° до −2°."), "{s}");
    }

    #[test]
    fn tomorrow_is_read_every_three_hours() {
        let v: Value = serde_json::from_str(r#"{
            "current":{"time":"2026-11-18T22:00","temperature_2m":-3,"apparent_temperature":-6,"weather_code":0,"wind_speed_10m":2,"relative_humidity_2m":70,"is_day":0},
            "hourly":{"time":["2026-11-18T22:00","2026-11-18T23:00","2026-11-19T06:00","2026-11-19T07:00","2026-11-19T09:00","2026-11-19T12:00","2026-11-19T21:00"],
                      "temperature_2m":[-3,-4,-5,-5,-2,3,0],"weather_code":[0,0,3,3,3,61,0],"precipitation_probability":[0,0,10,10,20,60,0]},
            "daily":{"time":["2026-11-18","2026-11-19"],"weather_code":[0,3],"temperature_2m_max":[3,4],"temperature_2m_min":[-6,-5],"precipitation_probability_max":[0,60],
                     "wind_speed_10m_max":[4.1,7.5],"sunrise":["2026-11-18T08:01","2026-11-19T08:03"],"sunset":["2026-11-18T16:40","2026-11-19T16:39"]}}"#).unwrap();
        let place = Place { city: "Караганда".into(), region: String::new(), country: "Казахстан".into(), lat: 49.8, lon: 73.1, approximate: true };
        let w = parse(place, &v).unwrap();
        let t: Vec<_> = w.tomorrow_hours.iter().map(|h| (h.time.as_str(), h.kind)).collect();
        assert_eq!(t, [("06:00", "cloud"), ("09:00", "cloud"), ("12:00", "rain"), ("21:00", "clear")]);
        let d = w.tomorrow.unwrap();
        assert_eq!((d.wind, d.sunrise.as_str(), d.sunset.as_str()), (7.5, "08:03", "16:39"));
    }

    /// Real network: `cargo test --lib live_weather -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_weather() {
        let w = weather(None).expect("weather here");
        println!("{}", summary(&w));
        println!("hours: {:?}", w.hours.iter().map(|h| format!("{} {} {}", h.time, deg(h.temp), h.kind)).collect::<Vec<_>>());
        let a = weather(Some("Астана")).expect("weather Astana");
        println!("{}", summary(&a));
    }
}
