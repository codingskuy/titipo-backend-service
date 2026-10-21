//! Satu `AppError` untuk seluruh API; bentuk badan mengikuti skema `AppError` di openapi.yaml.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct AppError {
    pub status: StatusCode,
    pub kode: &'static str,
    pub pesan: String,
    pub detail: Value,
}

impl AppError {
    pub fn new(status: StatusCode, kode: &'static str, pesan: impl Into<String>) -> Self {
        Self {
            status,
            kode,
            pesan: pesan.into(),
            detail: json!({}),
        }
    }

    pub fn dengan_detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }

    pub fn permintaan(kode: &'static str, pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, kode, pesan)
    }
    pub fn tidak_terotorisasi(kode: &'static str, pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, kode, pesan)
    }
    pub fn dilarang(kode: &'static str, pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, kode, pesan)
    }
    pub fn tidak_ditemukan(kode: &'static str, pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, kode, pesan)
    }
    pub fn konflik(kode: &'static str, pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, kode, pesan)
    }
    pub fn validasi(kode: &'static str, pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, kode, pesan)
    }
    pub fn internal(pesan: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "GALAT_INTERNAL", pesan)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "galat basis data");
        // Jangan bocorkan detail internal ke klien.
        AppError::internal("Galat internal")
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let badan = json!({ "kode": self.kode, "pesan": self.pesan, "detail": self.detail });
        (self.status, Json(badan)).into_response()
    }
}
