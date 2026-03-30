mod app;
mod config;
mod player;
mod spotify;
mod ui;

use anyhow::Result;
use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("isi_music=info".parse()?),
        )
        .init();

    let mut app = App::new().await?;
    app.run().await
}
