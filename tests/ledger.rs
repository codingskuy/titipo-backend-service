use chrono::{DateTime, Duration, TimeZone, Utc};
use sqlx::PgPool;
use titipo_backend_service::ledger::{self, Akun};

fn t0() -> DateTime<Utc> {
    // 2026-01-08 07.00 WIB (sesudah 06.00)
    Utc.with_ymd_and_hms(2026, 1, 8, 0, 0, 0).unwrap()
}

async fn pesanan(db: &PgPool, kode: &str, vendor: &str, total_item: i64, status: &str, serah: &str) {
    sqlx::query(
        "INSERT INTO pesanan (kode, id_klien, idempotency_key, request_hash, id_rumah_tangga, id_vendor, status,
             total_item, ongkir, total_bayar, berat_gram, jadwal_serah, kedaluwarsa_bayar)
         VALUES ($1,'K',$1,'h','RT-1',$2,$3,$4,5000,$4 + 5000,3000,$5::date,now())",
    )
    .bind(kode)
    .bind(vendor)
    .bind(status)
    .bind(total_item)
    .bind(serah)
    .execute(db)
    .await
    .unwrap();
}

/// Akun dengan deposit sudah genap (Rp100.000) agar kasus mengikat A/B/C terbaca murni.
async fn akun_deposit_genap(db: &PgPool, vendor: &str, prioritas: bool) {
    ledger::atur_prioritas(db, vendor, prioritas).await.unwrap();
    sqlx::query("UPDATE vendor_akun SET deposit_terkumpul = 100000 WHERE id_vendor = $1")
        .bind(vendor)
        .execute(db)
        .await
        .unwrap();
}

async fn akun(db: &PgPool, v: &str) -> Akun {
    ledger::akun(db, v).await.unwrap().unwrap()
}

#[sqlx::test]
async fn kasus_a_vendor_aktif(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-06").await;
    assert!(ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap());
    let (fee, bonus, ov, ok, jumlah): (i64, i64, i64, i64, i64) =
        sqlx::query_as("SELECT fee, bonus_prioritas, ongkir_vendor, ongkir_kas, jumlah FROM ledger_entri")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!((fee, bonus, ov, ok, jumlah), (5_640, 0, 4_000, 1_000, 9_640));
    assert_eq!(akun(&db, "V-SARI").await.saldo, 9_640);
}

#[sqlx::test]
async fn kasus_b_vendor_prioritas(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", true).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-06").await;
    ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap();
    let (fee, bonus, jumlah): (i64, i64, i64) = sqlx::query_as("SELECT fee, bonus_prioritas, jumlah FROM ledger_entri")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!((fee, bonus, jumlah), (5_640, 235, 9_875));
    assert_eq!(akun(&db, "V-SARI").await.saldo, 9_875);
}

#[sqlx::test]
async fn kasus_c_denda_saldo_tidak_negatif_sisa_dibawa(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-06").await;
    pesanan(
        &db,
        "T-2026-000002",
        "V-SARI",
        20_000,
        "dibatalkan_vendor",
        "2026-01-06",
    )
    .await;
    ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap(); // +9.640
    assert!(ledger::catat_denda(&db, "T-2026-000002", t0()).await.unwrap()); // -15.000
    let a = akun(&db, "V-SARI").await;
    assert_eq!((a.saldo, a.utang_denda), (0, 5_360)); // minus 5.360 -> saldo 0, sisa dibawa

    // Minggu depan: kredit baru melunasi sisa denda lebih dulu (9.640 - 5.360 = 4.280).
    pesanan(&db, "T-2026-000003", "V-SARI", 47_000, "selesai", "2026-01-13").await;
    ledger::cairkan_pesanan(&db, "T-2026-000003", t0() + Duration::days(7))
        .await
        .unwrap();
    let a = akun(&db, "V-SARI").await;
    assert_eq!((a.saldo, a.utang_denda), (4_280, 0));
}

#[sqlx::test]
async fn denda_dipotong_dari_saldo_bila_cukup(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    sqlx::query("UPDATE vendor_akun SET saldo = 20000")
        .execute(&db)
        .await
        .unwrap();
    pesanan(
        &db,
        "T-2026-000001",
        "V-SARI",
        20_000,
        "dibatalkan_vendor",
        "2026-01-06",
    )
    .await;
    ledger::catat_denda(&db, "T-2026-000001", t0()).await.unwrap();
    let a = akun(&db, "V-SARI").await;
    assert_eq!((a.saldo, a.utang_denda), (5_000, 0));
}

#[sqlx::test]
async fn pencairan_dan_denda_idempoten_serta_syarat_status(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-06").await;
    pesanan(&db, "T-2026-000002", "V-SARI", 47_000, "disiapkan", "2026-01-06").await;
    pesanan(
        &db,
        "T-2026-000003",
        "V-SARI",
        47_000,
        "dibatalkan_vendor",
        "2026-01-06",
    )
    .await;
    assert!(ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap());
    assert!(!ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap());
    assert!(ledger::cairkan_pesanan(&db, "T-2026-000002", t0()).await.is_err()); // belum selesai
    assert!(ledger::catat_denda(&db, "T-2026-000001", t0()).await.is_err()); // bukan batal vendor
    assert!(ledger::catat_denda(&db, "T-2026-000003", t0()).await.unwrap());
    assert!(!ledger::catat_denda(&db, "T-2026-000003", t0()).await.unwrap());
    assert_eq!(akun(&db, "V-SARI").await.saldo, 0);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger_entri")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 2);
}

