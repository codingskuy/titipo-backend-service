//! Konfigurasi dari variabel lingkungan (lihat docs/auth.md §3).

use chrono::{DateTime, Utc};
use std::time::Duration;

/// Satu service key server-ke-server. Kunci lama punya batas akhir (jendela rotasi 7 hari).
#[derive(Debug, Clone)]
pub struct KunciLayanan {
    pub nilai: String,
    pub berlaku_sampai: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwks_url: String,
    pub issuer: String,
    pub audiens: String,
    pub jwks_ttl: Duration,
    pub toleransi_jam: u64,
    pub kunci_layanan: Vec<KunciLayanan>,
    pub lumbung_base_url: String,
    pub alamat: String,
    /// Batas berat total per pesanan (gram); di atasnya 422. KONFIGURABEL karena dokumen bertentangan
    /// (FSD/openapi: 20 kg; AGENTS/KONTEKS: 50 kg). Bawaan mengikuti AGENTS/KONTEKS.
    pub batas_berat_gram: i64,
}

fn env_atau(nama: &str, bawaan: &str) -> String {
    std::env::var(nama).unwrap_or_else(|_| bawaan.to_string())
}

impl Config {
    pub fn dari_env() -> Result<Self, String> {
        let database_url = std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL wajib diisi".to_string())?;
        let utama = std::env::var("TITIPO_SERVICE_KEY").map_err(|_| "TITIPO_SERVICE_KEY wajib diisi".to_string())?;
        let mut kunci = vec![KunciLayanan {
            nilai: utama,
            berlaku_sampai: None,
        }];
        // Rotasi 90 hari: kunci lama tetap aktif sampai batas (maks 7 hari tumpang-tindih).
        if let Ok(lama) = std::env::var("TITIPO_SERVICE_KEY_PREV") {
            let sampai = std::env::var("TITIPO_SERVICE_KEY_PREV_SAMPAI").map_err(|_| {
                "TITIPO_SERVICE_KEY_PREV_SAMPAI (RFC 3339) wajib bila TITIPO_SERVICE_KEY_PREV diisi".to_string()
            })?;
            let sampai = DateTime::parse_from_rfc3339(&sampai)
                .map_err(|e| format!("TITIPO_SERVICE_KEY_PREV_SAMPAI tidak valid: {e}"))?
                .with_timezone(&Utc);
            kunci.push(KunciLayanan {
                nilai: lama,
                berlaku_sampai: Some(sampai),
            });
        }
        Ok(Self {
            database_url,
            jwks_url: env_atau("LUMBUNG_JWKS_URL", "https://auth.lumbung.skuy/.well-known/jwks.json"),
            issuer: env_atau("LUMBUNG_ISSUER", "https://auth.lumbung.skuy"),
            audiens: env_atau("TITIPO_JWT_AUD", "titipo"),
            jwks_ttl: Duration::from_secs(600),
            toleransi_jam: 60,
            kunci_layanan: kunci,
            lumbung_base_url: env_atau("LUMBUNG_BASE_URL", "https://api.lumbung.skuy"),
            alamat: env_atau("TITIPO_BIND", "0.0.0.0:8080"),
            batas_berat_gram: env_atau("BATAS_BERAT_GRAM", "50000")
                .parse()
                .map_err(|_| "BATAS_BERAT_GRAM harus bilangan bulat (gram)".to_string())?,
        })
    }

    /// Konfigurasi tetap untuk uji.
    pub fn uji() -> Self {
        Self {
            database_url: String::new(),
            jwks_url: "http://jwks.uji.invalid/jwks.json".into(),
            issuer: "https://auth.lumbung.skuy".into(),
            audiens: "titipo".into(),
            jwks_ttl: Duration::from_secs(600),
            toleransi_jam: 60,
            kunci_layanan: vec![KunciLayanan {
                nilai: "kunci-uji-utama".into(),
                berlaku_sampai: None,
            }],
            lumbung_base_url: "http://lumbung.uji.invalid".into(),
            alamat: "127.0.0.1:0".into(),
            batas_berat_gram: 50_000,
        }
    }
}
