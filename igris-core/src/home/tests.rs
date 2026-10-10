//! Smart home against a simulated LG webOS TV speaking the SSAP protocol.

use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::*;

#[derive(Default)]
struct Seen {
    requests: Vec<(String, Value)>,
    buttons: Vec<String>,
    /// Everything that arrived on the input channel, as sent.
    input: Vec<String>,
    prompts: usize,
}

struct FakeTv {
    host: String,
    seen: Arc<Mutex<Seen>>,
}

/// `allow`: what the "user" answers on the TV's pairing prompt.
async fn fake_tv(allow: bool) -> FakeTv {
    let main = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let input = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = main.local_addr().unwrap().to_string();
    let input_url = format!("ws://{}/input", input.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Seen::default()));

    let s = seen.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = input.accept().await {
            let s = s.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(sock).await else { return };
                while let Some(Ok(WsMessage::Text(t))) = ws.next().await {
                    let mut seen = s.lock().unwrap();
                    seen.input.push(t.to_string());
                    if let Some(name) = t.lines().find_map(|l| l.strip_prefix("name:")) {
                        seen.buttons.push(name.to_string());
                    }
                }
            });
        }
    });

    let s = seen.clone();
    tokio::spawn(async move {
        while let Ok((sock, _)) = main.accept().await {
            let s = s.clone();
            let input_url = input_url.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(sock).await else { return };
                let mut volume = 12u64;
                while let Some(Ok(WsMessage::Text(t))) = ws.next().await {
                    let m: Value = serde_json::from_str(&t).unwrap();
                    let id = m["id"].clone();
                    let reply = |payload: Value| json!({"type": "response", "id": id, "payload": payload});
                    let out: Vec<Value> = match m["type"].as_str() {
                        Some("register") => match m["payload"]["client-key"].as_str() {
                            Some("KEY123") => vec![json!({"type": "registered", "id": id, "payload": {"client-key": "KEY123"}})],
                            Some(_) => vec![json!({"type": "error", "id": id, "error": "401 insufficient permissions"})],
                            None => {
                                s.lock().unwrap().prompts += 1;
                                let mut v = vec![reply(json!({"pairingType": "PROMPT", "returnValue": true}))];
                                v.push(if allow {
                                    json!({"type": "registered", "id": id, "payload": {"client-key": "KEY123"}})
                                } else {
                                    json!({"type": "error", "id": id, "error": "403 User rejected pairing"})
                                });
                                v
                            }
                        },
                        Some("request") => {
                            let uri = m["uri"].as_str().unwrap_or_default().trim_start_matches("ssap://").to_string();
                            s.lock().unwrap().requests.push((uri.clone(), m["payload"].clone()));
                            let ok = |extra: Value| {
                                let mut p = json!({"returnValue": true});
                                if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
                                    p.extend(e.clone());
                                }
                                reply(p)
                            };
                            vec![match uri.as_str() {
                                "system/getSystemInfo" => ok(json!({"modelName": "OLED55C3"})),
                                "com.webos.service.connectionmanager/getinfo" => ok(json!({"wifiInfo": {"macAddress": "a8:23:fe:01:02:03"}})),
                                "audio/getVolume" => ok(json!({"volumeStatus": {"volume": volume, "muteStatus": false}})),
                                "audio/setVolume" => {
                                    volume = m["payload"]["volume"].as_u64().unwrap_or(volume);
                                    ok(json!({}))
                                }
                                "com.webos.applicationManager/listLaunchPoints" => ok(json!({"launchPoints": [
                                    {"id": "netflix", "title": "Netflix"},
                                    {"id": "youtube.leanback.v4", "title": "YouTube"}
                                ]})),
                                "com.webos.applicationManager/getForegroundAppInfo" => ok(json!({"appId": "netflix"})),
                                "tv/getExternalInputList" => ok(json!({"devices": [
                                    {"id": "HDMI_1", "label": "HDMI 1"},
                                    {"id": "HDMI_2", "label": "PlayStation"}
                                ]})),
                                "com.webos.service.networkinput/getPointerInputSocket" => ok(json!({"socketPath": input_url})),
                                "tv/channelUp" => {
                                    json!({"type": "error", "id": id, "error": "500 Application error", "payload": {"returnValue": false, "errorText": "No signal"}})
                                }
                                _ => ok(json!({})),
                            }]
                        }
                        _ => vec![],
                    };
                    for o in out {
                        if ws.send(WsMessage::Text(o.to_string().into())).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    FakeTv { host, seen }
}

fn home() -> SmartHome {
    SmartHome::new(Arc::new(Database::open_in_memory().unwrap()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_asks_on_the_tv_and_keeps_the_key_private() {
    let tv = fake_tv(true).await;
    let h = home();
    let d = h.pair_lg(&tv.host, None).await.unwrap();
    assert_eq!(d.name, "LG TV (OLED55C3)");
    assert!(d.can_power_on, "the MAC address was learned for Wake-on-LAN");
    assert_eq!(tv.seen.lock().unwrap().prompts, 1);
    // The key is stored, but nothing the UI/AI sees carries it.
    let shown = serde_json::to_string(&h.devices().unwrap()).unwrap();
    assert!(!shown.contains("KEY123") && !shown.contains("a8:23"), "{shown}");
    // Later commands don't prompt again.
    h.tv(&d.id, &TvAction::VolumeUp).await.unwrap();
    assert_eq!(tv.seen.lock().unwrap().prompts, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declined_prompt_adds_nothing() {
    let tv = fake_tv(false).await;
    let h = home();
    let e = h.pair_lg(&tv.host, None).await.unwrap_err();
    assert!(e.to_string().contains("declined"), "{e}");
    assert!(h.devices().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actions_reach_the_tv_and_report_what_happened() {
    let tv = fake_tv(true).await;
    let h = home();
    let id = h.pair_lg(&tv.host, Some("Living room")).await.unwrap().id;

    assert_eq!(h.tv(&id, &TvAction::SetVolume(25)).await.unwrap(), "Living room: volume set to 25");
    assert_eq!(h.tv(&id, &TvAction::Mute(true)).await.unwrap(), "Living room: muted");
    assert_eq!(h.tv(&id, &TvAction::OpenApp("youtube".into())).await.unwrap(), "Living room: opened YouTube");
    assert_eq!(h.tv(&id, &TvAction::SwitchInput("playstation".into())).await.unwrap(), "Living room: switched to PlayStation");
    assert_eq!(h.tv(&id, &TvAction::Button("home".into())).await.unwrap(), "Living room: pressed HOME");
    assert_eq!(h.tv(&id, &TvAction::PowerOff).await.unwrap(), "Living room: turned off");

    {
        let seen = tv.seen.lock().unwrap();
        let uris: Vec<&str> = seen.requests.iter().map(|(u, _)| u.as_str()).collect();
        assert!(uris.contains(&"audio/setVolume") && uris.contains(&"audio/setMute") && uris.contains(&"system/turnOff"));
        assert!(seen.requests.iter().any(|(u, p)| u == "system.launcher/launch" && p["id"] == "youtube.leanback.v4"));
        assert!(seen.requests.iter().any(|(u, p)| u == "tv/switchInput" && p["inputId"] == "HDMI_2"));
    }
    // The button went over the separate input socket.
    for _ in 0..50 {
        if !tv.seen.lock().unwrap().buttons.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(tv.seen.lock().unwrap().buttons, vec!["HOME"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_are_said_plainly() {
    let tv = fake_tv(true).await;
    let h = home();
    let id = h.pair_lg(&tv.host, None).await.unwrap().id;
    let e = h.tv(&id, &TvAction::ChannelUp).await.unwrap_err().to_string();
    assert!(e.contains("couldn't do that") && e.contains("No signal"), "{e}");
    let e = h.tv(&id, &TvAction::OpenApp("Hulu".into())).await.unwrap_err().to_string();
    assert!(e.contains("isn't installed") && e.contains("Netflix"), "{e}");
    let e = h.tv(&id, &TvAction::SwitchInput("HDMI 4".into())).await.unwrap_err().to_string();
    assert!(e.contains("No input called") && e.contains("HDMI 1"), "{e}");
    let e = h.tv(&id, &TvAction::Button("SELF_DESTRUCT".into())).await.unwrap_err().to_string();
    assert!(e.contains("Unknown button"), "{e}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_for_the_remote_screen() {
    let tv = fake_tv(true).await;
    let h = home();
    let id = h.pair_lg(&tv.host, None).await.unwrap().id;
    let s = h.tv_state(&id).await.unwrap();
    assert!(s.on);
    assert_eq!(s.volume, Some(12));
    assert_eq!(s.muted, Some(false));
    assert_eq!(s.app.as_deref(), Some("netflix"));
    assert_eq!(s.apps.len(), 2);
    assert_eq!(s.inputs[1].title, "PlayStation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_tv_reads_as_off_and_commands_say_why() {
    let h = home();
    // Pair against a fake, then point the entry at a port nobody listens on.
    let tv = fake_tv(true).await;
    let id = h.pair_lg(&tv.host, None).await.unwrap().id;
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
    h.db.conn().unwrap().execute("UPDATE home_devices SET host = ?1 WHERE id = ?2", params![dead, id]).unwrap();
    assert!(!h.tv_state(&id).await.unwrap().on);
    let e = h.tv(&id, &TvAction::VolumeUp).await.unwrap_err().to_string();
    assert!(e.contains("Couldn't reach the TV"), "{e}");
}

#[test]
fn picking_a_device_by_name_or_the_only_one() {
    let db = Database::open_in_memory().unwrap();
    let c = db.conn().unwrap();
    assert!(find(&c, None, DeviceKind::LgTv).unwrap_err().to_string().contains("No LG TV is set up"));
    for (id, name) in [("a", "Living room"), ("b", "Bedroom")] {
        c.execute("INSERT INTO home_devices (id, kind, name, host, created_at, updated_at) VALUES (?1, 'lg_tv', ?2, '10.0.0.1', 'x', 'x')", params![id, name])
            .unwrap();
    }
    assert_eq!(find(&c, Some("bedroom"), DeviceKind::LgTv).unwrap().id, "b");
    assert_eq!(find(&c, Some("living"), DeviceKind::LgTv).unwrap().id, "a");
    assert!(find(&c, None, DeviceKind::LgTv).unwrap_err().to_string().contains("Which one?"));
}

#[test]
fn actions_from_tool_input() {
    assert_eq!(TvAction::parse("set_volume", Some(&json!(30))).unwrap(), TvAction::SetVolume(30));
    assert_eq!(TvAction::parse("set_volume", Some(&json!("150"))).unwrap(), TvAction::SetVolume(100));
    assert_eq!(TvAction::parse("open_app", Some(&json!("Netflix"))).unwrap(), TvAction::OpenApp("Netflix".into()));
    assert!(TvAction::parse("open_app", None).is_err());
    assert!(TvAction::parse("launch_missiles", None).is_err());
}

#[test]
fn hosts_are_addresses_not_urls() {
    assert_eq!(validate_host(" 192.168.1.20 ").unwrap(), "192.168.1.20");
    assert!(validate_host("http://evil/").is_err());
    assert!(validate_host("").is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_touchpad_moves_clicks_and_scrolls_over_one_kept_channel() {
    use super::lg::PointerEvent;
    let tv = fake_tv(true).await;
    let h = home();
    let id = h.pair_lg(&tv.host, None).await.unwrap().id;
    h.pointer(&id, &[PointerEvent::Move { dx: 12, dy: -4 }, PointerEvent::Move { dx: 3, dy: 1 }]).await.unwrap();
    h.pointer(&id, &[PointerEvent::Click]).await.unwrap();
    h.pointer(&id, &[PointerEvent::Scroll { dx: 0, dy: 30 }]).await.unwrap();
    let want = ["type:move\ndx:12\ndy:-4\ndown:0\n\n", "type:move\ndx:3\ndy:1\ndown:0\n\n", "type:click\n\n", "type:scroll\ndx:0\ndy:30\n\n"];
    for _ in 0..50 {
        if tv.seen.lock().unwrap().input.len() >= want.len() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    {
        let seen = tv.seen.lock().unwrap();
        assert_eq!(seen.input, want);
        let opened = seen.requests.iter().filter(|(u, _)| u == "com.webos.service.networkinput/getPointerInputSocket").count();
        assert_eq!(opened, 1, "the channel is opened once and kept");
    }
    h.pointer_stop(&id).await;
}
