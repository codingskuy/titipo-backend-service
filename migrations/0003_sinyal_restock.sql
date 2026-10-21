-- 0003 — Anjuran restock dari sinyal pedaree.stok.v1 (TTP-BE-005). Anjuran saja, bukan pesanan.
CREATE TABLE sinyal_restock (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    jenis TEXT NOT NULL CHECK (jenis IN ('stok.menipis','stok.habis','kedaluwarsa.dekat')),
    item_id TEXT NOT NULL,
    tanggal DATE NOT NULL,
    item TEXT NOT NULL,
    sisa DOUBLE PRECISION NOT NULL CHECK (sisa >= 0),
    satuan TEXT NOT NULL,
    kedaluwarsa DATE NOT NULL,
    recipe_id TEXT,
    diterima_pada TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Kunci dedup konsumen: item_id + jenis + tanggal (menipis dan habis dedup terpisah).
    UNIQUE (item_id, jenis, tanggal)
);
CREATE INDEX idx_sinyal_restock_diterima ON sinyal_restock (diterima_pada);
