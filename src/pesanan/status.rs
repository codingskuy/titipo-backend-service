//! Mesin status pesanan (9 status) dan aturan transisi sah.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPesanan {
    MenungguPembayaran,
    MenungguKonfirmasi,
    Disiapkan,
    Diserahkan,
    Selesai,
    Kedaluwarsa,
    Dialihkan,
    DibatalkanSistem,
    DibatalkanVendor,
}

use StatusPesanan::*;

impl StatusPesanan {
    pub const SEMUA: [StatusPesanan; 9] = [
        MenungguPembayaran,
        MenungguKonfirmasi,
        Disiapkan,
        Diserahkan,
        Selesai,
        Kedaluwarsa,
        Dialihkan,
        DibatalkanSistem,
        DibatalkanVendor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MenungguPembayaran => "menunggu_pembayaran",
            MenungguKonfirmasi => "menunggu_konfirmasi",
            Disiapkan => "disiapkan",
            Diserahkan => "diserahkan",
            Selesai => "selesai",
            Kedaluwarsa => "kedaluwarsa",
            Dialihkan => "dialihkan",
            DibatalkanSistem => "dibatalkan_sistem",
            DibatalkanVendor => "dibatalkan_vendor",
        }
    }

    pub fn dari_str(s: &str) -> Option<Self> {
        Self::SEMUA.into_iter().find(|x| x.as_str() == s)
    }

    /// Status tujuan yang sah dari status ini.
    pub fn tujuan_sah(self) -> &'static [StatusPesanan] {
        match self {
            MenungguPembayaran => &[MenungguKonfirmasi, Kedaluwarsa, DibatalkanSistem],
            MenungguKonfirmasi => &[Disiapkan, Dialihkan, DibatalkanSistem],
            Dialihkan => &[MenungguKonfirmasi, DibatalkanSistem],
            Disiapkan => &[Diserahkan, DibatalkanVendor, DibatalkanSistem],
            Diserahkan => &[Selesai],
            Selesai | Kedaluwarsa | DibatalkanSistem | DibatalkanVendor => &[],
        }
    }

    pub fn boleh_ke(self, tujuan: StatusPesanan) -> bool {
        self.tujuan_sah().contains(&tujuan)
    }

    pub fn terminal(self) -> bool {
        self.tujuan_sah().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ada_sembilan_status_unik_dan_bisa_dibaca_balik() {
        assert_eq!(StatusPesanan::SEMUA.len(), 9);
        for s in StatusPesanan::SEMUA {
            assert_eq!(StatusPesanan::dari_str(s.as_str()), Some(s));
        }
        assert_eq!(StatusPesanan::dari_str("ngawur"), None);
    }

    #[test]
    fn jalur_bahagia_sah() {
        assert!(MenungguPembayaran.boleh_ke(MenungguKonfirmasi));
        assert!(MenungguKonfirmasi.boleh_ke(Disiapkan));
        assert!(Disiapkan.boleh_ke(Diserahkan));
        assert!(Diserahkan.boleh_ke(Selesai));
    }

    #[test]
    fn transisi_ilegal_ditolak() {
        assert!(!MenungguPembayaran.boleh_ke(Disiapkan));
        assert!(!MenungguKonfirmasi.boleh_ke(Selesai));
        assert!(!Diserahkan.boleh_ke(Disiapkan));
        assert!(!Disiapkan.boleh_ke(MenungguPembayaran));
        for s in StatusPesanan::SEMUA {
            assert!(!s.boleh_ke(s), "{s:?} tidak boleh ke dirinya sendiri");
        }
    }

    #[test]
    fn status_akhir_tidak_punya_tujuan() {
        for s in [Selesai, Kedaluwarsa, DibatalkanSistem, DibatalkanVendor] {
            assert!(s.terminal());
        }
        assert!(!Dialihkan.terminal());
    }
}
