use bucket::{App, config::Config, router};
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bucket=info,tower_http=warn".into()),
        )
        .init();
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {e}");
        std::process::exit(1)
    });
    if let Some(parent) = std::path::Path::new(&config.database).parent() {
        std::fs::create_dir_all(parent).expect("create data directory");
    }
    // SQLite locking protects upgrades. Only one application instance may own a database.
    let app = App::new(config.clone()).expect("initialize application database");
    let inbox = app.inbox.clone();
    let database = app.db.clone();
    let sending = app.sending.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            inbox.cleanup(&database);
            sending.cleanup(&database);
        }
    });
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await
        .expect("bind listener");
    tracing::info!(bind=%config.bind,"Bucket is ready");
    axum::serve(
        listener,
        router(app).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await
    .expect("HTTP server");
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("Shutting down");
}
