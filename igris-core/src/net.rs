//! HTTP clients.
//!
//! Every outbound HTTP client in IGRIS starts from [`client_builder`]. On
//! Android, the platform certificate verifier needs JNI setup that a Tauri app
//! doesn't have, so certificates are checked against the bundled Mozilla root
//! store instead (user-installed CAs are not trusted there). Elsewhere the
//! operating system's verifier is used, as before.

/// A `reqwest` client builder with this platform's TLS trust configured.
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().filter_map(|c| reqwest::Certificate::from_der(c).ok()));
    builder
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_a_client() {
        assert!(super::client_builder().build().is_ok());
    }
}
