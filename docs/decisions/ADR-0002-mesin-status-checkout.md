# ADR-0002 — Mesin status pesanan dan checkout idempoten (TTP-BE-004)

Status: Diusulkan | Tanggal: 2026-10-21

## Konteks

TTP-BE-004 mengimplementasikan mesin status 9 nilai dan `POST /pesanan` idempoten. Dokumen sumber bertentangan pada batas berat: FSD/openapi menyebut 20 kg per pesanan (422), AGENTS.md dan KONTEKS-LINTAS-DIVISI menyebut maksimal 50 kg per transaksi (di atasnya ditandai grosir).

## Keputusan

1. **Batas berat konfigurabel** lewat `BATAS_BERAT_GRAM`. Bawaan 50.000 g (mengikuti AGENTS/KONTEKS); set 20000 untuk perilaku FSD. Di atas batas = `422 BERAT_MELEBIHI_BATAS`; pesanan di atas 50.000 g ditandai `grosir` bila batas dinaikkan. Keputusan akhir milik manusia (pemilik dokumen).
2. **Transisi sah** (selain itu `422 TRANSISI_STATUS_ILEGAL`): `menunggu_pembayaran -> menunggu_konfirmasi | kedaluwarsa | dibatalkan_sistem`; `menunggu_konfirmasi -> disiapkan | dialihkan | dibatalkan_sistem`; `dialihkan -> menunggu_konfirmasi | dibatalkan_sistem`; `disiapkan -> diserahkan | dibatalkan_vendor | dibatalkan_sistem`; `diserahkan -> selesai`.
3. **Pengalihan**: vendor menolak atau lewat 30 menit membuat status `dialihkan`; admin menetapkan vendor cadangan (maks 2 kali, ke-3 = `422 ALIH_MELEBIHI_BATAS`). Bila gagal setelah 2 pengalihan: `dibatalkan_sistem` dengan `refund_jatuh_tempo` = H+1 (WIB).
4. **Idempotensi** `POST /pesanan`: kunci unik per `(id_rumah_tangga, Idempotency-Key)`; isi sama = 200 tanpa rekaman ganda, isi beda = `409 IDEMPOTENCY_KEY_BENTROK`.
5. **Audit append-only** `pesanan_audit` (trigger menolak UPDATE/DELETE): dari-status, ke-status, aktor, waktu.
6. Kode `T-YYYY-NNNNNN`: urutan per tahun WIB lewat tabel `urutan_kode` (atomik dalam transaksi).
7. Subtotal = gram x harga/kg / 1000, dibulatkan ke rupiah terdekat; ongkir flat Rp5.000 di luar `total_item`.
8. Katalog: tabel `katalog_cermin` (snapshot Lumbung); SKU berumur > 24 jam = `409 KATALOG_KEDALUWARSA`. Pengisian snapshot (job `sinkron-lumbung`) belum diimplementasikan.

## Konsekuensi

Fungsi job `tangani_timeout_konfirmasi` dan `kedaluwarsakan_belum_bayar` tersedia tetapi belum dijadwalkan (siklus 3). Endpoint daftar admin, serah, rating, outbox event belum termasuk tiket ini. Kontrak openapi hanya diberi catatan; bentuk endpoint tidak berubah.
