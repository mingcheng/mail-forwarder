use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub receivers: Vec<ReceiverConfig>,
    pub sender: SenderConfig,
    pub forward_to: String,
    pub log_file: Option<String>,
    pub log_level: Option<String>,
    #[serde(default)]
    pub quiet: bool,
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct SenderConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
}

// Default check interval in seconds (5 minutes)
pub const DEFAULT_CHECK_INTERVAL_SECONDS: u64 = 300;

// Implement loading configuration
impl AppConfig {
    // Load config from defaults, then file (if exists), then environment variables
    #[allow(dead_code)]
    pub fn new() -> Result<Self, ConfigError> {
        Self::configure_defaults()?
            // Merge in config file if present
            .add_source(File::with_name("config").required(false))
            // Merge in environment variables
            // e.g. APP_FORWARD_TO=... APP_RECEIVER__HOST=...
            .add_source(Environment::with_prefix("APP").separator("__"))
            .build()?
            .try_deserialize()
    }

    // Load config from a specific file path
    #[allow(dead_code)]
    pub fn new_from_file(path: &str) -> Result<Self, ConfigError> {
        Self::configure_defaults()?
            .add_source(File::with_name(path).required(true))
            .add_source(Environment::with_prefix("APP").separator("__"))
            .build()?
            .try_deserialize()
    }

    fn configure_defaults()
    -> Result<config::ConfigBuilder<config::builder::DefaultState>, ConfigError> {
        Ok(Config::builder())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::FileFormat;

    #[test]
    fn test_valid_config_deserialization() {
        let toml_str = r#"
            forward_to = "target@example.com"

            [sender]
            host = "smtp.example.com"
            port = 587
            username = "sender_user"
            password = "sender_pass"

            [[receivers]]
            host = "pop.example.com"
            port = 995
            username = "receiver_user"
            password = "receiver_pass"
            use_tls = true
            delete_after_forward = false
        "#;

        let builder = AppConfig::configure_defaults()
            .unwrap()
            .add_source(File::from_str(toml_str, FileFormat::Toml));

        let config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.forward_to, "target@example.com");

        assert_eq!(config.sender.host, "smtp.example.com");
        assert_eq!(config.sender.port, 587);

        assert_eq!(config.receivers.len(), 1);
        let receiver = &config.receivers[0];
        assert_eq!(receiver.host, "pop.example.com");
        assert_eq!(receiver.use_tls, true);
        assert_eq!(receiver.delete_after_forward, Some(false));
    }

    #[test]
    fn test_default_values() {
        // Minimal config (missing check_interval_seconds)
        let toml_str = r#"
            forward_to = "target@example.com"

            [sender]
            host = "smtp.example.com"
            port = 587
            username = "u"
            password = "p"
            
            [[receivers]]
            host = "pop.example.com"
            port = 995
            username = "u"
            password = "p"
            use_tls = true
        "#;

        let builder = AppConfig::configure_defaults()
            .unwrap()
            .add_source(File::from_str(toml_str, FileFormat::Toml));

        let _config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();
    }

    #[test]
    fn test_multiple_receivers() {
        let toml_str = r#"
            forward_to = "t"
            [sender]
            host = "h"
            port = 1
            username = "u"
            password = "p"

            [[receivers]]
            host = "r1"
            port = 1
            username = "u1"
            password = "p1"
            use_tls = true

            [[receivers]]
            host = "r2"
            port = 2
            username = "u2"
            password = "p2"
            use_tls = false
        "#;

        let builder = AppConfig::configure_defaults()
            .unwrap()
            .add_source(File::from_str(toml_str, FileFormat::Toml));

        let config: AppConfig = builder.build().unwrap().try_deserialize().unwrap();

        assert_eq!(config.receivers.len(), 2);
        assert_eq!(config.receivers[0].host, "r1");
        assert_eq!(config.receivers[1].host, "r2");
    }

    #[test]
    fn test_invalid_config_type() {
        let toml_str = r#"
            forward_to = 123 # Invalid type
            
            [sender]
            host = "h" 
            port = 1
            username = "u"
            password = "p"
        "#;

        let builder = AppConfig::configure_defaults()
            .unwrap()
            .add_source(File::from_str(toml_str, FileFormat::Toml));

        let res: Result<AppConfig, _> = builder.build().unwrap().try_deserialize();
        assert!(res.is_err());
    }
}
