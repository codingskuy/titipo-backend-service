# ADR-0005 — Kunci idempoten sinyal Pedaree per jenis

Status: Diterima | Tanggal: 2026-10-21

## Konteks

ADR-0001 butir 6 dan kontrak awal memakai kunci idempoten konsumen `item_id + tanggal`. Akibatnya sinyal `stok.habis` setelah `stok.menipis` pada item dan hari yang sama dibalas `duplikat_diabaikan`, padahal keduanya informasi berbeda. Keputusan pengguna (disetujui): dedup per jenis sinyal. Usulan perubahan KONTEKS-LINTAS-DIVISI §3.1 diajukan terpisah (dokumen TownHall tidak diedit di repo ini).

## Keputusan

Kunci idempoten konsumen menjadi `item_id + jenis + tanggal`. `stok.menipis` dan `stok.habis` (juga `kedaluwarsa.dekat`) pada item dan hari yang sama masing-masing diterima; pengulangan jenis yang sama di hari yang sama tetap `200 {"status":"duplikat_diabaikan"}`. Menggantikan butir 6 ADR-0001 hanya pada definisi kunci; sifat anjuran saja (tidak pernah membuat pesanan) tidak berubah.

Migrasi: `0003_sinyal_restock.sql` diubah langsung (`UNIQUE (item_id, jenis, tanggal)`) karena belum merge dan belum ada konsumen kunci lama; tanpa dual-read.

## Konsekuensi

Bentuk request/response tidak berubah; hanya deskripsi idempotensi di `docs/api/openapi.yaml` (versi 1.0.2). Produsen (Pedaree) boleh mengirim maksimal 1 per item per jenis per 24 jam.
