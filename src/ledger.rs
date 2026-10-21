//! Ledger fee vendor: pencairan H+1, denda, deposit, penarikan. Append-only, integer rupiah,
//! saldo tidak pernah negatif (sisa denda dibawa sebagai `utang_denda`).

use crate::{
    error::AppError,
    fee::{self, hitung_bagi_hasil},
    pesanan::{tanggal_wib, wib},
};
use chrono::{DateTime, Timelike, Utc};
use sqlx::{PgPool, Postgres, Transaction};

/// Saldo bisa ditarik mulai 06.00 WIB.
pub const JAM_BUKA_TARIK: u32 = 6;

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, Eq)]
pub struct Akun {
    pub id_vendor: String,
    pub prioritas: bool,
    pub saldo: i64,
    pub utang_denda: i64,
    pub deposit_terkumpul: i64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HasilTarik {
    pub diterima: i64,
    pub biaya_transfer: i64,
    pub saldo: i64,
}

const KOLOM_AKUN: &str = "id_vendor, prioritas, saldo, utang_denda, deposit_terkumpul";

pub async fn atur_prioritas(db: &PgPool, id_vendor: &str, prioritas: bool) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO vendor_akun (id_vendor, prioritas) VALUES ($1, $2)
         ON CONFLICT (id_vendor) DO UPDATE SET prioritas = EXCLUDED.prioritas, diubah_pada = now()",
    )
    .bind(id_vendor)
    .bind(prioritas)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn akun(db: &PgPool, id_vendor: &str) -> Result<Option<Akun>, AppError> {
    Ok(
        sqlx::query_as(&format!("SELECT {KOLOM_AKUN} FROM vendor_akun WHERE id_vendor = $1"))
            .bind(id_vendor)
            .fetch_optional(db)
            .await?,
    )
}

async fn kunci_akun(tx: &mut Transaction<'_, Postgres>, id_vendor: &str) -> Result<Akun, AppError> {
    sqlx::query("INSERT INTO vendor_akun (id_vendor) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(id_vendor)
        .execute(&mut **tx)
        .await?;
    Ok(sqlx::query_as(&format!(
        "SELECT {KOLOM_AKUN} FROM vendor_akun WHERE id_vendor = $1 FOR UPDATE"
    ))
    .bind(id_vendor)
    .fetch_one(&mut **tx)
    .await?)
}

struct Entri<'a> {
    id_vendor: &'a str,
    jenis: &'a str,
    pesanan_kode: Option<&'a str>,
    jumlah: i64,
    bagi: Option<fee::BagiHasil>,
    biaya_transfer: i64,
    saldo_setelah: i64,
    utang_setelah: i64,
    waktu: DateTime<Utc>,
}

