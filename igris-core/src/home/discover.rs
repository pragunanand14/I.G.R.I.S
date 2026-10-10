//! Finding devices on the local network. LG webOS TVs answer an SSDP search
//! for their "second screen" service; the answer's LOCATION points at a small
//! XML description with the TV's name.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use serde::Serialize;

use super::DeviceKind;

const SSDP: &str = "239.255.255.250:1900";
const LG_SERVICE: &str = "urn:lge-com:service:webos-second-screen:1";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub kind: DeviceKind,
    pub host: String,
    pub name: String,
}

fn search_message() -> String {
    format!("M-SEARCH * HTTP/1.1\r\nHOST: {SSDP}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {LG_SERVICE}\r\n\r\n")
}

/// The LOCATION header of an SSDP answer for LG's service (None for anything else).
fn lg_location(answer: &str) -> Option<String> {
    let mut lg = false;
    let mut location = None;
    for line in answer.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        if (k == "st" || k == "nt") && v.eq_ignore_ascii_case(LG_SERVICE) {
            lg = true;
        }
        if k == "location" {
            location = Some(v.to_string());
        }
    }
    if lg {
        location.or(Some(String::new()))
    } else {
        None
    }
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let start = xml.find(&format!("<{name}>"))? + name.len() + 2;
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    let v = xml[start..end].trim();
    (!v.is_empty()).then(|| v.replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\""))
}

/// Look for LG TVs for `wait`. `target` overrides the SSDP address (tests).
pub async fn find(wait: Duration, target: Option<SocketAddr>) -> Vec<Found> {
    let Ok(sock) = tokio::net::UdpSocket::bind("0.0.0.0:0").await else { return Vec::new() };
    let to: SocketAddr = target.unwrap_or_else(|| SSDP.parse().expect("valid address"));
    let msg = search_message();
    // UDP can drop a packet; ask twice.
    for _ in 0..2 {
        let _ = sock.send_to(msg.as_bytes(), to).await;
    }
    let mut seen: BTreeMap<String, Option<String>> = BTreeMap::new();
    let deadline = tokio::time::Instant::now() + wait;
    let mut buf = [0u8; 2048];
    while let Ok(Ok((n, from))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await {
        if let Some(loc) = lg_location(&String::from_utf8_lossy(&buf[..n])) {
            seen.entry(from.ip().to_string()).or_insert((!loc.is_empty()).then_some(loc));
        }
    }
    let http = crate::net::client_builder().no_proxy().timeout(Duration::from_secs(3)).build().ok();
    let mut out = Vec::new();
    for (host, location) in seen {
        let mut name = None;
        if let (Some(http), Some(loc)) = (&http, location) {
            if let Ok(resp) = http.get(&loc).send().await {
                if let Ok(xml) = resp.text().await {
                    name = tag(&xml, "friendlyName").or_else(|| tag(&xml, "modelName"));
                }
            }
        }
        out.push(Found { kind: DeviceKind::LgTv, host, name: name.unwrap_or_else(|| "LG TV".into()) });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lg_answers_count() {
        let lg = "HTTP/1.1 200 OK\r\nST: urn:lge-com:service:webos-second-screen:1\r\nLOCATION: http://192.168.1.20:1914/\r\n\r\n";
        assert_eq!(lg_location(lg).as_deref(), Some("http://192.168.1.20:1914/"));
        let other = "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nLOCATION: http://x/\r\n\r\n";
        assert_eq!(lg_location(other), None);
    }

    #[test]
    fn reads_the_name_from_the_description() {
        let xml = "<root><device><friendlyName>Living room TV</friendlyName><modelName>OLED55C3</modelName></device></root>";
        assert_eq!(tag(xml, "friendlyName").as_deref(), Some("Living room TV"));
        assert_eq!(tag("<x></x>", "friendlyName"), None);
    }

    #[tokio::test]
    async fn finds_a_tv_that_answers() {
        // A fake TV: answers the search and serves its description.
        let tv = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let tv_addr = tv.local_addr().unwrap();
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = http.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let (_, from) = tv.recv_from(&mut buf).await.unwrap();
            let answer = format!("HTTP/1.1 200 OK\r\nST: {LG_SERVICE}\r\nLOCATION: http://127.0.0.1:{http_port}/desc.xml\r\n\r\n");
            tv.send_to(answer.as_bytes(), from).await.unwrap();
        });
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut s, _) = http.accept().await.unwrap();
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b).await;
            let body = "<root><device><friendlyName>Bedroom TV</friendlyName></device></root>";
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
        });
        let found = find(Duration::from_millis(800), Some(tv_addr)).await;
        assert_eq!(found, vec![Found { kind: DeviceKind::LgTv, host: "127.0.0.1".into(), name: "Bedroom TV".into() }]);
    }
}
