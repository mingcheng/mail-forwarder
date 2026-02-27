/*!
 * Copyright (c) 2026 Ming Lyu, aka mingcheng
 *
 * This source code is licensed under the MIT License,
 * which is located in the LICENSE file in the source tree's root directory.
 *
 * File: smtp_sender.rs
 * Author: mingcheng <mingcheng@apache.org>
 * File Created: 2026-02-12 22:37:25
 *
 * Modified By: mingcheng <mingcheng@apache.org>
 * Last Modified: 2026-02-27 16:31:17
 */

use crate::config::SenderConfig;
use crate::traits::{Email, MailSender};
use async_trait::async_trait;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::sync::Arc;
use tokio::sync::OnceCell;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SmtpMailer: Send + Sync {
    async fn send(&self, envelope: Envelope, email: &[u8]) -> anyhow::Result<()>;
}

struct RealSmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

#[async_trait]
impl SmtpMailer for RealSmtpMailer {
    async fn send(&self, envelope: Envelope, email: &[u8]) -> anyhow::Result<()> {
        self.transport
            .send_raw(&envelope, email)
            .await
            .map_err(|e| anyhow::anyhow!("SMTP send failed: {}", e))?;
        Ok(())
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait SmtpMailerFactory: Send + Sync {
    fn create(&self, config: &SenderConfig) -> anyhow::Result<Box<dyn SmtpMailer>>;
}

pub struct RealSmtpMailerFactory;

impl SmtpMailerFactory for RealSmtpMailerFactory {
    fn create(&self, config: &SenderConfig) -> anyhow::Result<Box<dyn SmtpMailer>> {
        let creds = Credentials::new(config.username.clone(), config.password.clone());
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            .map_err(|e| anyhow::anyhow!("Invalid SMTP host: {}", e))?
            .port(config.port)
            .credentials(creds);

        if config.use_tls.unwrap_or(true) {
            let tls_params = TlsParameters::new(config.host.clone())
                .map_err(|e| anyhow::anyhow!("Invalid TLS parameters: {}", e))?;
            builder = builder.tls(Tls::Wrapper(tls_params));
        } else {
            builder = builder.tls(Tls::None);
        }

        let transport = builder.build();

        Ok(Box::new(RealSmtpMailer { transport }))
    }
}

#[cfg(test)]
#[path = "./smtp_sender_tests.rs"]
mod smtp_sender_tests;

pub struct SmtpSender {
    config: SenderConfig,
    factory: Arc<dyn SmtpMailerFactory>,
    mailer: OnceCell<Box<dyn SmtpMailer>>,
}

impl SmtpSender {
    pub fn new(config: SenderConfig) -> Self {
        Self {
            config,
            factory: Arc::new(RealSmtpMailerFactory),
            mailer: OnceCell::new(),
        }
    }

    #[cfg(test)]
    pub fn new_with_factory(config: SenderConfig, factory: Arc<dyn SmtpMailerFactory>) -> Self {
        Self {
            config,
            factory,
            mailer: OnceCell::new(),
        }
    }

    fn create_envelope(&self, target_address: &str) -> anyhow::Result<Envelope> {
        let sender_addr = self
            .config
            .username
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid sender address: {}", e))?;
        let target_addr = target_address
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid target address: {}", e))?;

        Envelope::new(Some(sender_addr), vec![target_addr])
            .map_err(|e| anyhow::anyhow!("Invalid envelope: {}", e))
    }
}

#[async_trait]
impl MailSender for SmtpSender {
    async fn send_email(&self, email: &Email, target_address: &str) -> anyhow::Result<()> {
        let mailer = self
            .mailer
            .get_or_try_init(|| async { self.factory.create(&self.config) })
            .await?;

        let envelope = self.create_envelope(target_address)?;

        let mut final_content = Vec::with_capacity(email.content.len() + 32);

        // Add custom headers
        final_content.extend_from_slice(b"X-Forwarded-By: mail-forwarder\r\n");

        final_content.extend_from_slice(b"X-Original-Message-ID: ");
        final_content.extend_from_slice(email.id.as_bytes());
        final_content.extend_from_slice(b"\r\n");

        final_content.extend_from_slice(b"X-Forwarded-Time: ");
        final_content.extend_from_slice(chrono::Utc::now().to_rfc3339().as_bytes());
        final_content.extend_from_slice(b"\r\n");

        final_content.extend_from_slice(&email.content);

        mailer.send(envelope, &final_content).await
    }
}
