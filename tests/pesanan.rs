mod common;

use axum::{body::Body, http::Request, Router};
use chrono::{DateTime, Duration, TimeZone, Utc};
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use titipo_backend_service::{config::Config, pesanan::layanan, AppState};

fn t0() -> DateTime<Utc> {
    // 2026-01-06 12.00 WIB
    Utc.with_ymd_and_hms(2026, 1, 6, 5, 0, 0).unwrap()
}

fn app_pada(db: PgPool, waktu: DateTime<Utc>, ubah: impl FnOnce(&mut Config)) -> Router {
    let (f, _) = fetcher_uji();
    let mut cfg = Config::uji();
    ubah(&mut cfg);
    let st = AppState::dengan_fetcher(db, cfg, f).dengan_jam(Arc::new(move || waktu));
    titipo_backend_service::app(st)
}

fn app(db: PgPool) -> Router {
    app_pada(db, t0(), |_| {})
}

async fn seed_katalog(db: &PgPool, sinkron: DateTime<Utc>) {
    for v in ["V-SARI-001", "V-BUDI-002", "V-CADANGAN-003"] {
        for (sku, nama, harga) in [("LMB-BAYAM-001", "Bayam", 9000), ("LMB-TELUR-001", "Telur", 29000)] {
            sqlx::query("INSERT INTO katalog_cermin VALUES ($1,$2,$3,$4,100000,$5)")
                .bind(v)
                .bind(sku)
                .bind(nama)
                .bind(harga)
                .bind(sinkron)
                .execute(db)
                .await
                .unwrap();
        }
        // 60 SKU tambahan untuk uji batas baris.
        for i in 0..60 {
            sqlx::query("INSERT INTO katalog_cermin VALUES ($1,$2,'Item',1000,1000000,$3)")
                .bind(v)
                .bind(format!("LMB-X-{i:03}"))
                .bind(sinkron)
                .execute(db)
                .await
                .unwrap();
        }
    }
}

fn ulid(n: u32) -> String {
    format!("01JMB2Z9KQ0ABCD1234{n:07}")
}

fn tok_rt(sub: &str) -> String {
    token(|k| k["sub"] = json!(sub))
}
fn tok_vendor(v: &str) -> String {
    token(|k| {
        k["sub"] = json!("USR-V");
        k["role"] = json!("vendor");
        k["vendor_id"] = json!(v);
    })
}
fn tok_admin() -> String {
    token(|k| {
        k["sub"] = json!("admin@titipo.id");
        k["role"] = json!("admin");
    })
}

fn post(path: &str, tok: &str, idem: Option<&str>, body: Value) -> Request<Body> {
    let mut r = Request::post(path)
        .header("Authorization", format!("Bearer {tok}"))
        .header("content-type", "application/json");
    if let Some(i) = idem {
        r = r.header("Idempotency-Key", i);
    }
    r.body(Body::from(body.to_string())).unwrap()
}

fn pesanan_standar(id: u32) -> Value {
    json!({"id": ulid(id), "id_vendor": "V-SARI-001", "item": [
        {"sku_lumbung": "LMB-BAYAM-001", "jumlah_gram": 2000},
        {"sku_lumbung": "LMB-TELUR-001", "jumlah_gram": 1000}]})
}

async fn checkout(app: &Router, rt: &str, n: u32) -> String {
    let (s, b) = panggil(app, post("/pesanan", &tok_rt(rt), Some(&ulid(n)), pesanan_standar(n))).await;
    assert_eq!(s, 201, "{b}");
    b["kode"].as_str().unwrap().to_string()
}

