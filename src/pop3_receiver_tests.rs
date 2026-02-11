use crate::config::ReceiverConfig;
use crate::traits::MailReceiver;
use std::sync::Arc;
use mockall::predicate::*;
use pop3::{Pop3Connection, Pop3Stat, Pop3MessageInfo, Pop3MessageUidInfo};
use std::io::Write;
use anyhow::Result;
use crate::pop3_receiver::{Pop3Receiver, MockPop3ConnectionFactoryTrait};

// Manually mock Pop3Connection because it's in another crate and we want to return Box<dyn Pop3Connection>
mockall::mock! {
    pub Pop3Conn {}

    impl Pop3Connection for Pop3Conn {
            fn login<'a>(&mut self, user: &'a str, password: &'a str) -> Result<(), Box<dyn std::error::Error>>;
            fn stat(&mut self) -> Result<Pop3Stat, Box<dyn std::error::Error>>;
            fn list(&mut self) -> Result<Vec<Pop3MessageInfo>, Box<dyn std::error::Error>>;
            fn get_message_size(&mut self, message_id: u32) -> Result<u32, Box<dyn std::error::Error>>;
            fn retrieve(&mut self, message_id: u32, writer: &mut dyn Write) -> Result<(), Box<dyn std::error::Error>>;
            fn delete(&mut self, message_id: u32) -> Result<(), Box<dyn std::error::Error>>;
            fn reset(&mut self) -> Result<(), Box<dyn std::error::Error>>;
            fn top(&mut self, message_id: u32, line_count: u32) -> Result<String, Box<dyn std::error::Error>>;
            fn list_unique_ids(&mut self) -> Result<Vec<Pop3MessageUidInfo>, Box<dyn std::error::Error>>;
            fn get_unique_id(&mut self, message_id: u32) -> Result<String, Box<dyn std::error::Error>>;
    }
}

#[tokio::test]
async fn test_fetch_emails_success() {
    let config = ReceiverConfig {
        host: "pop3.test.com".to_string(),
        port: 110,
        username: "user".to_string(),
        password: "pass".to_string(),
        use_tls: false,
        check_interval_seconds: None,
        proxy: None,
        delete_after_forward: None,
    };

    let mut mock_factory = MockPop3ConnectionFactoryTrait::new();
    mock_factory.expect_create()
        .returning(|_| {
            let mut mock_conn = MockPop3Conn::new();
            
            // Expect list
            mock_conn.expect_list()
                .times(1)
                .returning(|| Ok(vec![
                    Pop3MessageInfo { message_id: 1, message_size: 100 },
                    Pop3MessageInfo { message_id: 2, message_size: 200 }
                ]));

            // Expect UIDL retrieval
            mock_conn.expect_get_unique_id()
                .with(eq(1))
                .returning(|_| Ok("UID1".to_string()));
            mock_conn.expect_get_unique_id()
                .with(eq(2))
                .returning(|_| Ok("UID2".to_string()));
            
            // Expect Retrieve content
            mock_conn.expect_retrieve()
                .with(eq(1), always())
                .returning(|_, w| {
                    w.write_all(b"Subject: Test 1")?;
                    Ok(())
                });
            mock_conn.expect_retrieve()
                .with(eq(2), always())
                .returning(|_, w| {
                    w.write_all(b"Subject: Test 2")?;
                    Ok(())
                });
            

            Ok(Box::new(mock_conn))
        });

    let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));
    
    // Execute
    let emails = receiver.fetch_emails().await.unwrap();
    
    // Assertions
    assert_eq!(emails.len(), 2);
    assert_eq!(emails[0].id, "UID1");
}

#[tokio::test]
async fn test_delete_email_success() {
        let config = ReceiverConfig {
        host: "pop3.test.com".to_string(),
        port: 110,
        username: "user".to_string(),
        password: "pass".to_string(),
        use_tls: false,
        check_interval_seconds: None,
        proxy: None,
        delete_after_forward: None,
    };

    let mut mock_factory = MockPop3ConnectionFactoryTrait::new();
    mock_factory.expect_create()
        .returning(|_| {
            let mut mock_conn = MockPop3Conn::new();
            
            // Reconnect happens in delete_email, so list again to find ID by UID
            mock_conn.expect_list()
                .times(1)
                .returning(|| Ok(vec![
                    Pop3MessageInfo { message_id: 5, message_size: 500 }
                ]));

            // UIDL check to match "TARGET_UID" to id 5
            mock_conn.expect_get_unique_id()
                .with(eq(5))
                .returning(|_| Ok("TARGET_UID".to_string()));
            
            // Logic performs delete
            mock_conn.expect_delete()
                .with(eq(5))
                .times(1)
                .returning(|_| Ok(()));
            

            Ok(Box::new(mock_conn))
        });

    let mut receiver = Pop3Receiver::new_with_factory(config, Arc::new(mock_factory));
    
    let result = receiver.delete_email("TARGET_UID").await;
    assert!(result.is_ok());
}
