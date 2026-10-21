# ADR-0003 — Respons 400/422 untuk query tak valid di endpoint admin

Status: Diterima | Tanggal: 2026-10-21

## Konteks

Schemathesis dari titipo-web (mock Prism) menemukan `GET /admin/vendor?status=AAA` dan `GET /admin/pesanan?status=AAA` menjawab 422, tetapi `docs/api/openapi.yaml` hanya mendokumentasikan 200/401/403/429/500 (ST103). Issue #8.

## Keputusan

Kontrak naik ke 1.0.1 (hanya penambahan respons terdokumentasi, bentuk sukses tidak berubah):

- `400` (`Permintaan`): tipe parameter salah (`batas` bukan bilangan bulat, `hari` bukan tanggal ISO).
- `422` (`Validasi`): nilai enum `status` tidak dikenal, `status` kosong (`status=`), `batas` di luar 1..50.
- Tanpa parameter `status` berarti semua status; `status=` (kosong) TIDAK valid (422).

## Konsekuensi

Implementasi provider wajib mengikuti pembedaan ini; klien dan Schemathesis boleh mengandalkan 400/422 sebagai respons terdokumentasi.
