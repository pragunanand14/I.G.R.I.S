//! Home-screen widgets (Android): what a widget asked for when it opened
//! IGRIS, and the weather widget's place.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// A place found by name (Open-Meteo's free geocoding, no account).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    results: Vec<FoundPlace>,
}

#[derive(Deserialize)]
struct FoundPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    admin1: Option<String>,
    country: Option<String>,
}

/// Look a place up by name: "Mumbai" → "Mumbai, Maharashtra, India" and its coordinates.
pub async fn find_place(base: &str, query: &str) -> AppResult<Place> {
    let q = query.trim();
    if q.is_empty() || q.chars().count() > 80 {
        return Err(AppError::validation("Type a city or town."));
    }
    let client = igris_core::net::client_builder().timeout(std::time::Duration::from_secs(10)).build().map_err(|e| AppError::internal(e.to_string()))?;
    let res = client
        .get(format!("{base}/v1/search?count=1&language=en&format=json&name={}", percent_encoding::utf8_percent_encode(q, percent_encoding::NON_ALPHANUMERIC)))
        .send()
        .await
        .map_err(|_| AppError::validation("Couldn't look that place up — check the internet connection."))?;
    if !res.status().is_success() {
        return Err(AppError::validation("The place search isn't answering right now. Try again in a bit."));
    }
    let found: Found = res.json().await.map_err(|_| AppError::validation("The place search sent something unexpected."))?;
    let p = found.results.into_iter().next().ok_or_else(|| AppError::validation(format!("Couldn't find \"{q}\". Try the nearest city.")))?;
    let mut name = p.name;
    for part in [p.admin1, p.country].into_iter().flatten() {
        if !part.is_empty() && !name.contains(&part) {
            name = format!("{name}, {part}");
        }
    }
    Ok(Place { name, latitude: p.latitude, longitude: p.longitude })
}

const GEOCODING: &str = "https://geocoding-api.open-meteo.com";

/// What a widget asked for when it opened IGRIS ("listen", "widgets"), once.
#[tauri::command]
pub fn widget_launch_action(#[allow(unused_variables)] app: tauri::AppHandle) -> Option<String> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return app.igris_device().take_launch_action().ok().flatten();
    }
    #[cfg(not(mobile))]
    None
}

/// The weather widget's place, if chosen.
#[tauri::command]
pub async fn widget_weather_place(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<Option<String>> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return tauri::async_runtime::spawn_blocking(move || app.igris_device().weather_place())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()));
    }
    #[cfg(not(mobile))]
    Ok(None)
}

/// Choose the weather widget's place by name; returns the place found.
#[tauri::command]
pub async fn widget_set_weather_place(#[allow(unused_variables)] app: tauri::AppHandle, query: String) -> AppResult<Place> {
    let place = find_place(GEOCODING, &query).await?;
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        let p = place.clone();
        tauri::async_runtime::spawn_blocking(move || app.igris_device().set_weather_place(&p.name, p.latitude, p.longitude))
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()))?;
    }
    #[cfg(not(mobile))]
    return Err(AppError::validation(format!("Found {}, but widgets are on the phone app only.", place.name)));
    #[cfg(mobile)]
    Ok(place)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn serve(body: &'static str) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn a_city_is_found_with_its_region_and_country() {
        let base = serve(r#"{"results":[{"name":"Mumbai","latitude":19.07,"longitude":72.88,"admin1":"Maharashtra","country":"India"}]}"#).await;
        let p = find_place(&base, " mumbai ").await.unwrap();
        assert_eq!(p, Place { name: "Mumbai, Maharashtra, India".into(), latitude: 19.07, longitude: 72.88 });
    }

    #[tokio::test]
    async fn an_unknown_place_says_so() {
        let base = serve(r#"{"generationtime_ms":0.5}"#).await;
        assert!(find_place(&base, "Nowhereville").await.unwrap_err().to_string().contains("Couldn't find"));
        assert!(find_place(&base, "  ").await.is_err());
    }
}
