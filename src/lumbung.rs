//! Klien keluar ke Lumbung (facade). Service key hanya dipasang di server, tidak pernah di klien.

use crate::auth::service_key::HEADER_SERVICE_KEY;
use reqwest::{Client, RequestBuilder};

#[derive(Clone)]
pub struct LumbungClient {
    base_url: String,
    service_key: String,
    http: Client,
}

impl LumbungClient {
    pub fn baru(base_url: impl Into<String>, service_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            service_key: service_key.into(),
            http: Client::new(),
        }
    }

    /// GET ke Lumbung dengan header `X-Titipo-Service-Key`.
    pub fn get(&self, path: &str) -> RequestBuilder {
        self.http
            .get(format!("{}/{}", self.base_url, path.trim_start_matches('/')))
            .header(HEADER_SERVICE_KEY, &self.service_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_memasang_header_service_key() {
        let c = LumbungClient::baru("http://lumbung.test/", "rahasia-uji");
        let req = c.get("/katalog/V-1").build().unwrap();
        assert_eq!(req.url().as_str(), "http://lumbung.test/katalog/V-1");
        assert_eq!(req.headers().get("X-Titipo-Service-Key").unwrap(), "rahasia-uji");
    }
}
