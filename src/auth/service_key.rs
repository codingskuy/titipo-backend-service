//! Service key server-ke-server (`X-Titipo-Service-Key`). Tanpa kunci = 401, kunci salah = 403.
//! Rotasi 90 hari: kunci lama tetap sah sampai `berlaku_sampai` (maks 7 hari tumpang-tindih).

use crate::{config::KunciLayanan, error::AppError};
use chrono::{DateTime, Utc};

pub const HEADER_SERVICE_KEY: &str = "X-Titipo-Service-Key";

#[derive(Debug, Clone)]
pub struct Keyring {
    kunci: Vec<KunciLayanan>,
}

/// Perbandingan waktu-konstan (panjang sama) agar tidak bocor lewat timing.
fn sama_konstan(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Keyring {
    pub fn baru(kunci: Vec<KunciLayanan>) -> Self {
        Self { kunci }
    }

    pub fn verifikasi(&self, disajikan: Option<&str>, sekarang: DateTime<Utc>) -> Result<(), AppError> {
        let Some(disajikan) = disajikan.filter(|s| !s.is_empty()) else {
            return Err(AppError::tidak_terotorisasi(
                "SERVICE_KEY_TIDAK_ADA",
                "Header X-Titipo-Service-Key wajib",
            ));
        };
        // Periksa semua kunci tanpa berhenti dini.
        let mut cocok = false;
        for k in &self.kunci {
            let aktif = k.berlaku_sampai.map(|t| sekarang < t).unwrap_or(true);
            if sama_konstan(k.nilai.as_bytes(), disajikan.as_bytes()) && aktif {
                cocok = true;
            }
        }
        if cocok {
            Ok(())
        } else {
            Err(AppError::dilarang("SERVICE_KEY_SALAH", "Service key tidak sah"))
        }
    }
}
