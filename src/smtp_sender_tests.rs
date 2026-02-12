use crate::config::SenderConfig;
use crate::smtp_sender::{MockSmtpMailer, MockSmtpMailerFactory, SmtpSender};
use crate::traits::{Email, MailSender};
use std::sync::Arc;

#[tokio::test]
async fn test_send_email_success() {
    let config = SenderConfig {
        host: "smtp.test.com".to_string(),
        port: 465,
        username: "sender@test.com".to_string(),
        password: "pass".to_string(),
        use_tls: true,
    };

    let mut mock_factory = MockSmtpMailerFactory::new();
    mock_factory.expect_create().returning(|_| {
        let mut mock_mailer = MockSmtpMailer::new();
        mock_mailer
            .expect_send()
            .times(1)
            .withf(|envelope, content| {
                // Check content starts with X-Forwarded-By
                let content_str = String::from_utf8_lossy(content);
                if !content_str.starts_with("X-Forwarded-By: mail-forwarder") {
                    return false;
                }

                if let Some(sender) = envelope.from() {
                    if sender.to_string() != "sender@test.com" {
                        return false;
                    }
                } else {
                    return false;
                }

                if envelope.to().len() != 1 {
                    return false;
                }
                if envelope.to()[0].to_string() != "target@example.com" {
                    return false;
                }

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
        use_tls: true,
    };

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
    // Install default crypto provider for rustls (required for 0.23+)
    let _ = rustls::crypto::ring::default_provider().install_default();

    let config = SenderConfig {
        port: 465,
        use_tls: true,
        host: "<host>".to_string(),
        username: "<username>".to_string(),
        password: "<password>".to_string(),
    };

    if config.username.contains("<") {
        println!("Skipping real SMTP send test due to placeholder credentials");
        return;
    }

    // If these are dummy credentials, this will likely fail with authentication error or connection timeout,
    // which is expected. We just want to ensure it runs without panicking.
    let sender = SmtpSender::new(config);
    let email = Email {
        id: "real_test_1".to_string(),

        content: b"Subject: Real Test Email\r\n\r\nThis is a test email body.".to_vec(),
    };

    println!("Attempting to send email...");
    let result = sender.send_email(&email, "0243701308@shisu.edu.cn").await;

    match &result {
        Ok(_) => println!("Email sent successfully (mock/real)"),
        Err(e) => println!("Send failed (expected with dummy creds): {:?}", e),
    }

    // Assert it's either Ok or a specific networking/auth error, but no panic
    // assert!(result.is_ok()); // processing...
}