async fn catat(tx: &mut Transaction<'_, Postgres>, e: Entri<'_>) -> Result<(), AppError> {
    let (f, b, ov, ok) = e
        .bagi
        .map(|x| (x.fee, x.bonus_prioritas, x.ongkir_vendor, x.ongkir_kas))
        .unwrap_or_default();
    sqlx::query(
        "INSERT INTO ledger_entri (id_vendor, jenis, pesanan_kode, jumlah, fee, bonus_prioritas, ongkir_vendor,
             ongkir_kas, biaya_transfer, saldo_setelah, utang_denda_setelah, dibuat_pada)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(e.id_vendor)
    .bind(e.jenis)
    .bind(e.pesanan_kode)
    .bind(e.jumlah)
    .bind(f)
    .bind(b)
    .bind(ov)
    .bind(ok)
    .bind(e.biaya_transfer)
    .bind(e.saldo_setelah)
    .bind(e.utang_setelah)
    .bind(e.waktu)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn simpan_akun(tx: &mut Transaction<'_, Postgres>, a: &Akun, waktu: DateTime<Utc>) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE vendor_akun SET saldo = $2, utang_denda = $3, deposit_terkumpul = $4, diubah_pada = $5
         WHERE id_vendor = $1",
    )
    .bind(&a.id_vendor)
    .bind(a.saldo)
    .bind(a.utang_denda)
    .bind(a.deposit_terkumpul)
    .bind(waktu)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Cairkan fee satu pesanan `selesai`: kredit fee 12% (+0,5% Prioritas) + ongkir vendor Rp4.000,
/// lalu tahan 10% fee untuk deposit sampai genap Rp100.000. Idempoten: `Ok(false)` bila sudah dicairkan.
pub async fn cairkan_pesanan(db: &PgPool, kode: &str, sekarang: DateTime<Utc>) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;
    let baris: Option<(String, i64, String)> =
        sqlx::query_as("SELECT id_vendor, total_item, status FROM pesanan WHERE kode = $1 FOR UPDATE")
            .bind(kode)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((id_vendor, total_item, status)) = baris else {
        return Err(AppError::tidak_ditemukan(
            "PESANAN_TIDAK_DITEMUKAN",
            "Pesanan tidak ditemukan",
        ));
    };
    if status != "selesai" {
        return Err(AppError::validasi(
            "PESANAN_BELUM_SELESAI",
            "Fee hanya dicairkan untuk pesanan berstatus selesai",
        ));
    }
    let sudah: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_entri WHERE pesanan_kode = $1 AND jenis = 'pencairan_pesanan'")
            .bind(kode)
            .fetch_one(&mut *tx)
            .await?;
    if sudah > 0 {
        return Ok(false);
    }

    let mut a = kunci_akun(&mut tx, &id_vendor).await?;
    let bagi = hitung_bagi_hasil(total_item, a.prioritas);
    (a.saldo, a.utang_denda) = fee::terapkan_kredit(a.saldo, a.utang_denda, bagi.diterima_vendor);
    catat(
        &mut tx,
        Entri {
            id_vendor: &id_vendor,
            jenis: "pencairan_pesanan",
            pesanan_kode: Some(kode),
            jumlah: bagi.diterima_vendor,
            bagi: Some(bagi),
            biaya_transfer: 0,
            saldo_setelah: a.saldo,
            utang_setelah: a.utang_denda,
            waktu: sekarang,
        },
    )
    .await?;

    // Deposit: ditahan dari fee (bukan potong langsung dari modal), tidak pernah membuat saldo negatif.
    let tahan = fee::tahan_deposit(bagi.fee, a.deposit_terkumpul).min(a.saldo);
    if tahan > 0 {
        a.saldo -= tahan;
        a.deposit_terkumpul += tahan;
        catat(
            &mut tx,
            Entri {
                id_vendor: &id_vendor,
                jenis: "deposit",
                pesanan_kode: Some(kode),
                jumlah: -tahan,
                bagi: None,
                biaya_transfer: 0,
                saldo_setelah: a.saldo,
                utang_setelah: a.utang_denda,
                waktu: sekarang,
            },
        )
        .await?;
    }
    simpan_akun(&mut tx, &a, sekarang).await?;
    tx.commit().await?;
    Ok(true)
}

/// Job `cair-fee` (00.00 WIB): cairkan pesanan `selesai` yang jadwal serahnya sudah H-1 atau lebih lama.
pub async fn cairkan_h_plus_1(db: &PgPool, sekarang: DateTime<Utc>) -> Result<usize, AppError> {
    let kode: Vec<String> = sqlx::query_scalar(
        "SELECT p.kode FROM pesanan p
         WHERE p.status = 'selesai' AND p.jadwal_serah < $1
           AND NOT EXISTS (SELECT 1 FROM ledger_entri l WHERE l.pesanan_kode = p.kode AND l.jenis = 'pencairan_pesanan')
         ORDER BY p.kode",
    )
    .bind(tanggal_wib(sekarang))
    .fetch_all(db)
    .await?;
    let mut n = 0;
    for k in kode {
        if cairkan_pesanan(db, &k, sekarang).await? {
            n += 1;
        }
    }
    Ok(n)
}

