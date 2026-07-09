pub(crate) fn ensure_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[allow(dead_code)]
pub(crate) fn reqwest_client() -> reqwest::Client {
    ensure_rustls_crypto_provider();
    reqwest_client_builder()
        .build()
        .expect("build platform HTTP client")
}

pub(crate) fn reqwest_client_builder() -> reqwest::ClientBuilder {
    ensure_rustls_crypto_provider();
    codewhale_release::platform_http_client_builder()
}

pub(crate) fn reqwest_blocking_client_builder() -> reqwest::blocking::ClientBuilder {
    ensure_rustls_crypto_provider();
    codewhale_release::platform_blocking_http_client_builder()
}
