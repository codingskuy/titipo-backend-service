use crate::{auth::AuthUser, pesanan::handlers, state::AppState};
use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::time::Duration;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct Contoh {
    pesan: &'static str,
    status_fasad: &'static str,
    rujukan_auth: &'static str,
    sub: String,
}

async fn healthz() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "titipo-backend-service",
    })
}

async fn contoh(user: AuthUser) -> Json<Contoh> {
    Json(Contoh {
        pesan: "Contoh GET TitipO — STATUS stub, belum data produksi.",
        status_fasad: "stub — facade ke lumbung",
        rujukan_auth: "docs/auth.md (JWT aud=titipo + service key titipo->lumbung)",
        sub: user.sub,
    })
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/contoh", get(contoh))
        .route("/pesanan", post(handlers::buat))
        .route("/pesanan/{kode}/bayar", post(handlers::bayar))
        .route("/vendor/pesanan/{kode}/konfirmasi", post(handlers::konfirmasi_vendor))
        .route("/admin/pesanan/{kode}/alihkan", post(handlers::alihkan))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