/// Denda batal sepihak < 2 jam (Rp15.000) untuk pesanan `dibatalkan_vendor`. Dipotong dari saldo;
/// saldo tidak negatif, sisa dibawa ke minggu depan. Idempoten: `Ok(false)` bila sudah dicatat.
pub async fn catat_denda(db: &PgPool, kode: &str, sekarang: DateTime<Utc>) -> Result<bool, AppError> {
    let mut tx = db.begin().await?;
    let baris: Option<(String, String)> =
        sqlx::query_as("SELECT id_vendor, status FROM pesanan WHERE kode = $1 FOR UPDATE")
            .bind(kode)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((id_vendor, status)) = baris else {
        return Err(AppError::tidak_ditemukan(
            "PESANAN_TIDAK_DITEMUKAN",
            "Pesanan tidak ditemukan",
        ));
    };
    if status != "dibatalkan_vendor" {
        return Err(AppError::validasi(
            "DENDA_TIDAK_BERLAKU",
            "Denda hanya untuk pesanan dibatalkan vendor",
        ));
    }
    let sudah: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ledger_entri WHERE pesanan_kode = $1 AND jenis = 'denda'")
            .bind(kode)
            .fetch_one(&mut *tx)
            .await?;
    if sudah > 0 {
        return Ok(false);
    }
    let mut a = kunci_akun(&mut tx, &id_vendor).await?;
    let saldo_awal = a.saldo;
    (a.saldo, a.utang_denda) = fee::terapkan_denda(a.saldo, a.utang_denda, fee::DENDA_BATAL_SEPIHAK);
    catat(
        &mut tx,
        Entri {
            id_vendor: &id_vendor,
            jenis: "denda",
            pesanan_kode: Some(kode),
            jumlah: a.saldo - saldo_awal,
            bagi: None,
            biaya_transfer: 0,
            saldo_setelah: a.saldo,
            utang_setelah: a.utang_denda,
            waktu: sekarang,
        },
    )
    .await?;
    simpan_akun(&mut tx, &a, sekarang).await?;
    tx.commit().await?;
    Ok(true)
}

/// Penarikan: minimal Rp50.000, biaya transfer Rp2.500 ditanggung vendor, mulai 06.00 WIB.
/// Saldo berkurang sebesar `jumlah`; vendor menerima `jumlah - 2.500`.
pub async fn tarik(db: &PgPool, id_vendor: &str, jumlah: i64, sekarang: DateTime<Utc>) -> Result<HasilTarik, AppError> {
    if sekarang.with_timezone(&wib()).hour() < JAM_BUKA_TARIK {
        return Err(AppError::validasi(
            "PENARIKAN_BELUM_BUKA",
            "Saldo dapat ditarik mulai pukul 06.00 WIB",
        ));
    }
    if jumlah < fee::PENARIKAN_MINIMAL {
        return Err(AppError::validasi(
            "PENARIKAN_DI_BAWAH_MINIMUM",
            "Penarikan minimal Rp50.000",
        ));
    }
    let mut tx = db.begin().await?;
    let mut a = kunci_akun(&mut tx, id_vendor).await?;
    if a.saldo < jumlah {
        return Err(AppError::validasi("SALDO_TIDAK_CUKUP", "Saldo tidak mencukupi"));
    }
    a.saldo -= jumlah;
    catat(
        &mut tx,
        Entri {
            id_vendor,
            jenis: "penarikan",
            pesanan_kode: None,
            jumlah: -jumlah,
            bagi: None,
            biaya_transfer: fee::BIAYA_TRANSFER,
            saldo_setelah: a.saldo,
            utang_setelah: a.utang_denda,
            waktu: sekarang,
        },
    )
    .await?;
    simpan_akun(&mut tx, &a, sekarang).await?;
    tx.commit().await?;
    Ok(HasilTarik {
        diterima: jumlah - fee::BIAYA_TRANSFER,
        biaya_transfer: fee::BIAYA_TRANSFER,
        saldo: a.saldo,
    })
}
