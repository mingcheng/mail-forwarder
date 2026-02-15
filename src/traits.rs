/*!
 * Copyright (c) 2026 Ming Lyu, aka mingcheng
 *
 * This source code is licensed under the MIT License,
 * which is located in the LICENSE file in the source tree's root directory.
 *
 * File: traits.rs
 * Author: mingcheng <mingcheng@apache.org>
 * File Created: 2026-02-11 16:14:32
 *
 * Modified By: mingcheng <mingcheng@apache.org>
 * Last Modified: 2026-02-15 14:28:07
 */

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Email {
    pub id: String,
    pub content: Vec<u8>,
}

#[async_trait]
pub trait MailReceiver: Send + Sync {
    /// Connects/Authenticates and fetches pending emails
    async fn fetch_emails(&mut self) -> anyhow::Result<Vec<Email>>;

    /// Optional: Delete an email after processing
    async fn delete_email(&mut self, id: &str) -> anyhow::Result<()>;
}

#[async_trait]
pub trait MailSender: Send + Sync {
    /// Sends an email content to a specific recipient
    async fn send_email(&self, email: &Email, target_address: &str) -> anyhow::Result<()>;
}
