//! Autentikasi: JWT beraudiens `titipo` (JWKS Lumbung) + service key server-ke-server.

pub mod jwks;
pub mod service_key;

use crate::{error::AppError, state::AppState};
use axum::{extract::FromRequestParts, http::request::Parts};
use jsonwebtoken::{decode, decode_header, errors::ErrorKind, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Household,
    Vendor,
    Admin,
    /// Peran baca-saja web admin (openapi: `operasi` boleh baca).
    Operasi,
}

#[derive(Debug, Clone, Deserialize)]
struct Klaim {
    sub: String,
    role: Role,
    /// Id lapak vendor. ASUMSI: klaim opsional; bila kosong, `sub` dipakai sebagai id vendor.
    #[serde(default)]
    vendor_id: Option<String>,
}

/// Cakupan akses data: dasar penegakan "rumah tangga hanya miliknya, vendor hanya lapaknya".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skup {
    RumahTangga(String),
    Vendor(String),
    Semua,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub sub: String,
    pub role: Role,
    pub vendor_id: Option<String>,
}

impl AuthUser {
    pub fn skup(&self) -> Skup {
        match self.role {
            Role::Household => Skup::RumahTangga(self.sub.clone()),
            Role::Vendor => Skup::Vendor(self.vendor_id.clone().unwrap_or_else(|| self.sub.clone())),
            Role::Admin | Role::Operasi => Skup::Semua,
        }
    }

    /// Tolak 403 `PERAN_DITOLAK` bila peran tidak termasuk `diizinkan`.
    pub fn wajib_peran(&self, diizinkan: &[Role]) -> Result<(), AppError> {
        if diizinkan.contains(&self.role) {
            Ok(())
        } else {
            Err(AppError::dilarang(
                "PERAN_DITOLAK",
                "Peran tidak berhak mengakses sumber daya ini",
            ))
        }
    }
}

fn token_bearer(parts: &Parts) -> Result<&str, AppError> {
    let h = parts
        .headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::tidak_terotorisasi("TOKEN_TIDAK_ADA", "Authorization: Bearer wajib"))?;
    h.strip_prefix("Bearer ")
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::tidak_terotorisasi("TOKEN_TIDAK_ADA", "Format Authorization harus Bearer <token>"))
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = token_bearer(parts)?;
        let tidak_valid = || AppError::tidak_terotorisasi("TOKEN_TIDAK_VALID", "Token tidak valid");

        let header = decode_header(token).map_err(|_| tidak_valid())?;
        if header.alg != Algorithm::RS256 {
            return Err(tidak_valid());
        }
        let jwk = state
            .jwks
            .kunci(header.kid.as_deref())
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "gagal mengambil JWKS Lumbung");
                AppError::new(
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "JWKS_TIDAK_TERSEDIA",
                    "Verifikasi token tidak tersedia",
                )
            })?
            .ok_or_else(tidak_valid)?;
        let kunci = DecodingKey::from_jwk(&jwk).map_err(|_| tidak_valid())?;

        let mut v = Validation::new(Algorithm::RS256);
        v.leeway = state.config.toleransi_jam;
        v.set_audience(&[state.config.audiens.as_str()]);
        v.set_issuer(&[state.config.issuer.as_str()]);
        v.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);

        let data = decode::<Klaim>(token, &kunci, &v).map_err(|e| match e.kind() {
            ErrorKind::ExpiredSignature => {
                AppError::tidak_terotorisasi("TOKEN_KEDALUWARSA", "Token kedaluwarsa; login ulang")
            }
            ErrorKind::InvalidAudience => AppError::dilarang("AUDIENS_SALAH", "Audiens token bukan titipo"),
            _ => tidak_valid(),
        })?;
        Ok(AuthUser {
            sub: data.claims.sub,
            role: data.claims.role,
            vendor_id: data.claims.vendor_id,
        })
    }
}

/// Extractor untuk rute server-ke-server: `X-Titipo-Service-Key`.
#[derive(Debug, Clone, Copy)]
pub struct LayananSah;

impl FromRequestParts<AppState> for LayananSah {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let nilai = parts
            .headers
            .get(service_key::HEADER_SERVICE_KEY)
            .and_then(|v| v.to_str().ok());
        state.keyring.verifikasi(nilai, chrono::Utc::now())?;
        Ok(LayananSah)
    }
}

/// Extractor sinyal Pedaree: header `X-Service-Key` (kunci least-privilege milik Pedaree).
#[derive(Debug, Clone, Copy)]
pub struct PengirimPedaree;

impl FromRequestParts<AppState> for PengirimPedaree {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let nilai = parts.headers.get("X-Service-Key").and_then(|v| v.to_str().ok());
        state.keyring_pedaree.verifikasi(nilai, chrono::Utc::now())?;
        Ok(PengirimPedaree)
    }
}
