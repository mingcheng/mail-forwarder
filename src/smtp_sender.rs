use crate::config::SenderConfig;
use crate::traits::{Email, MailSender};
use async_trait::async_trait;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use std::sync::Arc;
use tokio::sync::OnceCell;

// Abstract the mailer so we can mock it
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SmtpMailer: Send + Sync {
    async fn send(&self, envelope: Envelope, email: &[u8]) -> anyhow::Result<()>;
}

// Wrapper for Real Lettre Transport
pub struct RealSmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

#[async_trait]
impl SmtpMailer for RealSmtpMailer {
    async fn send(&self, envelope: Envelope, email: &[u8]) -> anyhow::Result<()> {
        self.transport
            .send_raw(&envelope, email)
            .await
            .map_err(|e| anyhow::anyhow!("SMTP send error: {}", e))
            .map(|_| ())
    }
}

// Factory trait
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

    #[allow(dead_code)]
    pub fn new_with_factory(config: SenderConfig, factory: Arc<dyn SmtpMailerFactory>) -> Self {
        Self {
            config,
            factory,
            mailer: OnceCell::new(),
        }
    }
}

#[async_trait]
impl MailSender for SmtpSender {
    async fn send_email(&self, email: &Email, target_address: &str) -> anyhow::Result<()> {
        let mailer = self
            .mailer
            .get_or_try_init(|| async { self.factory.create(&self.config) })
            .await?;

        // Construct Envelope
        // Sender: the user we login as (to pass SPF checks usually)
        // Recipient: the forwarding target
        let sender_addr = self
            .config
            .username
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid sender address (username): {}", e))?;

        let target_addr = target_address
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid target address: {}", e))?;

        let envelope = Envelope::new(Some(sender_addr), vec![target_addr])
            .map_err(|e| anyhow::anyhow!("Invalid envelope: {}", e))?;

        // Add X-Forwarded-By header
        // Simple prepend approach.
        let mut final_content = Vec::new();
        final_content.extend_from_slice(b"X-Forwarded-By: mail-forwarder\r\n");
        final_content.extend_from_slice(&email.content);

        // Send raw content using our abstraction
        mailer
            .send(envelope, &final_content)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send email: {}", e))?;

        Ok(())
    }
}
