mod config;
mod pop3_receiver;
mod smtp_sender;
mod traits;

use clap::Parser;
use config::{AppConfig, DEFAULT_CHECK_INTERVAL_SECONDS};
use log::{error, info, warn};
use pop3_receiver::Pop3Receiver;
use rustls::crypto;
use smtp_sender::SmtpSender;
use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;
use traits::{MailReceiver, MailSender};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install default crypto provider to avoid panic with rustls 0.23+
    let _ = crypto::ring::default_provider().install_default();

    // Parse command line arguments
    let args = Args::parse();

    // Load configuration
    let config_result = match args.config {
        Some(path) => AppConfig::new_from_file(&path),
        None => AppConfig::new(),
    };

    let config = match config_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {:?}", e);
            if let Ok(path) = std::env::current_dir() {
                eprintln!("Current search path: {:?}", path);
            }
            eprintln!(
                "Please create a `config.toml` or set APP_... environment variables, or specify a config file with --config."
            );
            // Return Ok to exit gracefully instead of panic
            return Ok(());
        }
    };

    // Initialize Logger
    let mut builder = env_logger::Builder::new();

    // Log Level: Config > Env Var > Default Info
    if let Some(level) = &config.log_level {
        builder.parse_filters(level);
    } else if let Ok(env_level) = std::env::var("RUST_LOG") {
        builder.parse_filters(&env_level);
    } else {
        builder.filter_level(log::LevelFilter::Info);
    }

    // Log Target Configuration
    if let Some(log_file) = &config.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|e| anyhow::anyhow!("Failed to open log file {}: {}", log_file, e))?;

        if config.quiet {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        } else {
            // Log to both file and stderr
            struct MultiWriter {
                writers: Vec<Box<dyn Write + Send + 'static>>,
            }

            impl Write for MultiWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    for w in &mut self.writers {
                        let _ = w.write(buf);
                    }
                    Ok(buf.len())
                }

                fn flush(&mut self) -> std::io::Result<()> {
                    for w in &mut self.writers {
                        let _ = w.flush();
                    }
                    Ok(())
                }
            }

            let multi_writer = MultiWriter {
                writers: vec![Box::new(file), Box::new(std::io::stderr())],
            };
            builder.target(env_logger::Target::Pipe(Box::new(multi_writer)));
        }
    } else if config.quiet {
        // No log file and quiet requested -> suppress output
        builder.target(env_logger::Target::Pipe(Box::new(std::io::sink())));
    }

    builder.init();

    info!("Starting Mail Forwarder...");
    info!("Forwarding to: {}", config.forward_to);

    // Create a shared sender
    let sender = Arc::new(SmtpSender::new(config.sender.clone()));

    // Create a broadcast channel for graceful shutdown signal
    let (shutdown_tx, _) = broadcast::channel(1);

    let mut handles = vec![];

    for receiver_config in config.receivers {
        let sender = sender.clone();
        let forward_to = config.forward_to.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            let host = receiver_config.host.clone();
            let username = receiver_config.username.clone();
            let interval_seconds = receiver_config
                .check_interval_seconds
                .unwrap_or(DEFAULT_CHECK_INTERVAL_SECONDS);

            // Ensure interval is at least 10 seconds to avoid spamming
            let interval_seconds = std::cmp::max(interval_seconds, 10);

            info!(
                "Starting task for {}:{} ({}) - Interval: {}s",
                host, receiver_config.port, username, interval_seconds
            );

            let mut receiver: Box<dyn MailReceiver> =
                Box::new(Pop3Receiver::new(receiver_config.clone()));
            let mut seen_ids: HashSet<String> = HashSet::new();
            let delete_after_forward = receiver_config.delete_after_forward.unwrap_or(false);

            // Use tokio::time::interval for consistent timing
            let mut ticker = tokio::time::interval(Duration::from_secs(interval_seconds));
            // First tick completes immediately
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("[{}] Received shutdown signal. Stopping task...", username);
                        break;
                    }
                    _ = ticker.tick() => {
                        // Continue to fetch emails
                    }
                }

                // info!("Checking for new emails for {}...", username); // Optional verbose log

                match receiver.fetch_emails().await {
                    Ok(emails) => {
                        for email in emails {
                            if seen_ids.contains(&email.id) {
                                continue;
                            }

                            info!("[{}] Processing new email ID: {}", username, email.id);

                            // Send email
                            // If shutdown signal is received during sending, we probably want to finish sending this one?
                            // tokio::spawn is not cancelled automatically, but we are inside the select loop's body.
                            // The select only waits for the next tick or shutdown signal *before* starting this block
                            // or after this block finishes.
                            // So once we are here, we will finish processing the batch unless we check shutdown again.

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
            }
        });

        handles.push(handle);
    }

    // Wait for Ctrl+C
    match signal::ctrl_c().await {
        Ok(()) => {
            warn!("Shutdown signal received (Ctrl+C). notifying tasks...");
        }
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Send shutdown signal to all tasks
    let _ = shutdown_tx.send(());

    // Wait for all tasks to finish
    info!("Waiting for {} tasks to finish...", handles.len());
    for handle in handles {
        let _ = handle.await;
    }

    info!("All tasks stopped. Goodbye!");

    Ok(())
}
