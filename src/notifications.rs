/*!
 * Copyright (c) 2026 Ming Lyu, aka mingcheng
 *
 * This source code is licensed under the MIT License,
 * which is located in the LICENSE file in the source tree's root directory.
 *
 * File: notifications.rs
 * Author: mingcheng <mingcheng@apache.org>
 * File Created: 2026-02-27 15:47:40
 *
 * Modified By: mingcheng <mingcheng@apache.org>
 * Last Modified: 2026-02-27 16:30:53
 */

use crate::config::NotificationConfig;
use crate::traits::{Email, Notification};
use async_trait::async_trait;
use log::{error, info};
use reqwest::Client;
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// A notification handler that sends a message via Telegram Bot API.
pub struct TelegramNotification {
    chat_id: String,
    token: String,
    client: Client,
    api_url: String,
}

impl TelegramNotification {
    pub fn new(chat_id: String, token: String) -> Self {
        Self {
            chat_id,
            token,
            client: Client::new(),
            api_url: "https://api.telegram.org".to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_api_url(chat_id: String, token: String, api_url: String) -> Self {
        Self {
            chat_id,
            token,
            client: Client::new(),
            api_url,
        }
    }
}

#[async_trait]
impl Notification for TelegramNotification {
    async fn notify(&self, email: &Email, target_address: &str) -> anyhow::Result<()> {
        let url = format!("{}/bot{}/sendMessage", self.api_url, self.token);
        let message = format!(
            "Email forwarded successfully!\nID: {}\nTarget: {}",
            email.id, target_address
        );

        let payload = serde_json::json!({
            "chat_id": self.chat_id,
            "text": message,
        });

        let response = self.client.post(&url).json(&payload).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Telegram API error: {} - {}", status, text);
            return Err(anyhow::anyhow!("Telegram API error: {}", status));
        }

        info!("Telegram notification sent for email {}", email.id);
        Ok(())
    }
}

/// A notification handler that appends a log entry to a local file.
pub struct FileNotification {
    file_path: String,
    // Use a mutex to prevent concurrent writes to the same file from multiple tasks
    lock: Arc<Mutex<()>>,
}

impl FileNotification {
    pub fn new(file_path: String) -> Self {
        Self {
            file_path,
            lock: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait]
impl Notification for FileNotification {
    async fn notify(&self, email: &Email, target_address: &str) -> anyhow::Result<()> {
        let _guard = self.lock.lock().await;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await?;

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let log_entry = format!(
            "[{}] Forwarded email ID: {} to {}\n",
            timestamp, email.id, target_address
        );

        file.write_all(log_entry.as_bytes()).await?;
        info!("File notification written for email {}", email.id);
        Ok(())
    }
}

use lettre::Message;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

/// A notification handler that sends an email via SMTP.
pub struct EmailNotification {
    smtp_username: String,
    mailer: AsyncSmtpTransport<Tokio1Executor>,
}

impl EmailNotification {
    pub fn new(
        smtp_host: String,
        smtp_port: u16,
        smtp_username: String,
        smtp_password: String,
    ) -> anyhow::Result<Self> {
        let creds = Credentials::new(smtp_username.clone(), smtp_password);
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp_host)?
            .port(smtp_port)
            .credentials(creds)
            .build();

        Ok(Self {
            smtp_username,
            mailer,
        })
    }
}

#[async_trait]
impl Notification for EmailNotification {
    async fn notify(&self, email: &Email, target_address: &str) -> anyhow::Result<()> {
        let email_message = Message::builder()
            .from(self.smtp_username.parse()?)
            .to(self.smtp_username.parse()?) // Send to self as notification
            .subject(format!("Notification: Email {} forwarded", email.id))
            .header(ContentType::TEXT_PLAIN)
            .body(format!(
                "Email with ID {} was successfully forwarded to {}.",
                email.id, target_address
            ))?;

        self.mailer.send(email_message).await?;
        info!("Email notification sent for email {}", email.id);
        Ok(())
    }
}

/// Factory function to create a list of notification handlers based on the provided configuration.
pub fn create_notifications(configs: &[NotificationConfig]) -> Vec<Box<dyn Notification>> {
    let mut notifications: Vec<Box<dyn Notification>> = Vec::new();

    for config in configs {
        match config {
            NotificationConfig::Telegram { chat_id, token } => {
                notifications.push(Box::new(TelegramNotification::new(
                    chat_id.clone(),
                    token.clone(),
                )));
            }
            NotificationConfig::File { file_path } => {
                notifications.push(Box::new(FileNotification::new(file_path.clone())));
            }
            NotificationConfig::Email {
                smtp_host,
                smtp_port,
                smtp_username,
                smtp_password,
            } => {
                match EmailNotification::new(
                    smtp_host.clone(),
                    *smtp_port,
                    smtp_username.clone(),
                    smtp_password.clone(),
                ) {
                    Ok(notification) => notifications.push(Box::new(notification)),
                    Err(e) => error!("Failed to create EmailNotification: {}", e),
                }
            }
        }
    }

    notifications
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tokio::fs;

    #[tokio::test]
    async fn test_file_notification() {
        let temp_dir = env::temp_dir();
        let file_path = temp_dir.join("test_notification.log");
        let file_path_str = file_path.to_str().unwrap().to_string();

        // Ensure file doesn't exist before test
        let _ = fs::remove_file(&file_path).await;

        let notification = FileNotification::new(file_path_str.clone());
        let email = Email {
            id: "test-email-123".to_string(),
            content: vec![],
        };

        let result = notification.notify(&email, "target@example.com").await;
        assert!(result.is_ok());

        // Verify file contents
        let contents = fs::read_to_string(&file_path).await.unwrap();
        assert!(contents.contains("Forwarded email ID: test-email-123 to target@example.com"));

        // Clean up
        let _ = fs::remove_file(&file_path).await;
    }

    #[test]
    fn test_create_notifications() {
        let configs = vec![
            NotificationConfig::Telegram {
                chat_id: "123".to_string(),
                token: "abc".to_string(),
            },
            NotificationConfig::File {
                file_path: "test.log".to_string(),
            },
            NotificationConfig::Email {
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                smtp_username: "user".to_string(),
                smtp_password: "password".to_string(),
            },
        ];

        let notifications = create_notifications(&configs);
        assert_eq!(notifications.len(), 3);
    }

    #[tokio::test]
    async fn test_telegram_notification_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/botTEST_TOKEN/sendMessage")
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create_async()
            .await;

        let notification = TelegramNotification::with_api_url(
            "TEST_CHAT_ID".to_string(),
            "TEST_TOKEN".to_string(),
            server.url(),
        );

        let email = Email {
            id: "test-email-123".to_string(),
            content: vec![],
        };

        let result = notification.notify(&email, "target@example.com").await;
        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_telegram_notification_failure() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/botTEST_TOKEN/sendMessage")
            .with_status(400)
            .with_body(r#"{"ok":false,"description":"Bad Request"}"#)
            .create_async()
            .await;

        let notification = TelegramNotification::with_api_url(
            "TEST_CHAT_ID".to_string(),
            "TEST_TOKEN".to_string(),
            server.url(),
        );

        let email = Email {
            id: "test-email-123".to_string(),
            content: vec![],
        };

        let result = notification.notify(&email, "target@example.com").await;
        assert!(result.is_err());
        mock.assert_async().await;
    }
}
