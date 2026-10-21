//! Logika pesanan: checkout idempoten, transisi status beraudit, pengalihan, kedaluwarsa.

use super::{
    format_kode, jadwal_serah, lewat_jam_tutup, status::StatusPesanan, subtotal, tahun_wib, tanggal_wib,
    AMBANG_GROSIR_GRAM, BATAS_KONFIRMASI_MENIT, KEDALUWARSA_BAYAR_MENIT, MAKS_ALIH, MAKS_BARIS, ONGKIR,
    UMUR_MAKS_KATALOG_JAM,
};
use crate::error::AppError;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ItemBaru {
    pub sku_lumbung: String,
    pub jumlah_gram: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PesananBaru {
    pub id: String,
    pub id_vendor: String,
    pub item: Vec<ItemBaru>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PesananDibuat {
    pub kode: String,
    pub total_bayar: i64,
    pub kedaluwarsa_bayar: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PesananRow {
    pub id: Uuid,
    pub kode: String,
    pub id_rumah_tangga: String,
    pub id_vendor: String,
    pub status: String,
    pub total_bayar: i64,
    pub kedaluwarsa_bayar: DateTime<Utc>,
    pub request_hash: String,
    pub jumlah_alih: i16,
    pub dikonfirmasi_pada: Option<DateTime<Utc>>,
}

impl PesananRow {
    pub fn status(&self) -> StatusPesanan {
        StatusPesanan::dari_str(&self.status).expect("CHECK constraint menjamin status sah")
    }
}

const KOLOM: &str = "id, kode, id_rumah_tangga, id_vendor, status, total_bayar, kedaluwarsa_bayar, request_hash, \
                     jumlah_alih, dikonfirmasi_pada";

pub async fn cari_by_kode(db: &PgPool, kode: &str) -> Result<Option<PesananRow>, AppError> {
    Ok(sqlx::query_as(&format!("SELECT {KOLOM} FROM pesanan WHERE kode = $1"))
        .bind(kode)
        .fetch_optional(db)
        .await?)
}

async fn kunci_by_kode(tx: &mut Transaction<'_, Postgres>, kode: &str) -> Result<PesananRow, AppError> {
    sqlx::query_as(&format!("SELECT {KOLOM} FROM pesanan WHERE kode = $1 FOR UPDATE"))
        .bind(kode)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| AppError::tidak_ditemukan("PESANAN_TIDAK_DITEMUKAN", "Pesanan tidak ditemukan"))
}

/// Pindah status: kunci baris, validasi transisi sah (422 bila ilegal), catat audit append-only.
pub async fn pindah_status(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    ke: StatusPesanan,
    aktor: &str,
    sekarang: DateTime<Utc>,
) -> Result<StatusPesanan, AppError> {
    let saat_ini: String = sqlx::query_scalar("SELECT status FROM pesanan WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    let dari = StatusPesanan::dari_str(&saat_ini).expect("CHECK constraint menjamin status sah");
    if !dari.boleh_ke(ke) {
        return Err(AppError::validasi(
            "TRANSISI_STATUS_ILEGAL",
            format!("Transisi {} -> {} tidak diizinkan", dari.as_str(), ke.as_str()),
        )
        .dengan_detail(json!({ "dari": dari.as_str(), "ke": ke.as_str() })));
    }
    sqlx::query("UPDATE pesanan SET status = $2, diubah_pada = $3 WHERE id = $1")
        .bind(id)
        .bind(ke.as_str())
        .bind(sekarang)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO pesanan_audit (pesanan_id, dari_status, ke_status, aktor, waktu) VALUES ($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(dari.as_str())
        .bind(ke.as_str())
        .bind(aktor)
        .bind(sekarang)
        .execute(&mut **tx)
        .await?;
    Ok(dari)
}

// ---------------------------------------------------------------- checkout

fn validasi_bentuk(b: &PesananBaru, maks_berat_gram: i64) -> Result<i64, AppError> {
    if !super::ulid_valid(&b.id) {
        return Err(AppError::validasi(
            "ID_PESANAN_TIDAK_VALID",
            "id pesanan harus ULID 26 karakter",
        ));
    }
    if b.id_vendor.trim().is_empty() {
        return Err(AppError::validasi("VENDOR_WAJIB", "id_vendor wajib diisi"));
    }
    if b.item.is_empty() {
        return Err(AppError::validasi("ITEM_KOSONG", "Pesanan minimal satu item"));
    }
    if b.item.len() > MAKS_BARIS {
        return Err(AppError::validasi(
            "BARIS_MELEBIHI_BATAS",
            format!("Maksimal {MAKS_BARIS} baris per pesanan"),
        ));
    }
    let mut berat = 0i64;
    for (i, it) in b.item.iter().enumerate() {
        if it.jumlah_gram < 1 {
            return Err(AppError::validasi("JUMLAH_TIDAK_VALID", "jumlah_gram minimal 1"));
        }
        if b.item[..i].iter().any(|x| x.sku_lumbung == it.sku_lumbung) {
            return Err(AppError::validasi(
                "SKU_DUPLIKAT",
                "SKU yang sama muncul lebih dari sekali",
            ));
        }
        berat = berat.saturating_add(it.jumlah_gram);
    }
    if berat > maks_berat_gram {
        return Err(AppError::validasi(
            "BERAT_MELEBIHI_BATAS",
            format!("Total berat melebihi {} gram", maks_berat_gram),
        )
        .dengan_detail(json!({ "berat_gram": berat, "batas_gram": maks_berat_gram })));
    }
    Ok(berat)
}

#[derive(sqlx::FromRow)]
struct BarisKatalog {
    sku_lumbung: String,
    nama: String,
    harga_per_kg: i64,
    stok_gram: i64,
    disinkron_pada: DateTime<Utc>,
}

fn dari_baris(r: &PesananRow) -> PesananDibuat {
    PesananDibuat {
        kode: r.kode.clone(),
        total_bayar: r.total_bayar,
        kedaluwarsa_bayar: r.kedaluwarsa_bayar,
    }
}

/// Checkout. `Ok((true, _))` = dibuat baru (201); `Ok((false, _))` = pengiriman ulang idempoten (200).
pub async fn buat_pesanan(
    db: &PgPool,
    sekarang: DateTime<Utc>,
    maks_berat_gram: i64,
    id_rumah_tangga: &str,
    idempotency_key: &str,
    body: &PesananBaru,
) -> Result<(bool, PesananDibuat), AppError> {
    let hash = serde_json::to_string(body).map_err(|e| AppError::internal(e.to_string()))?;

    // Pengiriman ulang dijawab lebih dulu (tetap 200 walau sudah lewat 20.00).
    if let Some(ada) = cari_idempoten(db, id_rumah_tangga, idempotency_key).await? {
        return jawab_ulang(&ada, &hash);
    }

    let berat = validasi_bentuk(body, maks_berat_gram)?;
    if lewat_jam_tutup(sekarang) {
        return Err(AppError::validasi(
            "DI_LUAR_JAM_PESAN",
            "Pesan hanya sampai pukul 20.00 WIB; serah besok 06.00-08.00",
        ));
    }

    let katalog: Vec<BarisKatalog> = sqlx::query_as(
        "SELECT sku_lumbung, nama, harga_per_kg, stok_gram, disinkron_pada FROM katalog_cermin WHERE id_vendor = $1",
    )
    .bind(&body.id_vendor)
    .fetch_all(db)
    .await?;
    if katalog.is_empty() {
        return Err(AppError::tidak_ditemukan(
            "VENDOR_TIDAK_DITEMUKAN",
            "Vendor atau katalognya tidak ditemukan",
        ));
    }

    let batas_umur = sekarang - Duration::hours(UMUR_MAKS_KATALOG_JAM);
    let mut baris = Vec::with_capacity(body.item.len());
    let mut total_item = 0i64;
    for it in &body.item {
        let k = katalog
            .iter()
            .find(|k| k.sku_lumbung == it.sku_lumbung)
            .ok_or_else(|| {
                AppError::validasi(
                    "SKU_TIDAK_ADA",
                    format!("SKU {} tidak ada di katalog vendor", it.sku_lumbung),
                )
            })?;
        if k.disinkron_pada < batas_umur {
            return Err(
                AppError::konflik("KATALOG_KEDALUWARSA", "Snapshot katalog lebih dari 24 jam")
                    .dengan_detail(json!({ "sku_lumbung": k.sku_lumbung })),
            );
        }
        if k.stok_gram < it.jumlah_gram {
            return Err(
                AppError::konflik("STOK_HABIS", format!("Stok {} tidak mencukupi", k.nama))
                    .dengan_detail(json!({ "sku_lumbung": k.sku_lumbung, "stok_gram": k.stok_gram })),
            );
        }
        let sub = subtotal(it.jumlah_gram, k.harga_per_kg);
        total_item += sub;
        baris.push((k, it.jumlah_gram, sub));
    }
    let total_bayar = total_item + ONGKIR;
    let kedaluwarsa_bayar = sekarang + Duration::minutes(KEDALUWARSA_BAYAR_MENIT);

    let mut tx = db.begin().await?;
    let tahun = tahun_wib(sekarang);
    let urutan: i64 = sqlx::query_scalar(
        "INSERT INTO urutan_kode (tahun, terakhir) VALUES ($1, 1)
         ON CONFLICT (tahun) DO UPDATE SET terakhir = urutan_kode.terakhir + 1 RETURNING terakhir",
    )
    .bind(tahun)
    .fetch_one(&mut *tx)
    .await?;
    let kode = format_kode(tahun, urutan);

    let masuk = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO pesanan (kode, id_klien, idempotency_key, request_hash, id_rumah_tangga, id_vendor, status,
             total_item, ongkir, total_bayar, berat_gram, grosir, jadwal_serah, kedaluwarsa_bayar, dibuat_pada, diubah_pada)
         VALUES ($1,$2,$3,$4,$5,$6,'menunggu_pembayaran',$7,$8,$9,$10,$11,$12,$13,$14,$14) RETURNING id",
    )
    .bind(&kode)
    .bind(&body.id)
    .bind(idempotency_key)
    .bind(&hash)
    .bind(id_rumah_tangga)
    .bind(&body.id_vendor)
    .bind(total_item)
    .bind(ONGKIR)
    .bind(total_bayar)
    .bind(berat)
    .bind(berat > AMBANG_GROSIR_GRAM)
    .bind(jadwal_serah(sekarang))
    .bind(kedaluwarsa_bayar)
    .bind(sekarang)
    .fetch_one(&mut *tx)
    .await;

    let id = match masuk {
        Ok(id) => id,
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            // Balapan: permintaan kembar sudah menang. Batalkan dan jawab ulang.
            tx.rollback().await?;
            let ada = cari_idempoten(db, id_rumah_tangga, idempotency_key)
                .await?
                .ok_or_else(|| AppError::konflik("PESANAN_BENTROK", "Pesanan bentrok; coba lagi"))?;
            return jawab_ulang(&ada, &hash);
        }
        Err(e) => return Err(e.into()),
    };

    for (k, gram, sub) in &baris {
        sqlx::query(
            "INSERT INTO pesanan_item (pesanan_id, sku_lumbung, nama, jumlah_gram, harga_per_kg, subtotal)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(id)
        .bind(&k.sku_lumbung)
        .bind(&k.nama)
        .bind(gram)
        .bind(k.harga_per_kg)
        .bind(sub)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO pesanan_audit (pesanan_id, dari_status, ke_status, aktor, waktu) VALUES ($1,NULL,$2,$3,$4)",
    )
    .bind(id)
    .bind(StatusPesanan::MenungguPembayaran.as_str())
    .bind(id_rumah_tangga)
    .bind(sekarang)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((
        true,
        PesananDibuat {
            kode,
            total_bayar,
            kedaluwarsa_bayar,
        },
    ))
}

async fn cari_idempoten(db: &PgPool, rt: &str, kunci: &str) -> Result<Option<PesananRow>, AppError> {
    Ok(sqlx::query_as(&format!(
        "SELECT {KOLOM} FROM pesanan WHERE id_rumah_tangga = $1 AND idempotency_key = $2"
    ))
    .bind(rt)
    .bind(kunci)
    .fetch_optional(db)
    .await?)
}

fn jawab_ulang(ada: &PesananRow, hash: &str) -> Result<(bool, PesananDibuat), AppError> {
    if ada.request_hash == hash {
        Ok((false, dari_baris(ada)))
    } else {
        Err(AppError::konflik(
            "IDEMPOTENCY_KEY_BENTROK",
            "Idempotency-Key sama dipakai dengan isi permintaan berbeda",
        ))
    }
}

// ------------------------------------------------------------ pembayaran

/// Catat pembayaran QRIS: `menunggu_pembayaran -> menunggu_konfirmasi`. Idempoten untuk `id_gateway` sama.
pub async fn konfirmasi_bayar(
    db: &PgPool,
    sekarang: DateTime<Utc>,
    kode: &str,
    id_rumah_tangga: &str,
    id_gateway: &str,
) -> Result<StatusPesanan, AppError> {
    let mut tx = db.begin().await?;
    let p = kunci_by_kode(&mut tx, kode).await?;
    if p.id_rumah_tangga != id_rumah_tangga {
        return Err(AppError::tidak_ditemukan(
            "PESANAN_TIDAK_DITEMUKAN",
            "Pesanan tidak ditemukan",
        ));
    }
    if p.status() != StatusPesanan::MenungguPembayaran {
        let tercatat: Option<String> = sqlx::query_scalar("SELECT id_gateway FROM pembayaran WHERE pesanan_id = $1")
            .bind(p.id)
            .fetch_optional(&mut *tx)
            .await?;
        return match tercatat {
            Some(g) if g == id_gateway => Ok(p.status()),
            Some(_) => Err(AppError::konflik(
                "PEMBAYARAN_SUDAH_DICATAT",
                "Pesanan sudah dibayar dengan id_gateway lain",
            )),
            None => Err(
                AppError::validasi("TRANSISI_STATUS_ILEGAL", "Pesanan tidak menunggu pembayaran")
                    .dengan_detail(json!({ "status": p.status().as_str() })),
            ),
        };
    }
    if sekarang >= p.kedaluwarsa_bayar {
        return Err(AppError::validasi(
            "PEMBAYARAN_KEDALUWARSA",
            "Batas bayar 15 menit terlewati",
        ));
    }
    sqlx::query("INSERT INTO pembayaran (pesanan_id, id_gateway, jumlah, dicatat_pada) VALUES ($1,$2,$3,$4)")
        .bind(p.id)
        .bind(id_gateway)
        .bind(p.total_bayar)
        .bind(sekarang)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(d) if d.is_unique_violation() => {
                AppError::konflik("ID_GATEWAY_DIPAKAI", "id_gateway sudah dipakai pesanan lain")
            }
            other => other.into(),
        })?;
    pindah_status(
        &mut tx,
        p.id,
        StatusPesanan::MenungguKonfirmasi,
        id_rumah_tangga,
        sekarang,
    )
    .await?;
    sqlx::query("UPDATE pesanan SET batas_konfirmasi = $2 WHERE id = $1")
        .bind(p.id)
        .bind(sekarang + Duration::minutes(BATAS_KONFIRMASI_MENIT))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusPesanan::MenungguKonfirmasi)
}

