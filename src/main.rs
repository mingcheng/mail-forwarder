mod config;
mod pop3_receiver;
mod smtp_sender;
mod traits;

use config::AppConfig;
use log::{error, info};
use pop3_receiver::Pop3Receiver;
use smtp_sender::SmtpSender;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use traits::{MailReceiver, MailSender};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger with default level info
    if std::env::var("RUST_LOG").is_err() {
        // SAFETY: This is safe because it's single-threaded at this point during initialization,
        // and we control the environment. However, set_var is technically unsafe in 2024 edition.
        // A better approach is directly configuring env_logger builder, but for now:
        unsafe {
            std::env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::init();

    // Load configuration
    // In a real run, ensure you have a config.toml or ENV vars set
    let config = match AppConfig::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {:?}", e);
            eprintln!("Please create a `config.toml` or set APP_... environment variables.");
            // Return Ok to exit gracefully instead of panic
            return Ok(());
        }
    };

    info!("Starting Mail Forwarder...");
    info!("Forwarding to: {}", config.forward_to);

    // Create a shared sender
    let sender = Arc::new(SmtpSender::new(config.sender.clone()));

    let mut handles = vec![];

    for receiver_config in config.receivers {
        let sender = sender.clone();
        let forward_to = config.forward_to.clone();
        let default_interval = config.check_interval_seconds;

        let handle = tokio::spawn(async move {
            let host = receiver_config.host.clone();
            let username = receiver_config.username.clone();
            let interval = receiver_config
                .check_interval_seconds
                .unwrap_or(default_interval);

            info!(
                "Starting task for {}:{} ({})",
                host, receiver_config.port, username
            );

            let mut receiver: Box<dyn MailReceiver> =
                Box::new(Pop3Receiver::new(receiver_config.clone()));
            let mut seen_ids: HashSet<String> = HashSet::new();
            let delete_after_forward = receiver_config.delete_after_forward.unwrap_or(false);

            loop {
                // info!("Checking for new emails for {}...", username); // Optional verbose log

                match receiver.fetch_emails().await {
                    Ok(emails) => {
                        for email in emails {
                            if seen_ids.contains(&email.id) {
                                continue;
                            }

                            info!("[{}] Processing new email ID: {}", username, email.id);

                            match sender.send_email(&email, &forward_to).await {
                                Ok(_) => {
                                    info!(
                                        "[{}] Successfully forwarded email {}",
                                        username, email.id
                                    );
                                    seen_ids.insert(email.id.clone());

                                    if delete_after_forward {
                                        if let Err(e) = receiver.delete_email(&email.id).await {
                                            error!(
                                                "[{}] Failed to delete email {}: {:?}",
                                                username, email.id, e
                                            );
                                        } else {
                                            info!(
                                                "[{}] Deleted email from server: {}",
                                                username, email.id
                                            );
                                        }
                                    }
                                }

                                Err(e) => {
                                    error!(
                                        "[{}] Failed to forward email {}: {:?}",
                                        username, email.id, e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("[{}] Error fetching emails: {:?}", username, e);
                    }
                }

                tokio::time::sleep(Duration::from_secs(interval)).await;
            }
        });

        handles.push(handle);
    }

    // Wait for all tasks (they essentially run forever)
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}