async fn status_db(db: &PgPool, kode: &str) -> String {
    sqlx::query_scalar("SELECT status FROM pesanan WHERE kode=$1")
        .bind(kode)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn bayar(app: &Router, rt: &str, kode: &str, gw: &str) -> (u16, Value) {
    panggil(
        app,
        post(
            &format!("/pesanan/{kode}/bayar"),
            &tok_rt(rt),
            Some(&ulid(900)),
            json!({"id_gateway": gw}),
        ),
    )
    .await
}

async fn vendor_konfirmasi(app: &Router, v: &str, kode: &str, body: Value) -> (u16, Value) {
    panggil(
        app,
        post(
            &format!("/vendor/pesanan/{kode}/konfirmasi"),
            &tok_vendor(v),
            Some(&ulid(901)),
            body,
        ),
    )
    .await
}

// ---------------------------------------------------------------- checkout

#[sqlx::test]
async fn checkout_contoh_mengikat_52000(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let (s, b) = panggil(
        &app,
        post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), pesanan_standar(1)),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(b["kode"], "T-2026-000001");
    assert_eq!(b["total_bayar"], 52000);
    assert_eq!(b["kedaluwarsa_bayar"], "2026-01-06T05:15:00Z");

    let (ongkir, total_item, status): (i64, i64, String) =
        sqlx::query_as("SELECT ongkir, total_item, status FROM pesanan")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        (ongkir, total_item, status.as_str()),
        (5000, 47000, "menunggu_pembayaran")
    );
    let audit: (i64,) = sqlx::query_as("SELECT count(*) FROM pesanan_audit WHERE dari_status IS NULL")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(audit.0, 1);
}

#[sqlx::test]
async fn kirim_ulang_kunci_sama_200_tanpa_rekaman_ganda(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let (s1, b1) = panggil(
        &app,
        post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), pesanan_standar(1)),
    )
    .await;
    let (s2, b2) = panggil(
        &app,
        post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), pesanan_standar(1)),
    )
    .await;
    assert_eq!((s1, s2), (201, 200));
    assert_eq!(b1, b2);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM pesanan")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // Kunci sama, isi berbeda -> 409.
    let mut beda = pesanan_standar(1);
    beda["item"][0]["jumlah_gram"] = json!(3000);
    let (s3, b3) = panggil(&app, post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), beda)).await;
    assert_eq!(s3, 409);
    assert_eq!(b3["kode"], "IDEMPOTENCY_KEY_BENTROK");
}

#[sqlx::test]
async fn kirim_ulang_tetap_200_setelah_lewat_jam_tutup(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    checkout(&app(db.clone()), "RT-001", 1).await;
    let malam = Utc.with_ymd_and_hms(2026, 1, 6, 14, 0, 0).unwrap(); // 21.00 WIB
    let app2 = app_pada(db.clone(), malam, |_| {});
    let (s, _) = panggil(
        &app2,
        post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), pesanan_standar(1)),
    )
    .await;
    assert_eq!(s, 200);
}

#[sqlx::test]
async fn kode_berurutan_per_tahun(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    assert_eq!(checkout(&app, "RT-001", 1).await, "T-2026-000001");
    assert_eq!(checkout(&app, "RT-002", 2).await, "T-2026-000002");
}

#[sqlx::test]
async fn berat_melebihi_batas_422_dan_konfigurabel(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let besar = |gram: i64| json!({"id": ulid(5), "id_vendor": "V-SARI-001", "item": [{"sku_lumbung": "LMB-BAYAM-001", "jumlah_gram": gram}]});

    // Batas 20 kg (FSD/openapi): 20.001 g ditolak, 20.000 g lolos.
    let a20 = app_pada(db.clone(), t0(), |c| c.batas_berat_gram = 20_000);
    let (s, b) = panggil(&a20, post("/pesanan", &tok_rt("RT-001"), Some(&ulid(5)), besar(20_001))).await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (422, "BERAT_MELEBIHI_BATAS"));
    let (s, _) = panggil(&a20, post("/pesanan", &tok_rt("RT-001"), Some(&ulid(6)), besar(20_000))).await;
    assert_eq!(s, 201);

    // Bawaan 50 kg (AGENTS/KONTEKS): 50.000 g lolos, 50.001 g ditolak.
    let a50 = app(db.clone());
    let (s, _) = panggil(&a50, post("/pesanan", &tok_rt("RT-002"), Some(&ulid(7)), besar(50_000))).await;
    assert_eq!(s, 201);
    let (s, _) = panggil(&a50, post("/pesanan", &tok_rt("RT-002"), Some(&ulid(8)), besar(50_001))).await;
    assert_eq!(s, 422);
}

