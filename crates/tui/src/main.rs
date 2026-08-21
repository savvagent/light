//! light-factory TUI entry point.

mod api;
mod app;
mod browser;
mod config;
mod i18n;
mod provider;
mod session;
mod settings;
mod ws;

use clap::Parser;
use i18n::Locale;

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

    /// Language for the interface (en, es). Overrides the saved preference and
    /// environment, and persists for future runs.
    #[arg(long)]
    lang: Option<String>,

    /// Clear the saved session and exit.
    #[arg(long)]
    logout: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let locale = resolve_locale(&cli);

    if cli.logout {
        session::Session::clear()?;
        println!("{}", i18n::t(locale, "logout_done"));
        return Ok(());
    }

    let config = config::Config::from_url(&cli.url)?.with_lang(locale);
    let (provider, info) = provider::build();
    app::run(config, provider, info, cli.email).await
}

/// Resolve the locale from `--lang` → saved config → `LC_ALL` → `LANG` → English,
/// persisting the explicit `--lang` choice.
fn resolve_locale(cli: &Cli) -> Locale {
    let lc_all = std::env::var("LC_ALL").ok();
    let lang_env = std::env::var("LANG").ok();
    let saved = settings::load_lang();

    let locale = i18n::resolve_locale(
        cli.lang.as_deref(),
        saved.as_deref(),
        lang_env.as_deref(),
        lc_all.as_deref(),
    );

    if let Some(raw) = cli.lang.as_deref()
        && let Some(parsed) = Locale::parse(raw)
    {
        let _ = settings::save_lang(parsed.as_str());
    }

    locale
}
