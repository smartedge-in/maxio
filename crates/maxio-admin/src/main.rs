use clap::Parser;
use maxio_admin::{Cli, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    cli.run().await
}
