use crate::config::SenderConfig;
use crate::traits::{Email, MailSender};
use crate::smtp_sender::{SmtpSender, MockSmtpMailerFactory, MockSmtpMailer};
use std::sync::Arc;
use mockall::predicate::*;

#[tokio::test]
async fn test_send_email_success() {
    let config = SenderConfig {
        host: "smtp.test.com".to_string(),
        port: 587,
        username: "sender@test.com".to_string(),
        password: "pass".to_string(),
        proxy: None,
    };

    let mut mock_factory = MockSmtpMailerFactory::new();
    mock_factory.expect_create()
        .returning(|_| {
            let mut mock_mailer = MockSmtpMailer::new();
            mock_mailer.expect_send()
                .times(1)
                .withf(|envelope, content| {
                     // Check content starts with X-Forwarded-By
                     let content_str = String::from_utf8_lossy(content);
                     if !content_str.starts_with("X-Forwarded-By: mail-forwarder") {
                         return false;
                     }
                     
                     if let Some(sender) = envelope.from() {
                         if sender.to_string() != "sender@test.com" { return false; }
                     } else {
                         return false; 
                     }
                     
                     if envelope.to().len() != 1 { return false; }
                     if envelope.to()[0].to_string() != "target@example.com" { return false; }

                     true
                })
                .returning(|_, _| Ok(()));
            Ok(Box::new(mock_mailer))
        });

    let sender = SmtpSender::new_with_factory(config, Arc::new(mock_factory));
    let email = Email {
        id: "1".to_string(),
        content: b"Subject: Existing Content".to_vec(),
    };

    let result = sender.send_email(&email, "target@example.com").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_email_factory_error() {
     let config = SenderConfig {
        host: "smtp.test.com".to_string(),
        port: 587,
        username: "sender@test.com".to_string(),
        password: "pass".to_string(),
        proxy: None,
    };

    let mut mock_factory = MockSmtpMailerFactory::new();
    mock_factory.expect_create()
        .returning(|_| Err(anyhow::anyhow!("Connection failed")));

    let sender = SmtpSender::new_with_factory(config, Arc::new(mock_factory));
    let email = Email { id: "1".to_string(), content: vec![] };

    let result = sender.send_email(&email, "target@example.com").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "Connection failed");
}
