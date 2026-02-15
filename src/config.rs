/*!
 * Copyright (c) 2026 Ming Lyu, aka mingcheng
 *
 * This source code is licensed under the MIT License,
 * which is located in the LICENSE file in the source tree's root directory.
 *
 * File: config.rs
 * Author: mingcheng <mingcheng@apache.org>
 * File Created: 2026-02-12 22:37:25
 *
 * Modified By: mingcheng <mingcheng@apache.org>
 * Last Modified: 2026-02-15 14:37:31
 */

use config::{Config, ConfigError, File};
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
    #[serde(default = "default_protocol")]
    pub protocol: String, // "pop3" or "imap"
    pub use_tls: Option<bool>,
    pub check_interval_seconds: Option<u64>,
    pub delete_after_forward: Option<bool>,
    #[serde(default = "default_imap_folder")]
    pub imap_folder: String, // IMAP mailbox folder, default "INBOX"
}

// Default protocol is "pop3"
fn default_protocol() -> String {
    "pop3".to_string()
}

// Default IMAP folder is "INBOX"
fn default_imap_folder() -> String {
    "INBOX".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct SenderConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub use_tls: Option<bool>,
}

// Default check interval in seconds (5 minutes)
pub const DEFAULT_CHECK_INTERVAL_SECONDS: u64 = 300;

// Default config file path
pub const DEFAULT_CONFIG_PATH: &str = "/etc/mail-forwarder/config.toml";

impl AppConfig {
    /// Load config from defaults, then file (if exists), then environment variables
    pub fn new() -> Result<Self, ConfigError> {
        Config::builder()
            .add_source(File::with_name(DEFAULT_CONFIG_PATH).required(false))
            .build()?
            .try_deserialize()
    }

    /// Load config from a specific file path
    pub fn new_from_file(path: &str) -> Result<Self, ConfigError> {
        Config::builder()
            .add_source(File::with_name(path).required(true))
            .build()?
            .try_deserialize()
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
            protocol = "pop3"
            use_tls = true
            delete_after_forward = false
        "#;

        let config: AppConfig = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();

        assert_eq!(config.forward_to, "target@example.com");

        assert_eq!(config.sender.host, "smtp.example.com");
        assert_eq!(config.sender.port, 587);

        assert_eq!(config.receivers.len(), 1);
        let receiver = &config.receivers[0];
        assert_eq!(receiver.host, "pop.example.com");
        assert!(receiver.use_tls.unwrap_or(true));
        assert_eq!(receiver.delete_after_forward, Some(false));
    }

    #[test]
    fn test_default_values() {
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
            protocol = "pop3"
            use_tls = true
        "#;

        let _config: AppConfig = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();
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
            protocol = "pop3"
            use_tls = true

            [[receivers]]
            host = "r2"
            port = 2
            username = "u2"
            password = "p2"
            protocol = "imap"
            use_tls = false
        "#;

        let config: AppConfig = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();

        assert_eq!(config.receivers.len(), 2);
        assert_eq!(config.receivers[0].host, "r1");
        assert_eq!(config.receivers[1].host, "r2");
    }

    #[test]
    fn test_invalid_config_type() {
        let toml_str = r#"
            forward_to = 123

            [sender]
            host = "h"
            port = 1
            username = "u"
            password = "p"
        "#;

        let res: Result<AppConfig, _> = Config::builder()
            .add_source(File::from_str(toml_str, FileFormat::Toml))
            .build()
            .unwrap()
            .try_deserialize();
        assert!(res.is_err());
    }
}
