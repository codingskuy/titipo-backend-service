# Kontrak Auth TitipO — mengikuti pola Lumbung

Sumber pola: [Lumbung-TownHall](https://github.com/Coding-Skuy/Lumbung-TownHall).
TownHall TitipO: [TitipO-TownHall](https://github.com/Coding-Skuy/TitipO-TownHall).

## 1. JWT pengguna (`aud=titipo`)

- Penerbit: layanan auth Lumbung (issuer `https://auth.lumbung.skuy`).
- Klaim wajib:
  - `aud = "titipo"` — tolak token dengan audien lain (`403`).
  - `sub` — id pengguna Lumbung.
  - `role` — salah satu `vendor`, `household`, `admin`.
  - `exp`, `iat` standar.
- Verifikasi: JWKS Lumbung, cache 10 menit, toleransi clock-skew 60 detik.
- Klien (`titipo-app-kmp`, `titipo-web`) mengirim `Authorization: Bearer <token>` ke setiap request.

Contoh verifikasi (jsonwebtoken 9, RS256):

```rust
// Kunci JWKS diambil dari https://auth.lumbung.skuy/.well-known/jwks.json
// Validasi: aud == ["titipo"], iss == "https://auth.lumbung.skuy"
```

## 2. Service key `titipo → lumbung`

Untuk panggilan server-ke-server (mis. cek saldo penitip, validasi vendor):

- Header: `X-Titipo-Service-Key: <nilai dari TITIPO_SERVICE_KEY>`
- Simpan di env backend TitipO (`TITIPO_SERVICE_KEY`) dan di secret manager Lumbung (`LUMBUNG_TITIPO_KEYS`).
- Rotasi tiap 90 hari. Saat rotasi, dukung 2 kunci aktif selama 7 hari.
- Tolak tanpa kunci (`401`) atau kunci salah (`403`).

## 3. Variabel lingkungan

```bash
LUMBUNG_JWKS_URL=https://auth.lumbung.skuy/.well-known/jwks.json
LUMBUNG_ISSUER=https://auth.lumbung.skuy
TITIPO_JWT_AUD=titipo
TITIPO_SERVICE_KEY=isi-di-secret-manager
DATABASE_URL=postgres://titipo:titipo@localhost:5432/titipo
```

## 4. Matriks akses awal

| Rute | household | vendor | admin |
|------|-----------|--------|-------|
| `GET /healthz` | bebas | bebas | bebas |
| `GET /v1/contoh` | token | token | token |
| `pesanan/*` (nanti) | buat/baca miliknya | baca/terima lapaknya | semua |
| `langganan/*` (nanti) | kelola miliknya | baca pelanggannya | semua |

## 5. Implementasi (TTP-BE-003)

- Hanya RS256; algoritma lain ditolak 401. Pemetaan galat: tanpa token `TOKEN_TIDAK_ADA` 401, token kedaluwarsa `TOKEN_KEDALUWARSA` 401, audiens bukan `titipo` `AUDIENS_SALAH` 403, peran tak berhak `PERAN_DITOLAK` 403.
- Cache JWKS 10 menit; `kid` tak dikenal memicu satu refresh (rotasi kunci Lumbung) dengan jeda minimum 30 detik.
- Rotasi service key: `TITIPO_SERVICE_KEY` (aktif) dan opsional `TITIPO_SERVICE_KEY_PREV` + `TITIPO_SERVICE_KEY_PREV_SAMPAI` (RFC 3339, maks 7 hari setelah rotasi). Tanpa header 401 `SERVICE_KEY_TIDAK_ADA`, salah 403 `SERVICE_KEY_SALAH`.
- Variabel tambahan: `LUMBUNG_BASE_URL` (klien keluar ke Lumbung), `TITIPO_BIND` (default `0.0.0.0:8080`).
- Cakupan data (`Skup`): `household` dibatasi `sub` = id rumah tangga; `vendor` dibatasi klaim `vendor_id` (ASUMSI: opsional, bila kosong memakai `sub`); `admin` semua (tulis); `operasi` semua (baca saja, sesuai openapi).
- TTL token ditentukan penerbit (Lumbung); TitipO hanya memverifikasi `exp`. Standar KONTEKS/AGENTS: akses 15 menit + refresh 7 hari; angka "JWT 24 jam" pada FSD tidak dipakai di sini.

- Sinyal Pedaree (`POST /sinyal/pedaree-stok`) memakai kunci terpisah `PEDAREE_SERVICE_KEY` (header `X-Service-Key`), tidak berlaku di rute lain; JWT dan `X-Titipo-Service-Key` tidak diterima di endpoint ini.
