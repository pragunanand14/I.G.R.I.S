//! `igris-relay [--listen ADDR] [--allow DEVICE_ID,...]`
//!
//! Listens for plain WebSocket connections (default 127.0.0.1:8787). Put it
//! behind a TLS-terminating reverse proxy (see docs/CROSS_DEVICE.md); IGRIS
//! devices only connect to `wss://` relays, or `ws://` on this machine.

use std::collections::HashSet;

fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    let mut listen = std::env::var("IGRIS_RELAY_LISTEN").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let mut allow: HashSet<String> =
        std::env::var("IGRIS_RELAY_ALLOW").unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--listen" => listen = args.next().unwrap_or(listen),
            "--allow" => allow.extend(args.next().unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty())),
            "-h" | "--help" => {
                println!("igris-relay [--listen ADDR] [--allow DEVICE_ID,...]");
                return Ok(());
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    if let Some(bad) = allow.iter().find(|d| !igris_relay::proto::valid_device_id(d)) {
        eprintln!("not a device id: {bad}");
        std::process::exit(2);
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(&listen).await?;
        tracing::info!(event = "RELAY_LISTENING", addr = %listener.local_addr()?, restricted = !allow.is_empty());
        igris_relay::Relay::new(igris_relay::Config { allow, ..Default::default() }).serve(listener).await
    })
}
