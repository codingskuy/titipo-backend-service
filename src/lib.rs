//! titipo-backend-service — API TitipO (pesanan, vendor, fee, sinyal Pedaree).

pub mod auth;
pub mod config;
pub mod error;
pub mod lumbung;
pub mod pesanan;
pub mod pesanan_repo;
pub mod routes;
pub mod state;

pub use routes::app;
pub use state::AppState;