#[sqlx::test]
async fn lebih_dari_50_baris_422(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db);
    let item = |n: usize| {
        (0..n)
            .map(|i| json!({"sku_lumbung": format!("LMB-X-{i:03}"), "jumlah_gram": 1}))
            .collect::<Vec<_>>()
    };
    let (s, b) = panggil(
        &app,
        post(
            "/pesanan",
            &tok_rt("RT-001"),
            Some(&ulid(1)),
            json!({"id": ulid(1), "id_vendor": "V-SARI-001", "item": item(51)}),
        ),
    )
    .await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (422, "BARIS_MELEBIHI_BATAS"));
    let (s, _) = panggil(
        &app,
        post(
            "/pesanan",
            &tok_rt("RT-001"),
            Some(&ulid(2)),
            json!({"id": ulid(2), "id_vendor": "V-SARI-001", "item": item(50)}),
        ),
    )
    .await;
    assert_eq!(s, 201);
}

#[sqlx::test]
async fn katalog_lebih_24_jam_409(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(25)).await;
    let (s, b) = panggil(
        &app(db),
        post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), pesanan_standar(1)),
    )
    .await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (409, "KATALOG_KEDALUWARSA"));
}

#[sqlx::test]
async fn sku_dan_vendor_tak_dikenal_dan_stok(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let mut b = pesanan_standar(1);
    b["item"][0]["sku_lumbung"] = json!("LMB-GAIB-999");
    assert_eq!(
        panggil(&app, post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), b))
            .await
            .0,
        422
    );
    let mut b = pesanan_standar(2);
    b["id_vendor"] = json!("V-GAIB");
    assert_eq!(
        panggil(&app, post("/pesanan", &tok_rt("RT-001"), Some(&ulid(2)), b))
            .await
            .0,
        404
    );
    let mut b = pesanan_standar(3);
    sqlx::query("UPDATE katalog_cermin SET stok_gram = 500 WHERE sku_lumbung = 'LMB-BAYAM-001'")
        .execute(&db)
        .await
        .unwrap();
    b["item"][0]["jumlah_gram"] = json!(2000);
    let (s, r) = panggil(&app, post("/pesanan", &tok_rt("RT-001"), Some(&ulid(3)), b)).await;
    assert_eq!((s, r["kode"].as_str().unwrap()), (409, "STOK_HABIS"));
}

#[sqlx::test]
async fn lewat_20_00_wib_ditolak(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let tutup = Utc.with_ymd_and_hms(2026, 1, 6, 13, 0, 0).unwrap(); // 20.00 WIB
    let (s, b) = panggil(
        &app_pada(db, tutup, |_| {}),
        post("/pesanan", &tok_rt("RT-001"), Some(&ulid(1)), pesanan_standar(1)),
    )
    .await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (422, "DI_LUAR_JAM_PESAN"));
}

#[sqlx::test]
async fn permintaan_tak_valid_dan_peran(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db);
    // Tanpa / salah Idempotency-Key -> 400
    assert_eq!(
        panggil(&app, post("/pesanan", &tok_rt("RT-001"), None, pesanan_standar(1)))
            .await
            .0,
        400
    );
    assert_eq!(
        panggil(
            &app,
            post("/pesanan", &tok_rt("RT-001"), Some("bukan-ulid"), pesanan_standar(1))
        )
        .await
        .0,
        400
    );
    // JSON rusak -> 400
    let r = Request::post("/pesanan")
        .header("Authorization", format!("Bearer {}", tok_rt("RT-001")))
        .header("Idempotency-Key", ulid(1))
        .body(Body::from("{bukan json"))
        .unwrap();
    assert_eq!(panggil(&app, r).await.0, 400);
    // Vendor tidak boleh checkout; tanpa token 401
    assert_eq!(
        panggil(
            &app,
            post(
                "/pesanan",
                &tok_vendor("V-SARI-001"),
                Some(&ulid(1)),
                pesanan_standar(1)
            )
        )
        .await
        .0,
        403
    );
    let r = Request::post("/pesanan").body(Body::empty()).unwrap();
    assert_eq!(panggil(&app, r).await.0, 401);
}

// ---------------------------------------------------------------- mesin status

