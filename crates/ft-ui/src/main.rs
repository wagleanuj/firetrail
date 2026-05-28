//! ft-ui binary entrypoint. Parses CLI flags, initialises tracing, runs the server.

#![forbid(unsafe_code)]

use clap::Parser;
use ft_ui::server::{self, ServerOpts};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = ServerOpts::parse();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    server::run(opts).await
}
