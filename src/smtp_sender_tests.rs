use crate::config::SenderConfig;
use crate::smtp_sender::{MockSmtpMailer, MockSmtpMailerFactory, SmtpSender};
use crate::traits::{Email, MailSender};
use std::sync::Arc;

fn test_sender_config() -> SenderConfig {
    SenderConfig {
        host: "smtp.test.com".to_string(),
        port: 465,
        username: "sender@test.com".to_string(),
        password: "pass".to_string(),
        use_tls: Some(true),
    }
}

#[tokio::test]
async fn test_send_email_success() {
    let config = test_sender_config();

    let mut mock_factory = MockSmtpMailerFactory::new();
    mock_factory.expect_create().returning(|_| {
        let mut mock_mailer = MockSmtpMailer::new();
        mock_mailer
            .expect_send()
            .times(1)
            .withf(|envelope, content| {
                let content_str = String::from_utf8_lossy(content);
                content_str.starts_with("X-Forwarded-By: mail-forwarder")
                    && envelope
                        .from()
                        .is_some_and(|s| s.to_string() == "sender@test.com")
                    && envelope.to().len() == 1
                    && envelope.to()[0].to_string() == "target@example.com"
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
    let config = test_sender_config();

    let mut mock_factory = MockSmtpMailerFactory::new();
    mock_factory
        .expect_create()
        .returning(|_| Err(anyhow::anyhow!("Connection failed")));

    let sender = SmtpSender::new_with_factory(config, Arc::new(mock_factory));
    let email = Email {
        id: "1".to_string(),
        content: vec![],
    };

    let result = sender.send_email(&email, "target@example.com").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "Connection failed");
}

#[tokio::test]
async fn test_real_smtp_send() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = SenderConfig {
        host: "smtp.example.com".to_string(),
        port: 465,
        username: "test_user".to_string(),
        password: "test_pass".to_string(),
        use_tls: Some(true),
    };

    if config.username == "test_user" {
        println!("Skipping real SMTP send test - use real credentials to run");
        return;
    }
    let sender = SmtpSender::new(config);
    let email = Email {
        id: "real_test_1".to_string(),
        content: b"Subject: Real Test Email\r\n\r\nThis is a test email body.".to_vec(),
    };

    let result = sender.send_email(&email, "target@example.com").await;

    match &result {
        Ok(_) => println!("Email sent successfully"),
        Err(e) => println!("Send failed (expected with test credentials): {:?}", e),
    }

    assert!(result.is_ok());
}
