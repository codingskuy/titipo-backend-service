mod common;

use axum::{body::Body, http::Request, routing::get, Router};
use chrono::{Duration as Dur, Utc};
use common::*;
use serde_json::json;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use titipo_backend_service::{
    auth::{jwks::JwksCache, service_key::Keyring, AuthUser, LayananSah, Role, Skup},
    config::KunciLayanan,
    pesanan_repo, AppState,
};

/// Router uji dengan rute yang memakai extractor auth.
fn router_uji(state: AppState) -> Router {
    async fn saya(u: AuthUser) -> String {
        u.sub
    }
    async fn khusus_admin(u: AuthUser) -> Result<&'static str, titipo_backend_service::error::AppError> {
        u.wajib_peran(&[Role::Admin])?;
        Ok("ok")
    }
    async fn layanan(_: LayananSah) -> &'static str {
        "ok"
    }
    Router::new()
        .route("/saya", get(saya))
        .route("/admin", get(khusus_admin))
        .route("/layanan", get(layanan))
        .with_state(state)
}

// ---------- JWT ----------

#[tokio::test]
async fn token_sah_diterima() {
    let app = router_uji(state_tanpa_db());
    let (s, _) = panggil(&app, get_bearer("/saya", &token(|_| {}))).await;
    assert_eq!(s, 200);
}

#[tokio::test]
async fn tanpa_token_401() {
    let app = router_uji(state_tanpa_db());
    let (s, b) = panggil(&app, Request::get("/saya").body(Body::empty()).unwrap()).await;
    assert_eq!(s, 401);
    assert_eq!(b["kode"], "TOKEN_TIDAK_ADA");
}

#[tokio::test]
async fn audiens_selain_titipo_403() {
    let app = router_uji(state_tanpa_db());
    let t = token(|k| k["aud"] = json!("lumbung"));
    let (s, b) = panggil(&app, get_bearer("/saya", &t)).await;
    assert_eq!(s, 403);
    assert_eq!(b["kode"], "AUDIENS_SALAH");
}

#[tokio::test]
async fn token_kedaluwarsa_401() {
    let app = router_uji(state_tanpa_db());
    // Lewat dari toleransi 60 detik.
    let t = token(|k| k["exp"] = json!(unix_sekarang() - 120));
    let (s, b) = panggil(&app, get_bearer("/saya", &t)).await;
    assert_eq!(s, 401);
    assert_eq!(b["kode"], "TOKEN_KEDALUWARSA");
}

#[tokio::test]
async fn toleransi_clock_skew_60_detik() {
    let app = router_uji(state_tanpa_db());
    // Baru kedaluwarsa 30 detik lalu: masih dalam toleransi.
    let t = token(|k| k["exp"] = json!(unix_sekarang() - 30));
    let (s, _) = panggil(&app, get_bearer("/saya", &t)).await;
    assert_eq!(s, 200);
}

#[tokio::test]
async fn issuer_salah_401() {
    let app = router_uji(state_tanpa_db());
    let t = token(|k| k["iss"] = json!("https://penipu.example"));
    let (s, _) = panggil(&app, get_bearer("/saya", &t)).await;
    assert_eq!(s, 401);
}

#[tokio::test]
async fn token_acak_dan_algoritma_hs256_ditolak() {
    let app = router_uji(state_tanpa_db());
    let (s, _) = panggil(&app, get_bearer("/saya", "bukan.jwt.sama-sekali")).await;
    assert_eq!(s, 401);
    // HS256 (serangan confusion) ditolak walau struktur valid.
    let hs = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &json!({"sub":"x","role":"admin","aud":"titipo","iss":"https://auth.lumbung.skuy","exp": unix_sekarang()+900}),
        &jsonwebtoken::EncodingKey::from_secret(b"rahasia"),
    )
    .unwrap();
    let (s, _) = panggil(&app, get_bearer("/saya", &hs)).await;
    assert_eq!(s, 401);
}

#[tokio::test]
async fn peran_tak_dikenal_ditolak() {
    let app = router_uji(state_tanpa_db());
    let t = token(|k| k["role"] = json!("superuser"));
    let (s, _) = panggil(&app, get_bearer("/saya", &t)).await;
    assert_eq!(s, 401);
}

#[tokio::test]
async fn penegakan_peran_admin() {
    let app = router_uji(state_tanpa_db());
    let (s, b) = panggil(&app, get_bearer("/admin", &token(|_| {}))).await;
    assert_eq!(s, 403);
    assert_eq!(b["kode"], "PERAN_DITOLAK");
    let (s, _) = panggil(&app, get_bearer("/admin", &token(|k| k["role"] = json!("admin")))).await;
    assert_eq!(s, 200);
}

#[tokio::test]
async fn rute_contoh_nyata_butuh_token() {
    let app = titipo_backend_service::app(state_tanpa_db());
    let (s, _) = panggil(&app, Request::get("/v1/contoh").body(Body::empty()).unwrap()).await;
    assert_eq!(s, 401);
    let (s, b) = panggil(&app, get_bearer("/v1/contoh", &token(|_| {}))).await;
    assert_eq!(s, 200);
    assert_eq!(b["sub"], "RT-001");
    // healthz tetap bebas.
    let (s, _) = panggil(&app, Request::get("/healthz").body(Body::empty()).unwrap()).await;
    assert_eq!(s, 200);
}

// ---------- Cache JWKS ----------

