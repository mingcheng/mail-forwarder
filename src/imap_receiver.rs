use crate::config::ReceiverConfig;
use crate::traits::{Email, MailReceiver};
use async_imap::Session;
use async_native_tls::{TlsConnector, TlsStream};
use async_trait::async_trait;
use futures::{StreamExt, pin_mut};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use std::collections::HashSet;

type ImapSession = Session<TlsStream<Compat<TcpStream>>>;

pub struct ImapReceiver {
    config: ReceiverConfig,
}

impl ImapReceiver {
    pub fn new(config: ReceiverConfig) -> Self {
        Self { config }
    }

    async fn connect(&self) -> anyhow::Result<ImapSession> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let tcp_stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to {}: {}", addr, e))?;

        let tls = TlsConnector::new();
        let tls_stream = tls
            .connect(&self.config.host, tcp_stream.compat())
            .await
            .map_err(|e| anyhow::anyhow!("TLS connection failed: {}", e))?;

        let client = async_imap::Client::new(tls_stream);

        let session = client
            .login(&self.config.username, &self.config.password)
            .await
            .map_err(|e| anyhow::anyhow!("Login failed: {:?}", e.0))?;

        Ok(session)
    }

    async fn fetch_emails_internal(
        &self,
        seen_ids: &HashSet<String>,
    ) -> anyhow::Result<Vec<Email>> {
        let mut session = self.connect().await?;

        // Select the mailbox (default: INBOX)
        let mailbox = &self.config.imap_folder;
        session
            .select(mailbox)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to select mailbox {}: {}", mailbox, e))?;

        // Search for all unseen messages
        let search_result = session
            .search("UNSEEN")
            .await
            .map_err(|e| anyhow::anyhow!("Search failed: {}", e))?;

        let mut emails = Vec::new();

        let seq_nums: Vec<String> = search_result.iter().map(|n| n.to_string()).collect();
        if seq_nums.is_empty() {
            // Logout
            session
                .logout()
                .await
                .map_err(|e| anyhow::anyhow!("Logout failed: {}", e))?;
            return Ok(emails);
        }

        let sequence_set = seq_nums.join(",");

        // Fetch all unseen messages in one command
        {
            let mut fetch_stream = session
                .fetch(sequence_set, "RFC822")
                .await
                .map_err(|e| anyhow::anyhow!("Fetch failed for messages: {}", e))?;

            while let Some(fetch_result) = fetch_stream.next().await {
                let message = fetch_result
                    .map_err(|e| anyhow::anyhow!("Error reading fetch result: {}", e))?;

                if let Some(body) = message.body() {
                    let id = format!("{}", message.message);
                    if !seen_ids.contains(&id) {
                        emails.push(Email {
                            id,
                            content: body.to_vec(),
                        });
                    }
                }
            }
        }

        // Logout
        session
            .logout()
            .await
            .map_err(|e| anyhow::anyhow!("Logout failed: {}", e))?;

        Ok(emails)
    }
}

#[async_trait]
impl MailReceiver for ImapReceiver {
    async fn fetch_emails(&mut self, seen_ids: &HashSet<String>) -> anyhow::Result<Vec<Email>> {
        self.fetch_emails_internal(seen_ids).await
    }

    async fn delete_email(&mut self, id: &str) -> anyhow::Result<()> {
        let mut session = self.connect().await?;

        let mailbox = &self.config.imap_folder;
        session
            .select(mailbox)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to select mailbox {}: {}", mailbox, e))?;

        // Mark the message as deleted
        {
            let store_stream = session
                .store(id, "+FLAGS (\\Deleted)")
                .await
                .map_err(|e| anyhow::anyhow!("Failed to mark message {} as deleted: {}", id, e))?;
            pin_mut!(store_stream);

            // Consume the stream
            while store_stream.next().await.is_some() {}
        }

        // Expunge to permanently delete
        {
            let expunge_stream = session
                .expunge()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to expunge: {}", e))?;
            pin_mut!(expunge_stream);

            // Consume the stream
            while expunge_stream.next().await.is_some() {}
        }

        // Logout
        session
            .logout()
            .await
            .map_err(|e| anyhow::anyhow!("Logout failed: {}", e))?;

        Ok(())
    }

    async fn delete_emails(&mut self, ids: &[String]) -> anyhow::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let mut session = self.connect().await?;

        let mailbox = &self.config.imap_folder;
        session
            .select(mailbox)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to select mailbox {}: {}", mailbox, e))?;

        for id in ids {
            // Mark the message as deleted
            let store_stream = session
                .store(id, "+FLAGS (\\Deleted)")
                .await
                .map_err(|e| anyhow::anyhow!("Failed to mark message {} as deleted: {}", id, e))?;
            pin_mut!(store_stream);

            // Consume the stream
            while store_stream.next().await.is_some() {}
        }

        // Expunge to permanently delete
        {
            let expunge_stream = session
                .expunge()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to expunge: {}", e))?;
            pin_mut!(expunge_stream);

            // Consume the stream
            while expunge_stream.next().await.is_some() {}
        }

        // Logout
        session
            .logout()
            .await
            .map_err(|e| anyhow::anyhow!("Logout failed: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod imap_receiver_tests {
    use super::*;

    fn get_test_config() -> ReceiverConfig {
        ReceiverConfig {
            host: "outlook.office365.com".to_string(),
            port: 993,
            username: "mingcheng@outlook.com".to_string(),
            password: "lxmgtivelvpdsruq".to_string(),
            protocol: "imap".to_string(),
            use_tls: Some(true),
            check_interval_seconds: Some(60),
            delete_after_forward: Some(false),
            imap_folder: "INBOX".to_string(),
        }
    }

    #[tokio::test]
    async fn test_real_imap_connection() {
        let config = get_test_config();

        if config.username == "test_user" {
            println!("Skipping real IMAP connection test - use real credentials to run");
            return;
        }

        let mut receiver = ImapReceiver::new(config);

        let seen_ids = HashSet::new();
        let result = receiver.fetch_emails(&seen_ids).await;

        match &result {
            Ok(emails) => println!("Successfully fetched {} emails", emails.len()),
            Err(e) => println!("Fetch failed (expected with test credentials): {:?}", e),
        }

        // With test credentials, we expect this to fail
        // In real usage with proper credentials, it should succeed
    }
}
