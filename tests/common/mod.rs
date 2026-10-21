#![allow(dead_code)]
//! Pembantu uji: JWKS statis, penandatangan JWT RS256 dengan kunci UJI, dan pembangun state.

use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt;
use jsonwebtoken::{encode, jwk::JwkSet, Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use titipo_backend_service::{
    auth::jwks::{FetchFuture, JwksFetcher},
    config::Config,
    AppState,
};
use tower::ServiceExt;

const PEM: &[u8] = include_bytes!("../fixtures/uji_rsa.pem");
const N: &str = include_str!("../fixtures/uji_jwks_n.txt");

pub fn jwks_uji() -> JwkSet {
    serde_json::from_value(json!({"keys": [{
        "kty": "RSA", "kid": "uji-1", "alg": "RS256", "use": "sig", "n": N.trim(), "e": "AQAB"
    }]}))
    .unwrap()
}

pub struct FetcherStatis {
    pub set: JwkSet,
    pub panggilan: Arc<AtomicUsize>,
}

impl JwksFetcher for FetcherStatis {
    fn ambil(&self) -> FetchFuture<'_> {
        self.panggilan.fetch_add(1, Ordering::SeqCst);
        let set = self.set.clone();
        Box::pin(async move { Ok(set) })
    }
}

pub fn fetcher_uji() -> (Arc<dyn JwksFetcher>, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    (
        Arc::new(FetcherStatis {
            set: jwks_uji(),
            panggilan: n.clone(),
        }),
        n,
    )
}

pub fn state_dengan(db: PgPool) -> AppState {
    let (f, _) = fetcher_uji();
    AppState::dengan_fetcher(db, Config::uji(), f)
}

/// State dengan pool malas (tanpa koneksi nyata) untuk uji yang tidak menyentuh DB.
pub fn state_tanpa_db() -> AppState {
    let db = PgPool::connect_lazy("postgres://tidak-dipakai@localhost/tidak_ada").unwrap();
    state_dengan(db)
}

pub fn unix_sekarang() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Tanda tangani JWT UJI. `ubah` memodifikasi klaim sebelum ditandatangani.
pub fn token(ubah: impl FnOnce(&mut Value)) -> String {
    let sekarang = unix_sekarang();
    let mut klaim = json!({
        "sub": "RT-001", "role": "household", "aud": "titipo",
        "iss": "https://auth.lumbung.skuy", "iat": sekarang, "exp": sekarang + 900
    });
    ubah(&mut klaim);
    let mut h = Header::new(Algorithm::RS256);
    h.kid = Some("uji-1".into());
    encode(&h, &klaim, &EncodingKey::from_rsa_pem(PEM).unwrap()).unwrap()
}

pub async fn panggil(app: &Router, req: Request<Body>) -> (u16, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

pub fn get_bearer(path: &str, token: &str) -> Request<Body> {
    Request::get(path)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}