#[tokio::test]
async fn jwks_di_cache_selama_ttl() {
    let (f, n) = fetcher_uji();
    let cache = JwksCache::baru(f, Duration::from_secs(600));
    for _ in 0..5 {
        assert!(cache.kunci(Some("uji-1")).await.unwrap().is_some());
    }
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn jwks_diambil_ulang_setelah_ttl() {
    let (f, n) = fetcher_uji();
    let cache = JwksCache::baru(f, Duration::from_millis(0));
    cache.kunci(Some("uji-1")).await.unwrap();
    cache.kunci(Some("uji-1")).await.unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn kid_tak_dikenal_refresh_dibatasi() {
    let (f, n) = fetcher_uji();
    let cache = JwksCache::baru(f, Duration::from_secs(600)); // refresh minimum 30 detik
    assert!(cache.kunci(Some("acak-1")).await.unwrap().is_none()); // muat awal
    assert!(cache.kunci(Some("acak-2")).await.unwrap().is_none()); // dibatasi, tidak fetch
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn kid_tak_dikenal_refresh_bila_lewat_interval() {
    let (f, n) = fetcher_uji();
    let cache = JwksCache::baru(f, Duration::from_secs(600)).dengan_refresh_minimum(Duration::from_millis(0));
    cache.kunci(Some("acak-1")).await.unwrap();
    cache.kunci(Some("acak-2")).await.unwrap();
    assert_eq!(n.load(Ordering::SeqCst), 2);
}

// ---------- Service key ----------

fn req_layanan(kunci: Option<&str>) -> Request<Body> {
    let mut r = Request::get("/layanan");
    if let Some(k) = kunci {
        r = r.header("X-Titipo-Service-Key", k);
    }
    r.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn service_key_tanpa_401_salah_403_benar_200() {
    let app = router_uji(state_tanpa_db());
    assert_eq!(panggil(&app, req_layanan(None)).await.0, 401);
    assert_eq!(panggil(&app, req_layanan(Some("salah"))).await.0, 403);
    assert_eq!(panggil(&app, req_layanan(Some("kunci-uji-utama"))).await.0, 200);
}

#[test]
fn rotasi_dua_kunci_aktif_lalu_kunci_lama_gugur() {
    let sekarang = Utc::now();
    let ring = Keyring::baru(vec![
        KunciLayanan {
            nilai: "baru".into(),
            berlaku_sampai: None,
        },
        KunciLayanan {
            nilai: "lama".into(),
            berlaku_sampai: Some(sekarang + Dur::days(7)),
        },
    ]);
    // Jendela tumpang-tindih: keduanya sah.
    assert!(ring.verifikasi(Some("baru"), sekarang).is_ok());
    assert!(ring.verifikasi(Some("lama"), sekarang).is_ok());
    // Setelah 7 hari: kunci lama ditolak, kunci baru tetap sah.
    let kelak = sekarang + Dur::days(7) + Dur::seconds(1);
    assert!(ring.verifikasi(Some("lama"), kelak).is_err());
    assert!(ring.verifikasi(Some("baru"), kelak).is_ok());
}

// ---------- Peran vs data (sqlx::test) ----------

async fn isi_pesanan(db: &PgPool) {
    for (i, (rt, vendor)) in [
        ("RT-001", "V-SARI"),
        ("RT-001", "V-BUDI"),
        ("RT-002", "V-SARI"),
        ("RT-003", "V-BUDI"),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO pesanan (kode, id_klien, idempotency_key, request_hash, id_rumah_tangga, id_vendor, status,
                 total_item, ongkir, total_bayar, berat_gram, jadwal_serah, kedaluwarsa_bayar)
             VALUES ($1, 'K', $1, 'h', $2, $3, 'menunggu_pembayaran', 1000, 5000, 6000, 100, CURRENT_DATE + 1, now())",
        )
        .bind(format!("T-2026-{:06}", i + 1))
        .bind(rt)
        .bind(vendor)
        .execute(db)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn rumah_tangga_hanya_miliknya(db: PgPool) {
    isi_pesanan(&db).await;
    let u = AuthUser {
        sub: "RT-001".into(),
        role: Role::Household,
        vendor_id: None,
    };
    assert_eq!(u.skup(), Skup::RumahTangga("RT-001".into()));
    assert_eq!(pesanan_repo::id_terlihat(&db, &u.skup()).await.unwrap().len(), 2);
}

#[sqlx::test]
async fn vendor_hanya_lapaknya(db: PgPool) {
    isi_pesanan(&db).await;
    let u = AuthUser {
        sub: "USR-9".into(),
        role: Role::Vendor,
        vendor_id: Some("V-SARI".into()),
    };
    assert_eq!(pesanan_repo::id_terlihat(&db, &u.skup()).await.unwrap().len(), 2);
    // Tanpa klaim vendor_id, sub dipakai sebagai id vendor.
    let u2 = AuthUser {
        sub: "V-BUDI".into(),
        role: Role::Vendor,
        vendor_id: None,
    };
    assert_eq!(pesanan_repo::id_terlihat(&db, &u2.skup()).await.unwrap().len(), 2);
}

#[sqlx::test]
async fn admin_dan_operasi_melihat_semua(db: PgPool) {
    isi_pesanan(&db).await;
    for role in [Role::Admin, Role::Operasi] {
        let u = AuthUser {
            sub: "ADM".into(),
            role,
            vendor_id: None,
        };
        assert_eq!(pesanan_repo::id_terlihat(&db, &u.skup()).await.unwrap().len(), 4);
    }
}
