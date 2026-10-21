//! Pesanan: mesin status, checkout idempoten, transisi.

pub mod handlers;
pub mod layanan;
pub mod status;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Timelike, Utc};

pub const ONGKIR: i64 = 5_000;
pub const KEDALUWARSA_BAYAR_MENIT: i64 = 15;
pub const BATAS_KONFIRMASI_MENIT: i64 = 30;
pub const MAKS_BARIS: usize = 50;
pub const MAKS_ALIH: i16 = 2;
pub const UMUR_MAKS_KATALOG_JAM: i64 = 24;
/// Di atas ambang ini pesanan ditandai grosir (AGENTS/KONTEKS: 50 kg per transaksi).
pub const AMBANG_GROSIR_GRAM: i64 = 50_000;
/// Pesan hanya sampai pukul 20.00 WIB.
pub const JAM_TUTUP_PESAN: u32 = 20;

pub fn wib() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).expect("offset WIB")
}

pub fn tanggal_wib(t: DateTime<Utc>) -> NaiveDate {
    t.with_timezone(&wib()).date_naive()
}

pub fn lewat_jam_tutup(t: DateTime<Utc>) -> bool {
    t.with_timezone(&wib()).hour() >= JAM_TUTUP_PESAN
}

/// Serah selalu besok (tidak ada same-day).
pub fn jadwal_serah(t: DateTime<Utc>) -> NaiveDate {
    tanggal_wib(t) + Duration::days(1)
}

pub fn tahun_wib(t: DateTime<Utc>) -> i32 {
    t.with_timezone(&wib()).year()
}

/// Kode `T-YYYY-NNNNNN`.
pub fn format_kode(tahun: i32, urutan: i64) -> String {
    format!("T-{tahun:04}-{urutan:06}")
}

pub fn kode_valid(k: &str) -> bool {
    let b = k.as_bytes();
    b.len() == 13
        && &k[..2] == "T-"
        && b[6] == b'-'
        && b[2..6].iter().all(u8::is_ascii_digit)
        && b[7..].iter().all(u8::is_ascii_digit)
}

/// ULID 26 karakter Crockford base32 (tanpa I, L, O, U).
pub fn ulid_valid(s: &str) -> bool {
    s.len() == 26
        && s.bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z'))
}

/// Subtotal = gram × harga/kg ÷ 1000, dibulatkan ke rupiah terdekat (integer).
pub fn subtotal(jumlah_gram: i64, harga_per_kg: i64) -> i64 {
    (jumlah_gram * harga_per_kg + 500) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn kode_dan_ulid() {
        assert_eq!(format_kode(2026, 112), "T-2026-000112");
        assert!(kode_valid("T-2026-000112"));
        assert!(!kode_valid("T-26-000112"));
        assert!(!kode_valid("X-2026-000112"));
        assert!(!kode_valid("T-2026-00011a"));
        assert!(ulid_valid("01JMB2Z9KQ0ABCD1234567890A"));
        assert!(!ulid_valid("01JMB2Z9KQ0ABCD1234567890")); // 25 karakter
        assert!(!ulid_valid("01JMB2Z9KQ0ABCD1234567890U")); // U tidak ada di Crockford
        assert!(!ulid_valid("01jmb2z9kq0abcd1234567890a")); // huruf kecil
    }

    #[test]
    fn subtotal_dan_contoh_mengikat() {
        // Contoh mengikat: bayam 2 kg @9.000 + telur 1 kg @29.000 = 47.000; + ongkir = 52.000
        assert_eq!(subtotal(2000, 9000) + subtotal(1000, 29000) + ONGKIR, 52_000);
        assert_eq!(subtotal(333, 1000), 333);
        assert_eq!(subtotal(1, 1000), 1);
    }

    #[test]
    fn jam_tutup_20_wib() {
        // 19.59 WIB = 12.59 UTC; 20.00 WIB = 13.00 UTC
        assert!(!lewat_jam_tutup(Utc.with_ymd_and_hms(2026, 1, 6, 12, 59, 0).unwrap()));
        assert!(lewat_jam_tutup(Utc.with_ymd_and_hms(2026, 1, 6, 13, 0, 0).unwrap()));
        // 22.00 UTC = 05.00 WIB hari berikutnya: buka lagi
        assert!(!lewat_jam_tutup(Utc.with_ymd_and_hms(2026, 1, 6, 22, 0, 0).unwrap()));
        assert_eq!(
            jadwal_serah(Utc.with_ymd_and_hms(2026, 1, 6, 12, 0, 0).unwrap()).to_string(),
            "2026-01-07"
        );
    }
}
