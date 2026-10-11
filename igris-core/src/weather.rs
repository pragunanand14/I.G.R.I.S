//! Weather from Open-Meteo (free, no account): find a place by name, and the
//! current weather plus today's and tomorrow's forecast for it. Only the
//! place's coordinates are sent — never the device's location. The user's
//! "home" place (for "what's the weather?" and the briefing) is a setting.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

pub const GEOCODING: &str = "https://geocoding-api.open-meteo.com";
pub const FORECAST: &str = "https://api.open-meteo.com";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

fn client() -> AppResult<reqwest::Client> {
    crate::net::client_builder().timeout(std::time::Duration::from_secs(12)).build().map_err(|e| AppError::internal(e.to_string()))
}

fn url(base: &str, path: &str, query: &[(&str, &str)]) -> String {
    let mut u = reqwest::Url::parse(&format!("{base}{path}")).expect("static url");
    u.query_pairs_mut().extend_pairs(query.iter().copied());
    u.to_string()
}

async fn get(u: &str) -> AppResult<Value> {
    let res = client()?.get(u).send().await.map_err(|_| AppError::validation("Couldn't reach the weather service — check the internet connection."))?;
    if !res.status().is_success() {
        return Err(AppError::validation("The weather service isn't answering right now. Try again in a bit."));
    }
    res.json().await.map_err(|_| AppError::validation("The weather service sent something unexpected."))
}

/// "Mumbai" → "Mumbai, Maharashtra, India" and its coordinates.
pub async fn find_place(base: &str, query: &str) -> AppResult<Place> {
    let q = query.trim();
    if q.is_empty() || q.chars().count() > 80 {
        return Err(AppError::validation("Type a city or town."));
    }
    let v = get(&url(base, "/v1/search", &[("name", q), ("count", "1"), ("language", "en"), ("format", "json")])).await?;
    let p = v["results"].get(0).ok_or_else(|| AppError::validation(format!("Couldn't find \"{q}\". Try the nearest city.")))?;
    let mut name = p["name"].as_str().unwrap_or(q).to_string();
    for part in [p["admin1"].as_str(), p["country"].as_str()].into_iter().flatten() {
        if !part.is_empty() && !name.contains(part) {
            name = format!("{name}, {part}");
        }
    }
    Ok(Place { name, latitude: p["latitude"].as_f64().unwrap_or_default(), longitude: p["longitude"].as_f64().unwrap_or_default() })
}

/// The weather codes Open-Meteo reports (WMO), in plain words.
pub fn describe_code(code: i64) -> &'static str {
    match code {
        0 => "clear",
        1 | 2 => "partly cloudy",
        3 => "cloudy",
        45 | 48 => "foggy",
        51 | 53 | 55 | 56 | 57 => "drizzle",
        61 | 63 | 65 | 66 | 67 => "rain",
        71 | 73 | 75 | 77 | 85 | 86 => "snow",
        80..=82 => "rain showers",
        95 | 96 | 99 => "thunderstorms",
        _ => "mixed",
    }
}

/// Now, today and tomorrow, in one short paragraph (°C).
pub async fn report(base: &str, place: &Place) -> AppResult<String> {
    let (lat, lon) = (place.latitude.to_string(), place.longitude.to_string());
    let v = get(&url(
        base,
        "/v1/forecast",
        &[
            ("latitude", &lat),
            ("longitude", &lon),
            ("current", "temperature_2m,apparent_temperature,weather_code,relative_humidity_2m,wind_speed_10m"),
            ("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max"),
            ("timezone", "auto"),
            ("forecast_days", "2"),
        ],
    ))
    .await?;
    let c = &v["current"];
    let d = &v["daily"];
    let n = |x: &Value| x.as_f64().map(|f| f.round() as i64);
    let day = |i: usize, label: &str| -> Option<String> {
        Some(format!(
            "{label}: {}, {}°–{}°C, {}% chance of rain",
            describe_code(d["weather_code"][i].as_i64()?),
            n(&d["temperature_2m_min"][i])?,
            n(&d["temperature_2m_max"][i])?,
            n(&d["precipitation_probability_max"][i]).unwrap_or(0)
        ))
    };
    let now = format!(
        "Now in {}: {}°C (feels like {}°C), {}, humidity {}%, wind {} km/h.",
        place.name,
        n(&c["temperature_2m"]).unwrap_or_default(),
        n(&c["apparent_temperature"]).unwrap_or_default(),
        describe_code(c["weather_code"].as_i64().unwrap_or(-1)),
        n(&c["relative_humidity_2m"]).unwrap_or_default(),
        n(&c["wind_speed_10m"]).unwrap_or_default()
    );
    Ok([Some(now), day(0, "Today"), day(1, "Tomorrow")].into_iter().flatten().collect::<Vec<_>>().join(" "))
}

pub fn home(conn: &Connection) -> AppResult<Option<Place>> {
    crate::settings::get_extra(conn, "homePlace")
}

pub fn set_home(conn: &Connection, place: &Place) -> AppResult<()> {
    crate::settings::set_extra(conn, "homePlace", place)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A fake Open-Meteo answering both the geocoding and the forecast paths.
    pub(crate) async fn fake() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = if req.contains("/v1/search") && req.contains("name=Nowhere") {
                    "{}".to_string()
                } else if req.contains("/v1/search") {
                    r#"{"results":[{"name":"Mumbai","latitude":19.07,"longitude":72.88,"admin1":"Maharashtra","country":"India"}]}"#.to_string()
                } else {
                    r#"{"current":{"temperature_2m":31.4,"apparent_temperature":36.2,"weather_code":2,"relative_humidity_2m":70,"wind_speed_10m":12},
                        "daily":{"weather_code":[61,3],"temperature_2m_min":[26.1,25.9],"temperature_2m_max":[32.2,31.0],"precipitation_probability_max":[80,20]}}"#
                        .to_string()
                };
                let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        base
    }

    #[tokio::test]
    async fn finds_a_place_and_reports_its_weather() {
        let base = fake().await;
        let p = find_place(&base, " mumbai ").await.unwrap();
        assert_eq!(p.name, "Mumbai, Maharashtra, India");
        assert!(find_place(&base, "Nowhere").await.unwrap_err().to_string().contains("Couldn't find"));
        let r = report(&base, &p).await.unwrap();
        assert!(r.starts_with("Now in Mumbai, Maharashtra, India: 31°C (feels like 36°C), partly cloudy"), "{r}");
        assert!(r.contains("Today: rain, 26°–32°C, 80% chance of rain") && r.contains("Tomorrow: cloudy"), "{r}");
    }

    #[test]
    fn the_home_place_is_a_setting() {
        let db = crate::db::Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(home(&c).unwrap().is_none());
        set_home(&c, &Place { name: "Pune".into(), latitude: 18.5, longitude: 73.8 }).unwrap();
        assert_eq!(home(&c).unwrap().unwrap().name, "Pune");
        // It doesn't disturb the normal settings.
        assert!(crate::settings::load(&c).is_ok());
    }
}
