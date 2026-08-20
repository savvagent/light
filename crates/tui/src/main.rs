//! light-factory TUI entry point.

mod api;
mod app;
mod browser;
mod config;
mod session;
mod ws;

use clap::Parser;

/// Terminal UI client for the light-factory agentic coding platform.
#[derive(Parser)]
#[command(name = "light-factory", version, about, long_about = None)]
struct Cli {
    /// Base URL of the light-factory server.
    #[arg(long, env = "LIGHT_API_URL", default_value = "http://localhost:8080")]
    url: String,

    /// Email to pre-fill on the sign-in form.
    #[arg(long)]
    email: Option<String>,

    /// Clear the saved session and exit.
    #[arg(long)]
    logout: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.logout {
        session::Session::clear()?;
        println!("Logged out.");
        return Ok(());
    }

    let config = config::Config::from_url(&cli.url)?;
    app::run(config, cli.email).await
}
