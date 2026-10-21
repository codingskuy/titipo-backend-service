//! Penerima sinyal `pedaree.stok.v1` (Pedaree -> TitipO). ANJURAN restock saja:
//! tidak pernah membuat pesanan (modul ini sengaja tidak menyentuh tabel `pesanan`).

use crate::{auth::PengirimPedaree, error::AppError, state::AppState};
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

pub const JENIS_SINYAL: [&str; 3] = ["stok.menipis", "stok.habis", "kedaluwarsa.dekat"];
/// 13 satuan baku (KONTEKS-LINTAS-DIVISI §6).
pub const SATUAN_BAKU: [&str; 13] = [
    "gram", "kg", "ml", "liter", "sdm", "sdt", "pcs", "siung", "buah", "ikat", "batang", "bonggol", "lembar",
];
/// Anjuran relevan selama 48 jam.
pub const RELEVANSI_JAM: i64 = 48;

#[derive(Debug, Deserialize)]
struct Sinyal {
    jenis: String,
    item_id: String,
    tanggal: NaiveDate,
    item: String,
    sisa: f64,
    satuan: String,
    kedaluwarsa: NaiveDate,
    #[serde(default)]
    recipe_id: Option<String>,
}

fn tak_valid(pesan: impl Into<String>) -> AppError {
    AppError::validasi("PAYLOAD_SINYAL_TIDAK_VALID", pesan)
}

fn periksa(raw: Value) -> Result<Sinyal, AppError> {
    // Jenis diperiksa lebih dulu agar kode galatnya spesifik.
    match raw.get("jenis").and_then(Value::as_str) {
        Some(j) if JENIS_SINYAL.contains(&j) => {}
        Some(j) => {
            return Err(
                AppError::validasi("JENIS_SINYAL_TIDAK_DIKENAL", format!("Jenis sinyal tidak dikenal: {j}"))
                    .dengan_detail(json!({ "diizinkan": JENIS_SINYAL })),
            )
        }
        None => return Err(tak_valid("jenis wajib berupa string")),
    }
    let s: Sinyal = serde_json::from_value(raw).map_err(|e| tak_valid(e.to_string()))?;
    if s.item_id.trim().is_empty() || s.item.trim().is_empty() {
        return Err(tak_valid("item_id dan item wajib diisi"));
    }
    if !s.sisa.is_finite() || s.sisa < 0.0 {
        return Err(tak_valid("sisa harus angka >= 0"));
    }
    if !SATUAN_BAKU.contains(&s.satuan.as_str()) {
        return Err(
            AppError::validasi("SATUAN_TIDAK_VALID", format!("Satuan {} bukan satuan baku", s.satuan))
                .dengan_detail(json!({ "diizinkan": SATUAN_BAKU })),
        );
    }
    Ok(s)
}

/// Catat sinyal. `true` = baru dicatat; `false` = duplikat `item_id + tanggal` (diabaikan).
async fn catat(db: &PgPool, s: &Sinyal, sekarang: DateTime<Utc>) -> Result<bool, AppError> {
    let baru = sqlx::query(
        "INSERT INTO sinyal_restock (jenis, item_id, tanggal, item, sisa, satuan, kedaluwarsa, recipe_id, diterima_pada)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (item_id, tanggal) DO NOTHING",
    )
    .bind(&s.jenis)
    .bind(&s.item_id)
    .bind(s.tanggal)
    .bind(&s.item)
    .bind(s.sisa)
    .bind(&s.satuan)
    .bind(s.kedaluwarsa)
    .bind(&s.recipe_id)
    .bind(sekarang)
    .execute(db)
    .await?
    .rows_affected();
    Ok(baru == 1)
}

pub async fn terima(State(st): State<AppState>, _: PengirimPedaree, body: Bytes) -> Result<Response, AppError> {
    let raw: Value = serde_json::from_slice(&body)
        .map_err(|e| AppError::permintaan("PERMINTAAN_TIDAK_VALID", format!("Badan JSON tidak valid: {e}")))?;
    let sinyal = periksa(raw)?;
    if catat(&st.db, &sinyal, st.sekarang()).await? {
        Ok((
            StatusCode::CREATED,
            Json(json!({ "status": "dicatat_sebagai_anjuran" })),
        )
            .into_response())
    } else {
        Ok((StatusCode::OK, Json(json!({ "status": "duplikat_diabaikan" }))).into_response())
    }
}

#[derive(sqlx::FromRow, Debug, PartialEq)]
pub struct Saran {
    pub jenis: String,
    pub item_id: String,
    pub item: String,
    pub sisa: f64,
    pub satuan: String,
    pub kedaluwarsa: NaiveDate,
    pub recipe_id: Option<String>,
}

/// Daftar saran restock yang masih relevan (48 jam terakhir), terbaru dulu.
pub async fn saran_aktif(db: &PgPool, sekarang: DateTime<Utc>) -> Result<Vec<Saran>, sqlx::Error> {
    sqlx::query_as(
        "SELECT jenis, item_id, item, sisa, satuan, kedaluwarsa, recipe_id FROM sinyal_restock
         WHERE diterima_pada > $1 ORDER BY diterima_pada DESC, item_id",
    )
    .bind(sekarang - Duration::hours(RELEVANSI_JAM))
    .fetch_all(db)
    .await
}
