# ADR-0004 — Ledger fee vendor (TTP-BE-006)

Status: Diusulkan | Tanggal: 2026-10-21

## Konteks

BRD/30-keuangan-fee dan `keuangan/10-bagi-fee.md` menetapkan fee 12%, bonus Prioritas 0,5%, ongkir Rp5.000 (Rp4.000 vendor + Rp1.000 kas), denda Rp15.000, penarikan, dan deposit Rp100.000, dengan tiga kasus hitungan mengikat. Beberapa detail tidak dirinci.

## Keputusan

1. Fee dihitung dari `total_item` (bukan `total_bayar`), pembulatan setengah ke atas pada rupiah penuh, semua integer.
2. Pencairan per pesanan `selesai` menulis satu entri `pencairan_pesanan` = fee + bonus + ongkir vendor (Kasus A Rp9.640, Kasus B Rp9.875). Idempoten per pesanan.
3. Denda (`dibatalkan_vendor`) dipotong dari saldo; saldo tidak pernah negatif, sisa disimpan sebagai `utang_denda` dan dilunasi lebih dulu dari kredit berikutnya (Kasus C: minus Rp5.360 menjadi utang Rp5.360).
4. Deposit: 10% dari fee ditahan (entri `deposit`) pada tiap pencairan sampai total Rp100.000; ASUMSI: ditahan dari saldo setelah pencairan, bukan mengurangi entri pencairan. Kasus A/B/C diuji pada vendor yang depositnya sudah genap.
5. Penarikan: minimal Rp50.000, mulai 06.00 WIB, saldo berkurang sebesar jumlah tarik, vendor menerima jumlah dikurangi Rp2.500.
6. Status Prioritas disimpan di `vendor_akun.prioritas` (diatur lewat `ledger::atur_prioritas`); sumber kebenarannya (kurasi vendor) belum ada di repo ini.
7. Buku besar `ledger_entri` append-only (trigger).

## Konsekuensi

Belum ada endpoint HTTP ledger (kontrak openapi belum memuatnya); fungsi `cairkan_h_plus_1` (job `cair-fee`), `catat_denda`, dan `tarik` siap dipakai tiket berikutnya. Pembayaran pokok Lumbung tiap Senin dan kas TitipO di luar tiket ini.
