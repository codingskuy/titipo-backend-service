# CHANGELOG — titipo-backend-service

## [Belum dirilis]

### Ditambahkan
- 2026-10-21: TTP-BE-006 ledger fee 12% + bonus Prioritas 0,5% + denda Rp15.000 + penarikan + deposit; 3 kasus mengikat lolos uji (migrasi 0004, ADR-0004).
- 2026-10-21: TTP-BE-004 mesin status 9 nilai, checkout idempoten `POST /pesanan` (kode `T-YYYY-NNNNNN`), `bayar`, konfirmasi vendor, alihkan admin, audit append-only (migrasi 0002, ADR-0002). `BATAS_BERAT_GRAM` konfigurabel.
- 2026-10-21: TTP-BE-003 auth JWT audiens `titipo` (JWKS Lumbung, cache 10 menit, skew 60 dtk) + service key `X-Titipo-Service-Key` dengan rotasi 2 kunci; struktur lib+bin, `AppState`/`AppError`.
- 2026-10-14: fondasi AI-dev v1.0.0 (`AGENTS.md`, `CLAUDE.md`, `docs/decisions/ADR-0001-fondasi-teknis.md`, `versions/v1.0.0/docs/40-TECHSTACK.md`, `versions/v1.0.0/docs/60-BLAST-RADIUS.md`).
