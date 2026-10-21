# Catatan keputusan arsitektur (ADR)

Setiap keputusan arsitektur ditulis sebagai satu berkas ADR.

## Cara menambah ADR

1. Nomori berurutan: `ADR-0002-<slug-singkat>.md`, `ADR-0003-...`, tanpa melompati nomor.
2. Satu keputusan per berkas. Format: `Status: Diusulkan | Diterima | Digantikan oleh ADR-NNNN`, `Tanggal`, `Konteks`, `Keputusan`, `Konsekuensi`.
3. ADR bersifat immutable: jangan mengedit isi ADR lama. Untuk mengubah keputusan, tulis ADR baru lalu ubah hanya baris `Status` ADR lama menjadi `Digantikan oleh ADR-NNNN`.
4. Perubahan kontrak API/event wajib punya ADR baru bersama bump versi.
5. Daftar ADR saat ini: `ADR-0001-fondasi-teknis.md`, `ADR-0002-mesin-status-checkout.md`, `ADR-0004-ledger-fee.md`.
