//! Cache JWKS Lumbung (TTL 10 menit). Kunci tak dikenal memicu satu refresh (rotasi kunci),
//! dibatasi interval minimum agar `kid` acak tidak membanjiri endpoint JWKS Lumbung.

use jsonwebtoken::jwk::{Jwk, JwkSet};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = Result<JwkSet, String>> + Send + 'a>>;

/// Pengambil JWKS; implementasi HTTP untuk produksi, statis untuk uji.
pub trait JwksFetcher: Send + Sync {
    fn ambil(&self) -> FetchFuture<'_>;
}

pub struct HttpJwksFetcher {
    pub url: String,
    pub http: reqwest::Client,
}

impl JwksFetcher for HttpJwksFetcher {
    fn ambil(&self) -> FetchFuture<'_> {
        Box::pin(async move {
            let resp = self.http.get(&self.url).send().await.map_err(|e| e.to_string())?;
            let resp = resp.error_for_status().map_err(|e| e.to_string())?;
            resp.json::<JwkSet>().await.map_err(|e| e.to_string())
        })
    }
}

struct Isi {
    set: JwkSet,
    diambil: Instant,
}

pub struct JwksCache {
    fetcher: Arc<dyn JwksFetcher>,
    ttl: Duration,
    refresh_minimum: Duration,
    isi: RwLock<Option<Isi>>,
}

impl JwksCache {
    pub fn baru(fetcher: Arc<dyn JwksFetcher>, ttl: Duration) -> Self {
        Self {
            fetcher,
            ttl,
            refresh_minimum: Duration::from_secs(30),
            isi: RwLock::new(None),
        }
    }

    pub fn dengan_refresh_minimum(mut self, d: Duration) -> Self {
        self.refresh_minimum = d;
        self
    }

    async fn muat(&self) -> Result<(), String> {
        let set = self.fetcher.ambil().await?;
        *self.isi.write().await = Some(Isi {
            set,
            diambil: Instant::now(),
        });
        Ok(())
    }

    fn cari(set: &JwkSet, kid: Option<&str>) -> Option<Jwk> {
        match kid {
            Some(k) => set.find(k).cloned(),
            // Tanpa kid: hanya diterima bila JWKS berisi tepat satu kunci.
            None if set.keys.len() == 1 => set.keys.first().cloned(),
            None => None,
        }
    }

    /// Ambil kunci untuk `kid`. `Ok(None)` = kunci memang tidak ada di JWKS.
    pub async fn kunci(&self, kid: Option<&str>) -> Result<Option<Jwk>, String> {
        // 1. Cache segar?
        let perlu_muat = {
            let g = self.isi.read().await;
            match g.as_ref() {
                Some(i) if i.diambil.elapsed() < self.ttl => {
                    if let Some(k) = Self::cari(&i.set, kid) {
                        return Ok(Some(k));
                    }
                    // Tidak ketemu: boleh refresh bila lewat interval minimum.
                    i.diambil.elapsed() >= self.refresh_minimum
                }
                _ => true,
            }
        };
        if !perlu_muat {
            return Ok(None);
        }
        self.muat().await?;
        let g = self.isi.read().await;
        Ok(g.as_ref().and_then(|i| Self::cari(&i.set, kid)))
    }
}
