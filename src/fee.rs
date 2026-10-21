//! Hitungan fee vendor (BRD/30-keuangan-fee, keuangan/10-bagi-fee §1-2). Murni, integer rupiah.

/// Fee vendor 12% dari total item.
pub const FEE_PERSEN: i64 = 12;
/// Bonus vendor Prioritas 0,5% (per mil = 5).
pub const BONUS_PRIORITAS_PER_MIL: i64 = 5;
pub const ONGKIR_VENDOR: i64 = 4_000;
pub const ONGKIR_KAS: i64 = 1_000;
pub const DENDA_BATAL_SEPIHAK: i64 = 15_000;
pub const PENARIKAN_MINIMAL: i64 = 50_000;
pub const BIAYA_TRANSFER: i64 = 2_500;
pub const DEPOSIT_TARGET: i64 = 100_000;
/// Deposit diakumulasi dari 10% fee pertama.
pub const DEPOSIT_PERSEN_FEE: i64 = 10;

/// Pembulatan setengah ke atas pada rupiah penuh: `pembilang / penyebut`.
fn bagi_bulat(pembilang: i64, penyebut: i64) -> i64 {
    (pembilang * 2 + penyebut) / (penyebut * 2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BagiHasil {
    pub fee: i64,
    pub bonus_prioritas: i64,
    pub ongkir_vendor: i64,
    pub ongkir_kas: i64,
    /// fee + bonus + ongkir vendor (sebelum denda).
    pub diterima_vendor: i64,
}

pub fn hitung_bagi_hasil(total_item: i64, prioritas: bool) -> BagiHasil {
    let fee = bagi_bulat(total_item * FEE_PERSEN, 100);
    let bonus_prioritas = if prioritas {
        bagi_bulat(total_item * BONUS_PRIORITAS_PER_MIL, 1000)
    } else {
        0
    };
    BagiHasil {
        fee,
        bonus_prioritas,
        ongkir_vendor: ONGKIR_VENDOR,
        ongkir_kas: ONGKIR_KAS,
        diterima_vendor: fee + bonus_prioritas + ONGKIR_VENDOR,
    }
}

/// Hasil bersih sebuah pesanan setelah denda; boleh negatif (Kasus C), yang artinya
/// pemotongan terhadap saldo.
pub fn bersih_setelah_denda(total_item: i64, prioritas: bool, denda: i64) -> i64 {
    hitung_bagi_hasil(total_item, prioritas).diterima_vendor - denda
}

/// Kredit ke saldo: utang denda dilunasi lebih dulu. Mengembalikan (saldo, utang).
pub fn terapkan_kredit(saldo: i64, utang: i64, kredit: i64) -> (i64, i64) {
    let lunas = kredit.min(utang);
    (saldo + kredit - lunas, utang - lunas)
}

/// Debit denda: dipotong dari saldo; saldo tidak boleh negatif, sisanya jadi utang
/// yang dibawa ke minggu depan. Mengembalikan (saldo, utang).
pub fn terapkan_denda(saldo: i64, utang: i64, denda: i64) -> (i64, i64) {
    let potong = saldo.min(denda);
    (saldo - potong, utang + denda - potong)
}

/// Bagian fee yang ditahan untuk deposit (10% fee) sampai total deposit genap Rp100.000.
pub fn tahan_deposit(fee: i64, deposit_terkumpul: i64) -> i64 {
    bagi_bulat(fee * DEPOSIT_PERSEN_FEE, 100)
        .min(DEPOSIT_TARGET - deposit_terkumpul)
        .max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tiga kasus mengikat (keuangan/10-bagi-fee §2).
    #[test]
    fn kasus_a_vendor_aktif_tanpa_denda() {
        let b = hitung_bagi_hasil(47_000, false);
        assert_eq!((b.fee, b.bonus_prioritas, b.ongkir_vendor), (5_640, 0, 4_000));
        assert_eq!(b.diterima_vendor, 9_640);
    }

    #[test]
    fn kasus_b_vendor_prioritas() {
        let b = hitung_bagi_hasil(47_000, true);
        assert_eq!((b.fee, b.bonus_prioritas), (5_640, 235));
        assert_eq!(b.fee + b.bonus_prioritas, 5_875);
        assert_eq!(b.diterima_vendor, 9_875);
    }

    #[test]
    fn kasus_c_batal_sepihak_minus_5360() {
        assert_eq!(bersih_setelah_denda(47_000, false, DENDA_BATAL_SEPIHAK), -5_360);
    }

    #[test]
    fn ongkir_terbagi_5000_dan_di_luar_total_item() {
        let b = hitung_bagi_hasil(47_000, false);
        assert_eq!(b.ongkir_vendor + b.ongkir_kas, 5_000);
    }

    #[test]
    fn pembulatan_rupiah_penuh_setengah_ke_atas() {
        assert_eq!(hitung_bagi_hasil(1_000, false).fee, 120);
        assert_eq!(hitung_bagi_hasil(1_005, false).fee, 121); // 120,6 -> 121
        assert_eq!(hitung_bagi_hasil(1_004, false).fee, 120); // 120,48 -> 120
        assert_eq!(hitung_bagi_hasil(100, true).bonus_prioritas, 1); // 0,5 -> 1
    }

    #[test]
    fn saldo_tidak_negatif_sisa_denda_dibawa() {
        // Saldo 9.640 (hasil Kasus A) lalu denda 15.000 -> saldo 0, utang 5.360.
        assert_eq!(terapkan_denda(9_640, 0, 15_000), (0, 5_360));
        // Saldo cukup: dipotong dari saldo, tanpa utang.
        assert_eq!(terapkan_denda(20_000, 0, 15_000), (5_000, 0));
        // Minggu depan: kredit 9.640 melunasi utang 5.360 lebih dulu.
        assert_eq!(terapkan_kredit(0, 5_360, 9_640), (4_280, 0));
        // Kredit lebih kecil dari utang: seluruhnya melunasi utang.
        assert_eq!(terapkan_kredit(0, 5_360, 1_000), (0, 4_360));
    }

    #[test]
    fn deposit_10_persen_fee_sampai_genap() {
        assert_eq!(tahan_deposit(5_640, 0), 564);
        assert_eq!(tahan_deposit(5_640, 99_800), 200); // tersisa 200
        assert_eq!(tahan_deposit(5_640, 100_000), 0);
    }
}
