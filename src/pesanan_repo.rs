//! Akses data pesanan dengan penegakan cakupan (`Skup`).

use crate::auth::Skup;
use sqlx::PgPool;
use uuid::Uuid;

/// Id pesanan yang boleh dilihat pemegang `skup`.
/// Rumah tangga hanya miliknya; vendor hanya lapaknya; admin/operasi semua.
pub async fn id_terlihat(db: &PgPool, skup: &Skup) -> Result<Vec<Uuid>, sqlx::Error> {
    let (rt, vendor): (Option<&str>, Option<&str>) = match skup {
        Skup::RumahTangga(id) => (Some(id.as_str()), None),
        Skup::Vendor(id) => (None, Some(id.as_str())),
        Skup::Semua => (None, None),
    };
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM pesanan
         WHERE ($1::text IS NULL OR id_rumah_tangga = $1)
           AND ($2::text IS NULL OR id_vendor = $2)
         ORDER BY dibuat_pada, id",
    )
    .bind(rt)
    .bind(vendor)
    .fetch_all(db)
    .await
}
