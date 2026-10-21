-- 0002 — Pesanan, item, pembayaran, audit append-only, katalog cermin Lumbung (TTP-BE-004).
-- Tabel pesanan lama (0001) belum berisi data produksi; diganti skema 9 status.

DROP TABLE IF EXISTS pesanan CASCADE;

CREATE TABLE katalog_cermin (
    id_vendor TEXT NOT NULL,
    sku_lumbung TEXT NOT NULL,
    nama TEXT NOT NULL,
    harga_per_kg BIGINT NOT NULL CHECK (harga_per_kg >= 0),
    stok_gram BIGINT NOT NULL CHECK (stok_gram >= 0),
    disinkron_pada TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (id_vendor, sku_lumbung)
);

CREATE TABLE urutan_kode (
    tahun INT PRIMARY KEY,
    terakhir BIGINT NOT NULL
);

CREATE TABLE pesanan (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kode TEXT NOT NULL UNIQUE CHECK (kode ~ '^T-[0-9]{4}-[0-9]{6}$'),
    id_klien TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    id_rumah_tangga TEXT NOT NULL,
    id_vendor TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN (
        'menunggu_pembayaran','menunggu_konfirmasi','disiapkan','diserahkan','selesai',
        'kedaluwarsa','dialihkan','dibatalkan_sistem','dibatalkan_vendor')),
    total_item BIGINT NOT NULL CHECK (total_item >= 0),
    ongkir BIGINT NOT NULL CHECK (ongkir = 5000),
    total_bayar BIGINT NOT NULL CHECK (total_bayar = total_item + ongkir),
    berat_gram BIGINT NOT NULL CHECK (berat_gram > 0),
    grosir BOOLEAN NOT NULL DEFAULT false,
    jadwal_serah DATE NOT NULL,
    kedaluwarsa_bayar TIMESTAMPTZ NOT NULL,
    batas_konfirmasi TIMESTAMPTZ,
    dikonfirmasi_pada TIMESTAMPTZ,
    jumlah_alih SMALLINT NOT NULL DEFAULT 0 CHECK (jumlah_alih BETWEEN 0 AND 2),
    refund_jatuh_tempo DATE,
    dibuat_pada TIMESTAMPTZ NOT NULL DEFAULT now(),
    diubah_pada TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id_rumah_tangga, idempotency_key)
);
CREATE INDEX idx_pesanan_rumah_tangga ON pesanan (id_rumah_tangga);
CREATE INDEX idx_pesanan_vendor ON pesanan (id_vendor);
CREATE INDEX idx_pesanan_status ON pesanan (status);

CREATE TABLE pesanan_item (
    pesanan_id UUID NOT NULL REFERENCES pesanan(id) ON DELETE CASCADE,
    sku_lumbung TEXT NOT NULL,
    nama TEXT NOT NULL,
    jumlah_gram BIGINT NOT NULL CHECK (jumlah_gram > 0),
    harga_per_kg BIGINT NOT NULL CHECK (harga_per_kg >= 0),
    subtotal BIGINT NOT NULL CHECK (subtotal >= 0),
    PRIMARY KEY (pesanan_id, sku_lumbung)
);

CREATE TABLE pembayaran (
    pesanan_id UUID PRIMARY KEY REFERENCES pesanan(id),
    id_gateway TEXT NOT NULL UNIQUE,
    jumlah BIGINT NOT NULL CHECK (jumlah >= 0),
    dicatat_pada TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Audit append-only: tiap transisi mencatat dari-status, ke-status, aktor, waktu.
CREATE TABLE pesanan_audit (
    id BIGSERIAL PRIMARY KEY,
    pesanan_id UUID NOT NULL REFERENCES pesanan(id),
    dari_status TEXT,
    ke_status TEXT NOT NULL,
    aktor TEXT NOT NULL,
    waktu TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_pesanan_audit_pesanan ON pesanan_audit (pesanan_id, id);

CREATE FUNCTION tolak_ubah_audit() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'pesanan_audit bersifat append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_pesanan_audit_append_only
    BEFORE UPDATE OR DELETE ON pesanan_audit
    FOR EACH ROW EXECUTE FUNCTION tolak_ubah_audit();
