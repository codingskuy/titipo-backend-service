-- 0004 — Ledger fee vendor (TTP-BE-006). Uang integer rupiah; saldo tidak pernah negatif.

CREATE TABLE vendor_akun (
    id_vendor TEXT PRIMARY KEY,
    prioritas BOOLEAN NOT NULL DEFAULT false,
    saldo BIGINT NOT NULL DEFAULT 0 CHECK (saldo >= 0),
    -- Sisa denda yang belum terpotong (saldo tidak boleh negatif); dibawa ke minggu depan.
    utang_denda BIGINT NOT NULL DEFAULT 0 CHECK (utang_denda >= 0),
    deposit_terkumpul BIGINT NOT NULL DEFAULT 0 CHECK (deposit_terkumpul BETWEEN 0 AND 100000),
    diubah_pada TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Buku besar append-only. `jumlah` bertanda: kredit (+) / debit (-) terhadap saldo.
CREATE TABLE ledger_entri (
    id BIGSERIAL PRIMARY KEY,
    id_vendor TEXT NOT NULL REFERENCES vendor_akun(id_vendor),
    jenis TEXT NOT NULL CHECK (jenis IN ('pencairan_pesanan','denda','deposit','penarikan')),
    pesanan_kode TEXT,
    jumlah BIGINT NOT NULL,
    fee BIGINT NOT NULL DEFAULT 0,
    bonus_prioritas BIGINT NOT NULL DEFAULT 0,
    ongkir_vendor BIGINT NOT NULL DEFAULT 0,
    ongkir_kas BIGINT NOT NULL DEFAULT 0,
    biaya_transfer BIGINT NOT NULL DEFAULT 0,
    saldo_setelah BIGINT NOT NULL CHECK (saldo_setelah >= 0),
    utang_denda_setelah BIGINT NOT NULL DEFAULT 0 CHECK (utang_denda_setelah >= 0),
    dibuat_pada TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Idempoten: satu pencairan dan satu denda per pesanan.
CREATE UNIQUE INDEX uq_ledger_pesanan_jenis ON ledger_entri (pesanan_kode, jenis) WHERE pesanan_kode IS NOT NULL;
CREATE INDEX idx_ledger_vendor ON ledger_entri (id_vendor, id);

CREATE FUNCTION tolak_ubah_ledger() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'ledger_entri bersifat append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_ledger_append_only
    BEFORE UPDATE OR DELETE ON ledger_entri
    FOR EACH ROW EXECUTE FUNCTION tolak_ubah_ledger();
