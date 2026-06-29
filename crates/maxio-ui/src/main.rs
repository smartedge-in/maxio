use axum::Router;
use axum::routing::get;
use clap::Parser;
use maxio_ui::embedded::ui_handler;

#[derive(Parser, Debug)]
#[command(
    name = "maxio-ui",
    about = "Stateless MaxIO web console (static SPA only)",
    version = maxio_common::version::VERSION
)]
struct Cli {
    #[arg(long, env = "MAXIO_UI_PORT", default_value = "9080")]
    port: u16,

    #[arg(long, env = "MAXIO_UI_ADDRESS", default_value = "0.0.0.0")]
    address: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    async fn serve_ui(uri: axum::http::Uri) -> axum::response::Response {
        ui_handler(uri).await
    }

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .fallback(get(serve_ui));

    let addr = format!("{}:{}", cli.address, cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(
        "maxio-ui v{} listening on http://{}",
        maxio_common::version::VERSION,
        addr
    );
    axum::serve(listener, app).await?;
    Ok(())
}