// ------------------------------------------------------------ konfirmasi vendor

pub struct HasilKonfirmasi {
    pub status: StatusPesanan,
    pub dikonfirmasi: Option<DateTime<Utc>>,
}

/// Vendor menerima (`disiapkan`) atau menolak (pengalihan / batal sistem + refund H+1).
pub async fn konfirmasi_vendor(
    db: &PgPool,
    sekarang: DateTime<Utc>,
    kode: &str,
    id_vendor: &str,
    terima: bool,
) -> Result<HasilKonfirmasi, AppError> {
    let mut tx = db.begin().await?;
    let p = kunci_by_kode(&mut tx, kode).await?;
    if p.id_vendor != id_vendor {
        return Err(AppError::tidak_ditemukan(
            "PESANAN_TIDAK_DITEMUKAN",
            "Pesanan tidak ditemukan",
        ));
    }
    let aktor = format!("vendor:{id_vendor}");
    // Pengiriman ulang antrean luring: hasil sama, tanpa efek ganda.
    if terima && p.status() == StatusPesanan::Disiapkan {
        return Ok(HasilKonfirmasi {
            status: p.status(),
            dikonfirmasi: p.dikonfirmasi_pada,
        });
    }
    if terima {
        pindah_status(&mut tx, p.id, StatusPesanan::Disiapkan, &aktor, sekarang).await?;
        sqlx::query("UPDATE pesanan SET dikonfirmasi_pada = $2, batas_konfirmasi = NULL WHERE id = $1")
            .bind(p.id)
            .bind(sekarang)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(HasilKonfirmasi {
            status: StatusPesanan::Disiapkan,
            dikonfirmasi: Some(sekarang),
        });
    }
    let status = gagal_konfirmasi(&mut tx, &p, &aktor, sekarang).await?;
    tx.commit().await?;
    Ok(HasilKonfirmasi {
        status,
        dikonfirmasi: None,
    })
}

