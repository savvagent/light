//! Client configuration resolved from the CLI and environment.

use light_factory_tui::i18n::Locale;

/// The base HTTP URL of the server plus the derived WebSocket URL and locale.
#[derive(Debug, Clone)]
pub struct Config {
    pub http_base: String,
    pub ws_url: String,
    pub lang: Locale,
}

impl Config {
    /// Build a [`Config`] from a base URL. `http://` maps to `ws://` and
    /// `https://` maps to `wss://`; anything else is rejected. The locale
    /// defaults to English; use [`Config::with_lang`] to override it.
    pub fn from_url(base: &str) -> anyhow::Result<Self> {
        let http_base = base.trim().trim_end_matches('/').to_string();
        let ws_base = if http_base.starts_with("https://") {
            http_base.replace("https://", "wss://")
        } else if http_base.starts_with("http://") {
            http_base.replace("http://", "ws://")
        } else {
            anyhow::bail!("URL must start with http:// or https://, got: {base}");
        };
        Ok(Self {
            http_base,
            ws_url: format!("{ws_base}/ws"),
            lang: Locale::En,
        })
    }

    /// Set the locale this client renders in.
    pub fn with_lang(mut self, lang: Locale) -> Self {
        self.lang = lang;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn maps_http_to_ws() {
        let cfg = Config::from_url("http://localhost:8080").unwrap();
        assert_eq!(cfg.http_base, "http://localhost:8080");
        assert_eq!(cfg.ws_url, "ws://localhost:8080/ws");
    }

    #[test]
    fn maps_https_to_wss_and_trims_trailing_slash() {
        let cfg = Config::from_url("https://light-factory.fly.dev/").unwrap();
        assert_eq!(cfg.http_base, "https://light-factory.fly.dev");
        assert_eq!(cfg.ws_url, "wss://light-factory.fly.dev/ws");
    }

    #[test]
    fn rejects_unknown_scheme() {
        assert!(Config::from_url("ftp://example.com").is_err());
    }
}
