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
