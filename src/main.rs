use sqlx::postgres::PgPoolOptions;
use titipo_backend_service::{app, config::Config, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("titipo_backend_service=debug,tower_http=debug")
        .init();

    let config = Config::dari_env().unwrap_or_else(|e| {
        eprintln!("konfigurasi tidak valid: {e}");
        std::process::exit(1);
    });
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("gagal terhubung ke DB titipo");
    sqlx::migrate!("./migrations").run(&db).await.expect("migrasi gagal");

    let alamat = config.alamat.clone();
    let router = app(AppState::dari_config(db, config));
    let listener = tokio::net::TcpListener::bind(&alamat).await.expect("gagal bind");
    tracing::info!("titipo-backend-service mendengar di {}", alamat);
    axum::serve(listener, router).await.expect("server berhenti");
}
