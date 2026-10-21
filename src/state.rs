use crate::{
    auth::{
        jwks::{HttpJwksFetcher, JwksCache, JwksFetcher},
        service_key::Keyring,
    },
    config::Config,
};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub jwks: Arc<JwksCache>,
    pub keyring: Arc<Keyring>,
}

impl AppState {
    /// Susun state produksi: JWKS lewat HTTP ke Lumbung.
    pub fn dari_config(db: PgPool, config: Config) -> Self {
        let fetcher: Arc<dyn JwksFetcher> = Arc::new(HttpJwksFetcher {
            url: config.jwks_url.clone(),
            http: reqwest::Client::new(),
        });
        Self::dengan_fetcher(db, config, fetcher)
    }

    /// Susun state dengan pengambil JWKS sendiri (dipakai uji).
    pub fn dengan_fetcher(db: PgPool, config: Config, fetcher: Arc<dyn JwksFetcher>) -> Self {
        let jwks = Arc::new(JwksCache::baru(fetcher, config.jwks_ttl));
        let keyring = Arc::new(Keyring::baru(config.kunci_layanan.clone()));
        Self {
            db,
            config: Arc::new(config),
            jwks,
            keyring,
        }
    }
}
