use crate::config::ReceiverConfig;
use crate::traits::{Email, MailReceiver};
use async_trait::async_trait;
use pop3::{Pop3Connection, Pop3ConnectionFactory, Pop3MessageInfo};
use std::sync::Arc;

/// Local trait for POP3 client operations to allow mocking
/// Note: We require 'Send' to allow passing to spawn_blocking.
/// 'Sync' is not strictly required for exclusive ownership in a task, but 'Arc' usage might imply it.
/// For safety with Arc<dyn Factory>, the Factory must be Sync. The Client produced?
/// The client is moved into the thread. So Client must be Send.
#[cfg_attr(test, mockall::automock)]
pub trait Pop3Client: Send + Sync {
    fn list(&mut self) -> anyhow::Result<Vec<Pop3MessageInfo>>;
    fn get_unique_id(&mut self, seq_num: u32) -> anyhow::Result<String>;
    fn retrieve(&mut self, seq_num: u32, content: &mut Vec<u8>) -> anyhow::Result<()>;
    fn delete(&mut self, seq_num: u32) -> anyhow::Result<()>;
}

/// Wrapper for the real POP3 connection
struct RealPop3Client {
    // We require the inner connection to be Send so we can move it to threads
    inner: Box<dyn Pop3Connection + Send + Sync>,
}

impl Pop3Client for RealPop3Client {
    fn list(&mut self) -> anyhow::Result<Vec<Pop3MessageInfo>> {
        self.inner
            .list()
            .map_err(|e| anyhow::anyhow!("List error: {:?}", e))
    }

    fn get_unique_id(&mut self, seq_num: u32) -> anyhow::Result<String> {
        self.inner
            .get_unique_id(seq_num)
            .map_err(|e| anyhow::anyhow!("UIDL error: {:?}", e))
    }

    fn retrieve(&mut self, seq_num: u32, content: &mut Vec<u8>) -> anyhow::Result<()> {
        self.inner
            .retrieve(seq_num, content)
            .map_err(|e| anyhow::anyhow!("Retr error: {:?}", e))
    }

    fn delete(&mut self, seq_num: u32) -> anyhow::Result<()> {
        self.inner
            .delete(seq_num)
            .map_err(|e| anyhow::anyhow!("Delete error: {:?}", e))
    }
}

/// Factory trait for creating POP3 clients
#[cfg_attr(test, mockall::automock)]
pub trait Pop3ClientFactory: Send + Sync {
    fn create(&self, config: &ReceiverConfig) -> anyhow::Result<Box<dyn Pop3Client>>;
}

pub struct RealPop3ClientFactory;

impl Pop3ClientFactory for RealPop3ClientFactory {
    fn create(&self, config: &ReceiverConfig) -> anyhow::Result<Box<dyn Pop3Client>> {
        // We cast to Box<dyn Pop3Connection + Send + Sync>
        // Assuming the library implementation is Send + Sync (usually TcpStream is).
        let mut client: Box<dyn Pop3Connection + Send + Sync> = if config.use_tls {
            let conn = Pop3ConnectionFactory::new(&config.host, config.port)
                .map_err(|e| anyhow::anyhow!("TLS Connection error: {:?}", e))?;
            Box::new(conn)
        } else {
            let conn = Pop3ConnectionFactory::without_tls(&config.host, config.port)
                .map_err(|e| anyhow::anyhow!("Connection error: {:?}", e))?;
            Box::new(conn)
        };

        // Perform login
        client
            .login(&config.username, &config.password)
            .map_err(|e| anyhow::anyhow!("Login error: {:?}", e))?;

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

    /// Helper for injecting a factory (e.g. for testing)
    #[allow(dead_code)]
    pub fn new_with_factory(config: ReceiverConfig, factory: Arc<dyn Pop3ClientFactory>) -> Self {
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
                // Note: msg.message_id is assumed to be u32 based on typical POP3 crate usage
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
                // Try caching or optimizing this?
                // Currently scanning all messages to find the one with matching UID.
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

#[cfg(test)]
mod pop3_receiver_tests {
    use super::*;
    use mockall::predicate::*;

    fn get_test_config() -> ReceiverConfig {
        ReceiverConfig {
            host: "<hostt>".to_string(),
            port: 995,
            username: "<username>".to_string(),
            password: "<password>".to_string(),
            use_tls: true,
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

            // Expect list
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

            // Expect get_unique_id
            mock_client
                .expect_get_unique_id()
                .with(eq(1))
                .returning(|_| Ok("uid1".to_string()));
            mock_client
                .expect_get_unique_id()
                .with(eq(2))
                .returning(|_| Ok("uid2".to_string()));

            // Expect retrieve
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

            // The returned value is Box<dyn Pop3Client>.
            // Since MockPop3Client implements Pop3Client, we call Box::new(mock_client).
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

            // Expect list to find email
            mock_client.expect_list().returning(|| {
                Ok(vec![Pop3MessageInfo {
                    message_id: 10,
                    message_size: 100,
                }])
            });

            // Expect check uid
            mock_client
                .expect_get_unique_id()
                .with(eq(10))
                .returning(move |_| Ok("uid_target".to_string()));

            // Expect delete
            mock_client
                .expect_delete()
                .with(eq(10))
                .returning(|_| Ok(()));

            Ok(Box::new(mock_client))
        });

        // We create the receiver with the mock factory that is set up to find and delete the target email.
        let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));

        // We expect this to succeed since the mock is set up to find and delete the email.
        match receiver.delete_email(target_id).await {
            Ok(_) => println!("Delete succeeded as expected"),
            Err(e) => panic!("Delete failed unexpectedly: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_real_pop3_connection() {
        // Install default crypto provider to avoid panic with rustls 0.23+
        let _ = rustls::crypto::ring::default_provider().install_default();

        let config = get_test_config();
        // Skip this test if we don't have real credentials or network (optional)
        // But the user asked for a "real" test, so we run it.

        let mut receiver = Pop3Receiver::new(config);

        // We expect this to likely fail login if credentials are obfuscated,
        // or succeed if they are real.
        // We just want to ensure it tries to connect without panicking.
        let result = receiver.fetch_emails().await;

        // Print result for debugging
        match &result {
            Ok(emails) => {
                println!("Successfully fetched {} emails", emails.len());
                if emails.len() > 0 {
                    let mail = &emails[0];
                    println!("First email ID: {} {:?}", mail.id, mail.content);
                }
            }
            Err(e) => println!("Fetch failed (expected if creds invalid): {:?}", e),
        }

        // Use result check based on intended credentials
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_emails_connection_error() {
        let config = get_test_config();

        // We set up the mock factory to simulate a connection failure when creating the client.
        let mut mock_factory = MockPop3ClientFactory::new();
        mock_factory
            .expect_create()
            .returning(|_| Err(anyhow::anyhow!("Connection failed")));

        // We create the receiver with the mock factory that simulates a connection failure.
        let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));
        let result = receiver.fetch_emails().await;

        assert!(result.is_err());
    }
}
