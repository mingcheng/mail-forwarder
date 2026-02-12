use crate::config::ReceiverConfig;
use crate::traits::{Email, MailReceiver};
use async_trait::async_trait;
use pop3::{Pop3Connection, Pop3ConnectionFactory, Pop3MessageInfo};
use std::sync::Arc;

#[cfg_attr(test, mockall::automock)]
pub trait Pop3Client: Send + Sync {
    fn list(&mut self) -> anyhow::Result<Vec<Pop3MessageInfo>>;
    fn get_unique_id(&mut self, seq_num: u32) -> anyhow::Result<String>;
    fn retrieve(&mut self, seq_num: u32, content: &mut Vec<u8>) -> anyhow::Result<()>;
    fn delete(&mut self, seq_num: u32) -> anyhow::Result<()>;
}

struct RealPop3Client {
    inner: Box<dyn Pop3Connection + Send + Sync>,
}

impl Pop3Client for RealPop3Client {
    fn list(&mut self) -> anyhow::Result<Vec<Pop3MessageInfo>> {
        self.inner
            .list()
            .map_err(|e| anyhow::anyhow!("List failed: {:?}", e))
    }

    fn get_unique_id(&mut self, seq_num: u32) -> anyhow::Result<String> {
        self.inner
            .get_unique_id(seq_num)
            .map_err(|e| anyhow::anyhow!("UIDL failed: {:?}", e))
    }

    fn retrieve(&mut self, seq_num: u32, content: &mut Vec<u8>) -> anyhow::Result<()> {
        self.inner
            .retrieve(seq_num, content)
            .map_err(|e| anyhow::anyhow!("Retrieve failed: {:?}", e))
    }

    fn delete(&mut self, seq_num: u32) -> anyhow::Result<()> {
        self.inner
            .delete(seq_num)
            .map_err(|e| anyhow::anyhow!("Delete failed: {:?}", e))
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait Pop3ClientFactory: Send + Sync {
    fn create(&self, config: &ReceiverConfig) -> anyhow::Result<Box<dyn Pop3Client>>;
}

pub struct RealPop3ClientFactory;

impl Pop3ClientFactory for RealPop3ClientFactory {
    fn create(&self, config: &ReceiverConfig) -> anyhow::Result<Box<dyn Pop3Client>> {
        let mut client: Box<dyn Pop3Connection + Send + Sync> = if config.use_tls.unwrap_or(true) {
            Box::new(
                Pop3ConnectionFactory::new(&config.host, config.port)
                    .map_err(|e| anyhow::anyhow!("TLS connection failed: {:?}", e))?,
            )
        } else {
            Box::new(
                Pop3ConnectionFactory::without_tls(&config.host, config.port)
                    .map_err(|e| anyhow::anyhow!("Connection failed: {:?}", e))?,
            )
        };

        client
            .login(&config.username, &config.password)
            .map_err(|e| anyhow::anyhow!("Login failed: {:?}", e))?;

        Ok(Box::new(RealPop3Client { inner: client }))
    }
}

pub struct Pop3Receiver {
    config: ReceiverConfig,
    factory: Arc<dyn Pop3ClientFactory>,
}

impl Pop3Receiver {
    pub fn new(config: ReceiverConfig) -> Self {
        Self {
            config,
            factory: Arc::new(RealPop3ClientFactory),
        }
    }

    #[cfg(test)]
    pub fn new_with_factory(config: ReceiverConfig, factory: Arc<dyn Pop3ClientFactory>) -> Self {
        Self { config, factory }
    }
}

#[async_trait]
impl MailReceiver for Pop3Receiver {
    async fn fetch_emails(&mut self) -> anyhow::Result<Vec<Email>> {
        let config = self.config.clone();
        let factory = self.factory.clone();

        tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Email>> {
            let mut client = factory.create(&config)?;
            let list = client.list()?;
            let mut emails = Vec::new();

            for msg in list {
                let uid = client
                    .get_unique_id(msg.message_id)
                    .unwrap_or_else(|_| msg.message_id.to_string());

                let mut content = Vec::new();
                client.retrieve(msg.message_id, &mut content)?;

                emails.push(Email { id: uid, content });
            }

            Ok(emails)
        })
        .await?
    }

    async fn delete_email(&mut self, id: &str) -> anyhow::Result<()> {
        let config = self.config.clone();
        let target_uid = id.to_string();
        let factory = self.factory.clone();

        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut client = factory.create(&config)?;
            let list = client.list()?;

            for msg in list {
                let uid = client
                    .get_unique_id(msg.message_id)
                    .unwrap_or_else(|_| msg.message_id.to_string());

                if uid == target_uid {
                    client.delete(msg.message_id)?;
                    return Ok(());
                }
            }

            Err(anyhow::anyhow!("Message with ID {} not found", target_uid))
        })
        .await?
    }
}