#[sqlx::test]
async fn alur_bahagia_dengan_audit_append_only(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let kode = checkout(&app, "RT-001", 1).await;

    let (s, b) = bayar(&app, "RT-001", &kode, "QR-67890").await;
    assert_eq!((s, b["status"].as_str().unwrap()), (200, "menunggu_konfirmasi"));
    // Kirim ulang bayar (sama id_gateway) -> 200 tanpa efek ganda
    assert_eq!(bayar(&app, "RT-001", &kode, "QR-67890").await.0, 200);
    // id_gateway lain -> 409
    assert_eq!(bayar(&app, "RT-001", &kode, "QR-LAIN").await.0, 409);

    let (s, b) = vendor_konfirmasi(&app, "V-SARI-001", &kode, json!({"terima": true})).await;
    assert_eq!((s, b["status"].as_str().unwrap()), (200, "disiapkan"));
    let (s, b2) = vendor_konfirmasi(&app, "V-SARI-001", &kode, json!({"terima": true})).await;
    assert_eq!(s, 200);
    assert_eq!(b["dikonfirmasi"], b2["dikonfirmasi"]);

    let audit: Vec<(Option<String>, String, String)> =
        sqlx::query_as("SELECT dari_status, ke_status, aktor FROM pesanan_audit ORDER BY id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        audit,
        vec![
            (None, "menunggu_pembayaran".into(), "RT-001".into()),
            (
                Some("menunggu_pembayaran".into()),
                "menunggu_konfirmasi".into(),
                "RT-001".into()
            ),
            (
                Some("menunggu_konfirmasi".into()),
                "disiapkan".into(),
                "vendor:V-SARI-001".into()
            ),
        ]
    );
    // Append-only: UPDATE dan DELETE ditolak trigger.
    assert!(sqlx::query("UPDATE pesanan_audit SET aktor='x'")
        .execute(&db)
        .await
        .is_err());
    assert!(sqlx::query("DELETE FROM pesanan_audit").execute(&db).await.is_err());
}

#[sqlx::test]
async fn transisi_ilegal_422(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let kode = checkout(&app, "RT-001", 1).await;
    // Vendor mengonfirmasi sebelum dibayar -> 422
    let (s, b) = vendor_konfirmasi(&app, "V-SARI-001", &kode, json!({"terima": true})).await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (422, "TRANSISI_STATUS_ILEGAL"));
    assert_eq!(status_db(&db, &kode).await, "menunggu_pembayaran");
    // Bayar lewat batas 15 menit -> 422
    let telat = app_pada(db.clone(), t0() + Duration::minutes(16), |_| {});
    let (s, b) = bayar(&telat, "RT-001", &kode, "QR-1").await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (422, "PEMBAYARAN_KEDALUWARSA"));
}

#[sqlx::test]
async fn kepemilikan_pesanan_ditegakkan(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let kode = checkout(&app, "RT-001", 1).await;
    assert_eq!(bayar(&app, "RT-LAIN", &kode, "QR-1").await.0, 404);
    bayar(&app, "RT-001", &kode, "QR-1").await;
    // Vendor lain tidak bisa mengonfirmasi
    assert_eq!(
        vendor_konfirmasi(&app, "V-BUDI-002", &kode, json!({"terima": true}))
            .await
            .0,
        404
    );
    // Rumah tangga tidak bisa konfirmasi sebagai vendor
    let (s, _) = panggil(
        &app,
        post(
            &format!("/vendor/pesanan/{kode}/konfirmasi"),
            &tok_rt("RT-001"),
            Some(&ulid(1)),
            json!({"terima": true}),
        ),
    )
    .await;
    assert_eq!(s, 403);
}

