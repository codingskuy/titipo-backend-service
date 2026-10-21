mod common;

use axum::{body::Body, http::Request, Router};
use chrono::{DateTime, Duration, TimeZone, Utc};
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use titipo_backend_service::{config::Config, sinyal, AppState};

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 14, 5, 0, 0).unwrap()
}

fn app_pada(db: PgPool, waktu: DateTime<Utc>) -> Router {
    let (f, _) = fetcher_uji();
    let st = AppState::dengan_fetcher(db, Config::uji(), f).dengan_jam(Arc::new(move || waktu));
    titipo_backend_service::app(st)
}

fn kirim(kunci: Option<&str>, body: Value) -> Request<Body> {
    let mut r = Request::post("/sinyal/pedaree-stok").header("content-type", "application/json");
    if let Some(k) = kunci {
        r = r.header("X-Service-Key", k);
    }
    r.body(Body::from(body.to_string())).unwrap()
}

fn contoh() -> Value {
    json!({"jenis": "stok.menipis", "item_id": "ITM-CABAI-001", "tanggal": "2026-10-14", "item": "Cabai merah",
           "sisa": 150, "satuan": "gram", "kedaluwarsa": "2026-10-17", "recipe_id": null})
}

const KUNCI: &str = "kunci-pedaree-uji";

#[sqlx::test]
async fn sinyal_sah_dicatat_201_dan_duplikat_200(db: PgPool) {
    let app = app_pada(db.clone(), t0());
    let (s, b) = panggil(&app, kirim(Some(KUNCI), contoh())).await;
    assert_eq!((s, b["status"].as_str().unwrap()), (201, "dicatat_sebagai_anjuran"));
    // Duplikat item_id + tanggal: 200 duplikat_diabaikan (bukan galat), tanpa rekaman ganda.
    let mut dup = contoh();
    dup["sisa"] = json!(10); // isi lain boleh beda; kunci dedup item_id + tanggal
    let (s, b) = panggil(&app, kirim(Some(KUNCI), dup)).await;
    assert_eq!((s, b["status"].as_str().unwrap()), (200, "duplikat_diabaikan"));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sinyal_restock")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // Tanggal berbeda = sinyal baru.
    let mut besok = contoh();
    besok["tanggal"] = json!("2026-10-15");
    assert_eq!(panggil(&app, kirim(Some(KUNCI), besok)).await.0, 201);
}

#[sqlx::test]
async fn tiga_jenis_diterima_jenis_lain_422(db: PgPool) {
    let app = app_pada(db, t0());
    for (i, j) in ["stok.menipis", "stok.habis", "kedaluwarsa.dekat"]
        .into_iter()
        .enumerate()
    {
        let mut b = contoh();
        b["jenis"] = json!(j);
        b["item_id"] = json!(format!("ITM-{i}"));
        assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 201, "{j}");
    }
    let mut b = contoh();
    b["jenis"] = json!("stok.melimpah");
    let (s, r) = panggil(&app, kirim(Some(KUNCI), b)).await;
    assert_eq!((s, r["kode"].as_str().unwrap()), (422, "JENIS_SINYAL_TIDAK_DIKENAL"));
}

#[sqlx::test]
async fn validasi_payload(db: PgPool) {
    let app = app_pada(db, t0());
    // Satuan di luar 13 baku
    let mut b = contoh();
    b["satuan"] = json!("karung");
    let (s, r) = panggil(&app, kirim(Some(KUNCI), b)).await;
    assert_eq!((s, r["kode"].as_str().unwrap()), (422, "SATUAN_TIDAK_VALID"));
    // Semua 13 satuan baku lolos
    for (i, sat) in sinyal::SATUAN_BAKU.iter().enumerate() {
        let mut b = contoh();
        b["satuan"] = json!(sat);
        b["item_id"] = json!(format!("ITM-S{i}"));
        assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 201, "{sat}");
    }
    // Field wajib hilang / salah tipe / sisa negatif / tanggal rusak
    for hapus in ["item", "sisa", "satuan", "kedaluwarsa", "item_id", "tanggal"] {
        let mut b = contoh();
        b.as_object_mut().unwrap().remove(hapus);
        assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 422, "tanpa {hapus}");
    }
    let mut b = contoh();
    b["sisa"] = json!(-1);
    assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 422);
    let mut b = contoh();
    b["tanggal"] = json!("kemarin");
    assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 422);
    // recipe_id Pawonee boleh kosong atau hilang, dan diteruskan bila ada
    let mut b = contoh();
    b.as_object_mut().unwrap().remove("recipe_id");
    b["item_id"] = json!("ITM-R1");
    assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 201);
    let mut b = contoh();
    b["recipe_id"] = json!("RCP-0042");
    b["item_id"] = json!("ITM-R2");
    assert_eq!(panggil(&app, kirim(Some(KUNCI), b)).await.0, 201);
    // JSON rusak -> 400
    let r = Request::post("/sinyal/pedaree-stok")
        .header("X-Service-Key", KUNCI)
        .body(Body::from("{rusak"))
        .unwrap();
    assert_eq!(panggil(&app, r).await.0, 400);
}

#[sqlx::test]
async fn service_key_wajib_dan_hanya_milik_pedaree(db: PgPool) {
    let app = app_pada(db, t0());
    assert_eq!(panggil(&app, kirim(None, contoh())).await.0, 401);
    assert_eq!(panggil(&app, kirim(Some("salah"), contoh())).await.0, 403);
    // Kunci service TitipO->Lumbung dan JWT pengguna tidak berlaku di endpoint ini.
    assert_eq!(panggil(&app, kirim(Some("kunci-uji-utama"), contoh())).await.0, 403);
    let r = Request::post("/sinyal/pedaree-stok")
        .header("Authorization", format!("Bearer {}", token(|_| {})))
        .header("content-type", "application/json")
        .body(Body::from(contoh().to_string()))
        .unwrap();
    assert_eq!(panggil(&app, r).await.0, 401);
}

#[sqlx::test]
async fn sinyal_tidak_pernah_membuat_pesanan(db: PgPool) {
    let app = app_pada(db.clone(), t0());
    for (i, j) in ["stok.menipis", "stok.habis", "kedaluwarsa.dekat"]
        .into_iter()
        .enumerate()
    {
        let mut b = contoh();
        b["jenis"] = json!(j);
        b["item_id"] = json!(format!("ITM-{i}"));
        panggil(&app, kirim(Some(KUNCI), b.clone())).await;
        panggil(&app, kirim(Some(KUNCI), b)).await; // duplikat
    }
    let mut buruk = contoh();
    buruk["jenis"] = json!("tidak.dikenal");
    panggil(&app, kirim(Some(KUNCI), buruk)).await;

    for tabel in ["pesanan", "pesanan_item", "pesanan_audit", "pembayaran"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {tabel}"))
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(n, 0, "{tabel} harus tetap kosong");
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sinyal_restock")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 3);
}

#[sqlx::test]
async fn saran_relevan_48_jam(db: PgPool) {
    let app = app_pada(db.clone(), t0());
    panggil(&app, kirim(Some(KUNCI), contoh())).await;
    assert_eq!(
        sinyal::saran_aktif(&db, t0() + Duration::hours(47))
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        sinyal::saran_aktif(&db, t0() + Duration::hours(49))
            .await
            .unwrap()
            .len(),
        0
    );
    let s = &sinyal::saran_aktif(&db, t0()).await.unwrap()[0];
    assert_eq!(
        (s.item_id.as_str(), s.satuan.as_str(), s.sisa),
        ("ITM-CABAI-001", "gram", 150.0)
    );
}