#[cfg(test)]
mod pop3_receiver_tests {
    use super::*;
    use mockall::predicate::*;

    fn get_test_config() -> ReceiverConfig {
        ReceiverConfig {
            host: "pop.example.com".to_string(),
            port: 995,
            username: "test_user".to_string(),
            password: "test_pass".to_string(),
            use_tls: Some(true),
            check_interval_seconds: Some(60),
            delete_after_forward: Some(false),
        }
    }

    #[tokio::test]
    async fn test_fetch_emails_success() {
        let config = get_test_config();

        let mut mock_factory = MockPop3ClientFactory::new();
        mock_factory.expect_create().returning(|_| {
            let mut mock_client = MockPop3Client::new();

            mock_client.expect_list().returning(|| {
                Ok(vec![
                    Pop3MessageInfo {
                        message_id: 1,
                        message_size: 100,
                    },
                    Pop3MessageInfo {
                        message_id: 2,
                        message_size: 200,
                    },
                ])
            });

            mock_client
                .expect_get_unique_id()
                .with(eq(1))
                .returning(|_| Ok("uid1".to_string()));
            mock_client
                .expect_get_unique_id()
                .with(eq(2))
                .returning(|_| Ok("uid2".to_string()));

            mock_client
                .expect_retrieve()
                .with(eq(1), always())
                .returning(|_, buf| {
                    buf.extend_from_slice(b"email1");
                    Ok(())
                });
            mock_client
                .expect_retrieve()
                .with(eq(2), always())
                .returning(|_, buf| {
                    buf.extend_from_slice(b"email2");
                    Ok(())
                });

            Ok(Box::new(mock_client))
        });

        let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));
        let emails = receiver.fetch_emails().await.unwrap();

        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0].id, "uid1");
        assert_eq!(emails[0].content, b"email1");
        assert_eq!(emails[1].id, "uid2");
    }

    #[tokio::test]
    async fn test_delete_email_success() {
        let config = get_test_config();
        let target_id = "uid_target";

        let mut mock_factory = MockPop3ClientFactory::new();
        mock_factory.expect_create().returning(move |_| {
            let mut mock_client = MockPop3Client::new();

            mock_client.expect_list().returning(|| {
                Ok(vec![Pop3MessageInfo {
                    message_id: 10,
                    message_size: 100,
                }])
            });

            mock_client
                .expect_get_unique_id()
                .with(eq(10))
                .returning(move |_| Ok("uid_target".to_string()));

            mock_client
                .expect_delete()
                .with(eq(10))
                .returning(|_| Ok(()));

            Ok(Box::new(mock_client))
        });

        let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));
        receiver
            .delete_email(target_id)
            .await
            .expect("Delete should succeed");
    }

    #[tokio::test]
    async fn test_real_pop3_connection() {
        let _ = rustls::crypto::ring::default_provider().install_default();

        let config = get_test_config();

        if config.username == "test_user" {
            println!("Skipping real POP3 connection test - use real credentials to run");
            return;
        }

        let mut receiver = Pop3Receiver::new(config);

        let result = receiver.fetch_emails().await;

        match &result {
            Ok(emails) => println!("Successfully fetched {} emails", emails.len()),
            Err(e) => println!("Fetch failed (expected with test credentials): {:?}", e),
        }

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_emails_connection_error() {
        let config = get_test_config();

        let mut mock_factory = MockPop3ClientFactory::new();
        mock_factory
            .expect_create()
            .returning(|_| Err(anyhow::anyhow!("Connection failed")));

        let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));
        let result = receiver.fetch_emails().await;

        assert!(result.is_err());
    }
}