#[sqlx::test]
async fn pengalihan_maks_2_kali_lalu_refund_penuh_h_plus_1(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let kode = checkout(&app, "RT-001", 1).await;
    bayar(&app, "RT-001", &kode, "QR-1").await;
    let alih = |v: &'static str| {
        post(
            &format!("/admin/pesanan/{kode}/alihkan"),
            &tok_admin(),
            None,
            json!({"id_vendor_cadangan": v}),
        )
    };

    // Vendor 1 menolak -> dialihkan
    let (_, b) = vendor_konfirmasi(
        &app,
        "V-SARI-001",
        &kode,
        json!({"terima": false, "alasan": "Stok habis"}),
    )
    .await;
    assert_eq!(b["status"], "dialihkan");
    // Penolakan tanpa alasan -> 422
    assert_eq!(
        vendor_konfirmasi(&app, "V-SARI-001", &kode, json!({"terima": false}))
            .await
            .0,
        422
    );

    // Alih ke-1
    assert_eq!(panggil(&app, alih("V-BUDI-002")).await.0, 200);
    assert_eq!(status_db(&db, &kode).await, "menunggu_konfirmasi");
    // Vendor 2 menolak -> dialihkan; alih ke-2
    vendor_konfirmasi(&app, "V-BUDI-002", &kode, json!({"terima": false, "alasan": "Libur"})).await;
    assert_eq!(panggil(&app, alih("V-CADANGAN-003")).await.0, 200);
    // Alih ke-3 ditolak 422
    let (s, b) = panggil(&app, alih("V-SARI-001")).await;
    assert_eq!((s, b["kode"].as_str().unwrap()), (422, "ALIH_MELEBIHI_BATAS"));
    // Vendor 3 menolak -> batal sistem + refund penuh H+1
    let (_, b) = vendor_konfirmasi(
        &app,
        "V-CADANGAN-003",
        &kode,
        json!({"terima": false, "alasan": "Tutup"}),
    )
    .await;
    assert_eq!(b["status"], "dibatalkan_sistem");
    let (jml, refund): (i16, Option<chrono::NaiveDate>) =
        sqlx::query_as("SELECT jumlah_alih, refund_jatuh_tempo FROM pesanan WHERE kode=$1")
            .bind(&kode)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(jml, 2);
    assert_eq!(refund.unwrap().to_string(), "2026-01-07");
    // Audit mencatat tiap transisi
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM pesanan_audit")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 7); // dibuat, bayar, dialih, menunggu, dialih, menunggu, batal
}

#[sqlx::test]
async fn alihkan_hanya_admin(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db);
    let kode = checkout(&app, "RT-001", 1).await;
    bayar(&app, "RT-001", &kode, "QR-1").await;
    let (s, _) = panggil(
        &app,
        post(
            &format!("/admin/pesanan/{kode}/alihkan"),
            &tok_rt("RT-001"),
            None,
            json!({"id_vendor_cadangan": "V-BUDI-002"}),
        ),
    )
    .await;
    assert_eq!(s, 403);
}

// ---------------------------------------------------------------- pekerjaan berkala

#[sqlx::test]
async fn job_timeout_konfirmasi_mengalihkan_lalu_membatalkan(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let kode = checkout(&app, "RT-001", 1).await;
    bayar(&app, "RT-001", &kode, "QR-1").await;

    // Belum 30 menit: tidak diproses.
    assert_eq!(
        layanan::tangani_timeout_konfirmasi(&db, t0() + Duration::minutes(29))
            .await
            .unwrap(),
        0
    );
    // Lewat 30 menit: dialihkan.
    assert_eq!(
        layanan::tangani_timeout_konfirmasi(&db, t0() + Duration::minutes(31))
            .await
            .unwrap(),
        1
    );
    assert_eq!(status_db(&db, &kode).await, "dialihkan");

    // Dua pengalihan selesai lalu timeout lagi -> batal sistem + refund H+1.
    sqlx::query("UPDATE pesanan SET status='menunggu_konfirmasi', jumlah_alih=2, batas_konfirmasi=$1")
        .bind(t0())
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        layanan::tangani_timeout_konfirmasi(&db, t0() + Duration::minutes(40))
            .await
            .unwrap(),
        1
    );
    assert_eq!(status_db(&db, &kode).await, "dibatalkan_sistem");
    let refund: Option<chrono::NaiveDate> = sqlx::query_scalar("SELECT refund_jatuh_tempo FROM pesanan")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(refund.unwrap().to_string(), "2026-01-07");
}

#[sqlx::test]
async fn job_kedaluwarsa_belum_bayar(db: PgPool) {
    seed_katalog(&db, t0() - Duration::hours(1)).await;
    let app = app(db.clone());
    let kode = checkout(&app, "RT-001", 1).await;
    assert_eq!(
        layanan::kedaluwarsakan_belum_bayar(&db, t0() + Duration::minutes(14))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        layanan::kedaluwarsakan_belum_bayar(&db, t0() + Duration::minutes(15))
            .await
            .unwrap(),
        1
    );
    assert_eq!(status_db(&db, &kode).await, "kedaluwarsa");
}
