//! Handler HTTP: POST /pesanan, /pesanan/{kode}/bayar, /vendor/pesanan/{kode}/konfirmasi,
//! /admin/pesanan/{kode}/alihkan.

use super::{kode_valid, layanan, ulid_valid};
use crate::{
    auth::{AuthUser, Role, Skup},
    error::AppError,
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::json;

fn kunci_idempoten(h: &HeaderMap) -> Result<String, AppError> {
    let v = h
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::permintaan("IDEMPOTENCY_KEY_WAJIB", "Header Idempotency-Key (ULID) wajib"))?;
    if ulid_valid(v) {
        Ok(v.to_string())
    } else {
        Err(AppError::permintaan(
            "IDEMPOTENCY_KEY_TIDAK_VALID",
            "Idempotency-Key harus ULID 26 karakter",
        ))
    }
}

fn badan<T: DeserializeOwned>(b: &Bytes) -> Result<T, AppError> {
    serde_json::from_slice(b)
        .map_err(|e| AppError::permintaan("PERMINTAAN_TIDAK_VALID", format!("Badan JSON tidak valid: {e}")))
}

fn kode_path(kode: &str) -> Result<(), AppError> {
    if kode_valid(kode) {
        Ok(())
    } else {
        Err(AppError::permintaan(
            "KODE_PESANAN_TIDAK_VALID",
            "Kode pesanan harus berformat T-YYYY-NNNNNN",
        ))
    }
}

pub async fn buat(
    State(st): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    user.wajib_peran(&[Role::Household])?;
    let kunci = kunci_idempoten(&headers)?;
    let req: layanan::PesananBaru = badan(&body)?;
    let (baru, hasil) = layanan::buat_pesanan(
        &st.db,
        st.sekarang(),
        st.config.batas_berat_gram,
        &user.sub,
        &kunci,
        &req,
    )
    .await?;
    let kode = if baru { StatusCode::CREATED } else { StatusCode::OK };
    Ok((kode, Json(hasil)).into_response())
}

#[derive(Deserialize)]
struct BayarReq {
    id_gateway: String,
}

pub async fn bayar(
    State(st): State<AppState>,
    user: AuthUser,
    Path(kode): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    user.wajib_peran(&[Role::Household])?;
    kode_path(&kode)?;
    kunci_idempoten(&headers)?;
    let req: BayarReq = badan(&body)?;
    if req.id_gateway.trim().is_empty() {
        return Err(AppError::validasi("ID_GATEWAY_WAJIB", "id_gateway wajib diisi"));
    }
    let status = layanan::konfirmasi_bayar(&st.db, st.sekarang(), &kode, &user.sub, &req.id_gateway).await?;
    Ok(Json(json!({ "status": status })))
}

#[derive(Deserialize)]
struct KonfirmasiReq {
    terima: bool,
    #[serde(default)]
    alasan: Option<String>,
}

pub async fn konfirmasi_vendor(
    State(st): State<AppState>,
    user: AuthUser,
    Path(kode): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    user.wajib_peran(&[Role::Vendor])?;
    kode_path(&kode)?;
    kunci_idempoten(&headers)?;
    let req: KonfirmasiReq = badan(&body)?;
    if !req.terima && req.alasan.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err(AppError::validasi("ALASAN_WAJIB", "Penolakan wajib menyertakan alasan"));
    }
    let Skup::Vendor(id_vendor) = user.skup() else {
        return Err(AppError::dilarang("PERAN_DITOLAK", "Hanya vendor"));
    };
    let h = layanan::konfirmasi_vendor(&st.db, st.sekarang(), &kode, &id_vendor, req.terima).await?;
    Ok(Json(json!({ "status": h.status, "dikonfirmasi": h.dikonfirmasi })))
}

#[derive(Deserialize)]
struct AlihkanReq {
    id_vendor_cadangan: String,
}

pub async fn alihkan(
    State(st): State<AppState>,
    user: AuthUser,
    Path(kode): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    user.wajib_peran(&[Role::Admin])?;
    kode_path(&kode)?;
    let req: AlihkanReq = badan(&body)?;
    if req.id_vendor_cadangan.trim().is_empty() {
        return Err(AppError::validasi("VENDOR_WAJIB", "id_vendor_cadangan wajib diisi"));
    }
    layanan::alihkan(
        &st.db,
        st.sekarang(),
        &kode,
        &req.id_vendor_cadangan,
        &format!("admin:{}", user.sub),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "id": kode, "dicatat_oleh": user.sub })))
}
