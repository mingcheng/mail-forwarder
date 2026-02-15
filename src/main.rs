mod config;
mod imap_receiver;
mod pop3_receiver;
mod smtp_sender;
mod traits;

use clap::Parser;
use config::{AppConfig, DEFAULT_CHECK_INTERVAL_SECONDS};
use imap_receiver::ImapReceiver;
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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    config: Option<String>,
}

fn initialize_logger(config: &AppConfig) -> anyhow::Result<()> {
    let mut builder = env_logger::Builder::new();

    if let Some(level) = &config.log_level {
        builder.parse_filters(level);
    } else if let Ok(env_level) = std::env::var("RUST_LOG") {
        builder.parse_filters(&env_level);
    } else {
        builder.filter_level(log::LevelFilter::Info);
    }

    if let Some(log_file) = &config.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .map_err(|e| anyhow::anyhow!("Failed to open log file {}: {}", log_file, e))?;

        if config.quiet {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        } else {
            let multi_writer = MultiWriter {
                writers: vec![Box::new(file), Box::new(std::io::stderr())],
            };
            builder.target(env_logger::Target::Pipe(Box::new(multi_writer)));
        }
    } else if config.quiet {
        builder.target(env_logger::Target::Pipe(Box::new(std::io::sink())));
    }

    builder.init();
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = crypto::ring::default_provider().install_default();

    let args = Args::parse();

    let config = match args.config {
        Some(path) => AppConfig::new_from_file(&path),
        None => AppConfig::new(),
    }
    .unwrap_or_else(|e| {
        eprintln!("Failed to load config: {:?}", e);
        if let Ok(path) = std::env::current_dir() {
            eprintln!("Current search path: {:?}", path);
        }
        eprintln!("Please create a `config.toml` or set APP_... environment variables, or specify a config file with --config.");
        std::process::exit(1);
    });

    initialize_logger(&config)?;

    info!("Starting Mail Forwarder...");
    info!("Forwarding to: {}", config.forward_to);

    let sender = Arc::new(SmtpSender::new(config.sender.clone()));
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
                .unwrap_or(DEFAULT_CHECK_INTERVAL_SECONDS)
                .max(10);

            info!(
                "Starting task for {}:{} ({}) - Protocol: {} - Interval: {}s",
                host, receiver_config.port, username, receiver_config.protocol, interval_seconds
            );

            #[allow(clippy::wildcard_in_or_patterns)]
            let mut receiver: Box<dyn MailReceiver> = match receiver_config.protocol.as_str() {
                "imap" => Box::new(ImapReceiver::new(receiver_config.clone())),
                "pop3" | _ => Box::new(Pop3Receiver::new(receiver_config.clone())),
            };
            let mut seen_ids: HashSet<String> = HashSet::new();
            let delete_after_forward = receiver_config.delete_after_forward.unwrap_or(false);

            let mut ticker = tokio::time::interval(Duration::from_secs(interval_seconds));
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("[{}] Received shutdown signal. Stopping task...", username);
                        break;
                    }
                    _ = ticker.tick() => {}
                }

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
            }
        });

        handles.push(handle);
    }

    match signal::ctrl_c().await {
        Ok(()) => warn!("Shutdown signal received (Ctrl+C). Notifying tasks..."),
        Err(err) => error!("Unable to listen for shutdown signal: {}", err),
    }

    let _ = shutdown_tx.send(());

    info!("Waiting for {} tasks to finish...", handles.len());
    for handle in handles {
        let _ = handle.await;
    }

    info!("All tasks stopped. Goodbye!");
    Ok(())
}
