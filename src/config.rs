use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub receivers: Vec<ReceiverConfig>,
    pub sender: SenderConfig,
    pub forward_to: String,
    pub check_interval_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReceiverConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
    pub check_interval_seconds: Option<u64>,
    pub delete_after_forward: Option<bool>,
    pub proxy: Option<String>, // e.g. "socks5://127.0.0.1:1080"
}

#[derive(Debug, Deserialize, Clone)]
pub struct SenderConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub proxy: Option<String>, // e.g. "socks5://127.0.0.1:1080"
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        let builder = Config::builder()
            // Start with defaults
            .set_default("check_interval_seconds", 60)?
            // Merge in config file if present
            .add_source(File::with_name("config").required(false))
            // Merge in environment variables
            // e.g. APP_FORWARD_TO=... APP_RECEIVER__HOST=...
            .add_source(Environment::with_prefix("APP").separator("__"));

        builder.build()?.try_deserialize()
    }
}