/// Vendor menolak / timeout 30 menit: alihkan (bila pengalihan < 2) atau batal sistem + refund penuh H+1.
async fn gagal_konfirmasi(
    tx: &mut Transaction<'_, Postgres>,
    p: &PesananRow,
    aktor: &str,
    sekarang: DateTime<Utc>,
) -> Result<StatusPesanan, AppError> {
    if p.status() != StatusPesanan::MenungguKonfirmasi {
        return Err(
            AppError::validasi("TRANSISI_STATUS_ILEGAL", "Pesanan tidak menunggu konfirmasi")
                .dengan_detail(json!({ "status": p.status().as_str() })),
        );
    }
    if p.jumlah_alih < MAKS_ALIH {
        pindah_status(tx, p.id, StatusPesanan::Dialihkan, aktor, sekarang).await?;
        sqlx::query("UPDATE pesanan SET batas_konfirmasi = NULL WHERE id = $1")
            .bind(p.id)
            .execute(&mut **tx)
            .await?;
        Ok(StatusPesanan::Dialihkan)
    } else {
        batalkan_dengan_refund(tx, p.id, aktor, sekarang).await?;
        Ok(StatusPesanan::DibatalkanSistem)
    }
}

async fn batalkan_dengan_refund(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    aktor: &str,
    sekarang: DateTime<Utc>,
) -> Result<(), AppError> {
    pindah_status(tx, id, StatusPesanan::DibatalkanSistem, aktor, sekarang).await?;
    let refund: NaiveDate = tanggal_wib(sekarang) + Duration::days(1);
    sqlx::query("UPDATE pesanan SET refund_jatuh_tempo = $2, batas_konfirmasi = NULL WHERE id = $1")
        .bind(id)
        .bind(refund)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// ------------------------------------------------------------ pengalihan (admin)

/// Alihkan ke vendor cadangan; maksimal 2 kali per pesanan (lebih = 422).
pub async fn alihkan(
    db: &PgPool,
    sekarang: DateTime<Utc>,
    kode: &str,
    id_vendor_cadangan: &str,
    aktor: &str,
) -> Result<(), AppError> {
    let ada: i64 = sqlx::query_scalar("SELECT count(*) FROM katalog_cermin WHERE id_vendor = $1")
        .bind(id_vendor_cadangan)
        .fetch_one(db)
        .await?;
    if ada == 0 {
        return Err(AppError::tidak_ditemukan(
            "VENDOR_TIDAK_DITEMUKAN",
            "Vendor cadangan tidak ditemukan",
        ));
    }
    let mut tx = db.begin().await?;
    let p = kunci_by_kode(&mut tx, kode).await?;
    if !matches!(p.status(), StatusPesanan::MenungguKonfirmasi | StatusPesanan::Dialihkan) {
        return Err(AppError::validasi(
            "TRANSISI_STATUS_ILEGAL",
            "Pesanan tidak dapat dialihkan pada status ini",
        )
        .dengan_detail(json!({ "status": p.status().as_str() })));
    }
    if p.jumlah_alih >= MAKS_ALIH {
        return Err(AppError::validasi(
            "ALIH_MELEBIHI_BATAS",
            "Pesanan sudah dialihkan 2 kali",
        ));
    }
    if p.id_vendor == id_vendor_cadangan {
        return Err(AppError::konflik(
            "VENDOR_SAMA",
            "Vendor cadangan sama dengan vendor saat ini",
        ));
    }
    if p.status() == StatusPesanan::MenungguKonfirmasi {
        pindah_status(&mut tx, p.id, StatusPesanan::Dialihkan, aktor, sekarang).await?;
    }
    pindah_status(&mut tx, p.id, StatusPesanan::MenungguKonfirmasi, aktor, sekarang).await?;
    sqlx::query(
        "UPDATE pesanan SET id_vendor = $2, jumlah_alih = jumlah_alih + 1, batas_konfirmasi = $3 WHERE id = $1",
    )
    .bind(p.id)
    .bind(id_vendor_cadangan)
    .bind(sekarang + Duration::minutes(BATAS_KONFIRMASI_MENIT))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

// ------------------------------------------------------------ pekerjaan berkala

/// Job `timeout-konfirmasi` (tiap 1 menit): konfirmasi lewat 30 menit dialihkan, atau dibatalkan + refund H+1
/// bila pengalihan sudah 2 kali. Mengembalikan jumlah pesanan yang diproses.
pub async fn tangani_timeout_konfirmasi(db: &PgPool, sekarang: DateTime<Utc>) -> Result<usize, AppError> {
    let kode: Vec<String> =
        sqlx::query_scalar("SELECT kode FROM pesanan WHERE status = 'menunggu_konfirmasi' AND batas_konfirmasi < $1")
            .bind(sekarang)
            .fetch_all(db)
            .await?;
    let mut n = 0;
    for k in kode {
        let mut tx = db.begin().await?;
        let p = kunci_by_kode(&mut tx, &k).await?;
        // Baris mungkin sudah berubah sejak daftar diambil.
        if p.status() == StatusPesanan::MenungguKonfirmasi {
            gagal_konfirmasi(&mut tx, &p, "sistem:timeout-konfirmasi", sekarang).await?;
            n += 1;
        }
        tx.commit().await?;
    }
    Ok(n)
}

/// Pesanan yang tidak dibayar dalam 15 menit menjadi `kedaluwarsa`.
pub async fn kedaluwarsakan_belum_bayar(db: &PgPool, sekarang: DateTime<Utc>) -> Result<usize, AppError> {
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM pesanan WHERE status = 'menunggu_pembayaran' AND kedaluwarsa_bayar <= $1")
            .bind(sekarang)
            .fetch_all(db)
            .await?;
    let mut n = 0;
    for id in ids {
        let mut tx = db.begin().await?;
        if pindah_status(
            &mut tx,
            id,
            StatusPesanan::Kedaluwarsa,
            "sistem:kedaluwarsa-bayar",
            sekarang,
        )
        .await
        .is_ok()
        {
            n += 1;
        }
        tx.commit().await?;
    }
    Ok(n)
}