#[sqlx::test]
async fn job_cair_fee_hanya_pesanan_selesai_h_min_1(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-07").await; // H-1 -> cair
    pesanan(&db, "T-2026-000002", "V-SARI", 47_000, "selesai", "2026-01-08").await; // hari ini -> belum
    pesanan(&db, "T-2026-000003", "V-SARI", 47_000, "diserahkan", "2026-01-07").await; // belum selesai
    assert_eq!(ledger::cairkan_h_plus_1(&db, t0()).await.unwrap(), 1);
    assert_eq!(ledger::cairkan_h_plus_1(&db, t0()).await.unwrap(), 0);
    assert_eq!(akun(&db, "V-SARI").await.saldo, 9_640);
}

#[sqlx::test]
async fn deposit_10_persen_fee_pertama_sampai_genap(db: PgPool) {
    // Vendor baru: 10% fee (564) ditahan dari tiap pencairan sampai deposit Rp100.000.
    pesanan(&db, "T-2026-000001", "V-BARU", 47_000, "selesai", "2026-01-06").await;
    ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap();
    let a = akun(&db, "V-BARU").await;
    assert_eq!((a.deposit_terkumpul, a.saldo), (564, 9_640 - 564));
    // Mendekati genap: hanya menahan sisa yang kurang.
    sqlx::query("UPDATE vendor_akun SET deposit_terkumpul = 99800")
        .execute(&db)
        .await
        .unwrap();
    pesanan(&db, "T-2026-000002", "V-BARU", 47_000, "selesai", "2026-01-06").await;
    ledger::cairkan_pesanan(&db, "T-2026-000002", t0()).await.unwrap();
    let a2 = akun(&db, "V-BARU").await;
    assert_eq!(a2.deposit_terkumpul, 100_000);
    assert_eq!(a2.saldo, a.saldo + 9_640 - 200);
}

#[sqlx::test]
async fn penarikan_contoh_ledger_128400(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    sqlx::query("UPDATE vendor_akun SET saldo = 128400")
        .execute(&db)
        .await
        .unwrap();
    let h = ledger::tarik(&db, "V-SARI", 100_000, t0()).await.unwrap();
    assert_eq!((h.diterima, h.biaya_transfer, h.saldo), (97_500, 2_500, 28_400));
}

#[sqlx::test]
async fn penarikan_aturan(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    sqlx::query("UPDATE vendor_akun SET saldo = 80000")
        .execute(&db)
        .await
        .unwrap();
    // Di bawah minimum
    assert!(ledger::tarik(&db, "V-SARI", 49_999, t0()).await.is_err());
    // Melebihi saldo
    assert!(ledger::tarik(&db, "V-SARI", 80_001, t0()).await.is_err());
    // Sebelum 06.00 WIB (05.59 WIB = 22.59 UTC hari sebelumnya)
    let dini = Utc.with_ymd_and_hms(2026, 1, 7, 22, 59, 0).unwrap();
    assert!(ledger::tarik(&db, "V-SARI", 50_000, dini).await.is_err());
    // Tepat minimum pada 06.00 WIB (23.00 UTC) lolos
    let enam = Utc.with_ymd_and_hms(2026, 1, 7, 23, 0, 0).unwrap();
    let h = ledger::tarik(&db, "V-SARI", 50_000, enam).await.unwrap();
    assert_eq!((h.diterima, h.saldo), (47_500, 30_000));
}

#[sqlx::test]
async fn ledger_append_only_dan_saldo_tidak_negatif(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-06").await;
    ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap();
    assert!(sqlx::query("UPDATE ledger_entri SET jumlah = 1")
        .execute(&db)
        .await
        .is_err());
    assert!(sqlx::query("DELETE FROM ledger_entri").execute(&db).await.is_err());
    // Constraint DB: saldo negatif mustahil.
    assert!(sqlx::query("UPDATE vendor_akun SET saldo = -1")
        .execute(&db)
        .await
        .is_err());
}

#[sqlx::test]
async fn ongkir_5000_terbagi_dan_tidak_masuk_total_item(db: PgPool) {
    akun_deposit_genap(&db, "V-SARI", false).await;
    pesanan(&db, "T-2026-000001", "V-SARI", 47_000, "selesai", "2026-01-06").await;
    ledger::cairkan_pesanan(&db, "T-2026-000001", t0()).await.unwrap();
    let (ov, ok): (i64, i64) = sqlx::query_as("SELECT ongkir_vendor, ongkir_kas FROM ledger_entri")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!((ov, ok, ov + ok), (4_000, 1_000, 5_000));
    // Fee dihitung dari total_item (47.000), bukan total_bayar (52.000).
    let fee: i64 = sqlx::query_scalar("SELECT fee FROM ledger_entri")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(fee, 5_640);
}
