use crate::config::ReceiverConfig;
use crate::traits::{Email, MailReceiver};
use async_trait::async_trait;
use log::warn;
use pop3::{Pop3Connection, Pop3ConnectionFactory, Pop3MessageInfo, Pop3MessageUidInfo};

// Factory trait for creating connections
#[cfg_attr(test, mockall::automock)]
pub trait Pop3ConnectionFactoryTrait: Send + Sync {
    fn create(&self, config: &ReceiverConfig) -> anyhow::Result<Box<dyn Pop3Connection>>;
}

pub struct RealPop3ConnectionFactory;

impl Pop3ConnectionFactoryTrait for RealPop3ConnectionFactory {
    fn create(&self, config: &ReceiverConfig) -> anyhow::Result<Box<dyn Pop3Connection>> {
        if config.proxy.is_some() {
            warn!(
                "POP3 Proxy is currently not supported with the updated client library. Connecting directly."
            );
        }

        let mut client: Box<dyn Pop3Connection> = if config.use_tls {
            let conn = Pop3ConnectionFactory::new(&config.host, config.port)
                .map_err(|e| anyhow::anyhow!("TLS Connection error: {:?}", e))?;
            Box::new(conn)
        } else {
            let conn = Pop3ConnectionFactory::without_tls(&config.host, config.port)
                .map_err(|e| anyhow::anyhow!("Connection error: {:?}", e))?;
            Box::new(conn)
        };

        client
            .login(&config.username, &config.password)
            .map_err(|e| anyhow::anyhow!("Login error: {:?}", e))?;

        Ok(client)
    }
}

use std::sync::Arc;

#[cfg(test)]
#[path = "./pop3_receiver_tests.rs"]
mod pop3_receiver_tests;

pub struct Pop3Receiver {
    config: ReceiverConfig,
    factory: Arc<dyn Pop3ConnectionFactoryTrait>,
}

impl Pop3Receiver {
    pub fn new(config: ReceiverConfig) -> Self {
        Self {
            config,
            factory: Arc::new(RealPop3ConnectionFactory),
        }
    }

    // For testing
    #[allow(dead_code)]
    pub fn new_with_factory(
        config: ReceiverConfig,
        factory: Arc<dyn Pop3ConnectionFactoryTrait>,
    ) -> Self {
        Self { config, factory }
    }
}

#[async_trait]
impl MailReceiver for Pop3Receiver {
    async fn fetch_emails(&mut self) -> anyhow::Result<Vec<Email>> {
        let config = self.config.clone();
        let factory = self.factory.clone();

        // Execute blocking POP3 operations in a generic blocking thread
        let emails = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Email>> {
            let mut client = factory.create(&config)?;

            // List messages
            let list = client
                .list()
                .map_err(|e| anyhow::anyhow!("List error: {:?}", e))?;

            let mut emails = Vec::new();

            for msg in list {
                // Get UIDL for unique ID
                let uid = client
                    .get_unique_id(msg.message_id)
                    .unwrap_or_else(|_| msg.message_id.to_string());

                // Retrieve content
                let mut content = Vec::new();
                client
                    .retrieve(msg.message_id, &mut content)
                    .map_err(|e| anyhow::anyhow!("Retr error: {:?}", e))?;

                emails.push(Email { id: uid, content });
            }

            // Pop3Connection trait doesn't have quit, it drops connection on drop

            Ok(emails)
        })
        .await??;

        Ok(emails)
    }

    async fn delete_email(&mut self, id: &str) -> anyhow::Result<()> {
        let config = self.config.clone();
        let target_uid = id.to_string();
        let factory = self.factory.clone();

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            // Reconnect to delete
            let mut client = factory.create(&config)?;

            let list = client
                .list()
                .map_err(|e| anyhow::anyhow!("List error during delete: {:?}", e))?;

            let mut found_num = None;
            for msg in list {
                if let Ok(uid) = client.get_unique_id(msg.message_id) {
                    if uid == target_uid {
                        found_num = Some(msg.message_id);
                        break;
                    }
                } else if msg.message_id.to_string() == target_uid {
                    found_num = Some(msg.message_id);
                    break;
                }
            }

            if let Some(num) = found_num {
                client
                    .delete(num)
                    .map_err(|e| anyhow::anyhow!("Delete error: {:?}", e))?;
            } else {
                return Err(anyhow::anyhow!(
                    "Message with ID {} not found for deletion",
                    target_uid
                ));
            }

            Ok(())
        })
        .await??;

        Ok(())
    }
}
