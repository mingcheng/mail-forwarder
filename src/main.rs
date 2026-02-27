/*!
 * Copyright (c) 2026 Ming Lyu, aka mingcheng
 *
 * This source code is licensed under the MIT License,
 * which is located in the LICENSE file in the source tree's root directory.
 *
 * File: main.rs
 * Author: mingcheng <mingcheng@apache.org>
 * File Created: 2026-02-12 15:38:23
 *
 * Modified By: mingcheng <mingcheng@apache.org>
 * Last Modified: 2026-02-27 16:23:51
 */

mod config;
mod imap_receiver;
mod notifications;
mod pop3_receiver;
mod smtp_sender;
mod traits;

use clap::Parser;
use config::{AppConfig, DEFAULT_CHECK_INTERVAL_SECONDS, ReceiverConfig};
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
use traits::{MailReceiver, MailSender, Notification};

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

/// Context required for processing a batch of fetched emails.
/// This struct groups together all the necessary dependencies and state
/// to avoid passing too many arguments to the `process_emails` function.
struct ProcessContext<'a> {
    /// The username of the receiver account, used for logging.
    username: &'a str,
    /// The SMTP sender instance used to forward emails.
    sender: &'a SmtpSender,
    /// The target email address to forward to.
    forward_to: &'a str,
    /// The receiver instance, used to delete emails from the server if configured.
    receiver: &'a mut dyn MailReceiver,
    /// A set of email IDs that have already been processed, to prevent duplicate processing.
    seen_ids: &'a mut HashSet<String>,
    /// Whether to delete emails from the source server after successful forwarding.
    delete_after_forward: bool,
    /// A list of notification handlers to trigger after successful processing.
    notifications: &'a [Box<dyn Notification>],
}

/// Processes a batch of fetched emails.
///
/// This function handles the core logic of:
/// 1. Forwarding each new email via SMTP.
/// 2. Tracking successfully forwarded emails.
/// 3. Deleting successfully forwarded emails from the source server (if configured).
/// 4. Triggering notifications for successfully processed emails.
async fn process_emails(ctx: &mut ProcessContext<'_>, emails: Vec<traits::Email>) {
    let mut to_delete = Vec::new();
    let mut successfully_processed = Vec::new();

    // Step 1 & 2: Forward emails and track successes
    for email in emails {
        // Skip if we've already seen this email ID in the current session
        if ctx.seen_ids.contains(&email.id) {
            continue;
        }

        info!("[{}] Processing new email ID: {}", ctx.username, email.id);

        match ctx.sender.send_email(&email, ctx.forward_to).await {
            Ok(_) => {
                info!(
                    "[{}] Successfully forwarded email {}",
                    ctx.username, email.id
                );
                // Mark as seen to avoid reprocessing
                ctx.seen_ids.insert(email.id.clone());
                // Queue for deletion if forwarding succeeded
                to_delete.push(email.id.clone());
                // Queue for notification
                successfully_processed.push(email);
            }
            Err(e) => {
                error!(
                    "[{}] Failed to forward email {}: {:?}",
                    ctx.username, email.id, e
                );
            }
        }
    }

    // Step 3: Delete emails from the source server if configured
    let notify_emails = if ctx.delete_after_forward && !to_delete.is_empty() {
        match ctx.receiver.delete_emails(&to_delete).await {
            Ok(_) => {
                info!(
                    "[{}] Successfully deleted {} emails from server",
                    ctx.username,
                    to_delete.len()
                );
                // Remove deleted emails from seen_ids to prevent memory leaks over time
                for id in &to_delete {
                    ctx.seen_ids.remove(id);
                }
                // Only notify if deletion also succeeded
                successfully_processed
            }
            Err(e) => {
                error!("[{}] Failed to delete emails: {:?}", ctx.username, e);
                // Skip notifications if deletion fails, to avoid false positives
                // or duplicate notifications if the email is fetched again later.
                Vec::new()
            }
        }
    } else {
        // If deletion is not configured, notify for all successfully forwarded emails
        successfully_processed
    };

    // Step 4: Trigger notifications
    for email in notify_emails {
        for notification in ctx.notifications {
            if let Err(e) = notification.notify(&email, ctx.forward_to).await {
                error!(
                    "[{}] Failed to send notification for email {}: {:?}",
                    ctx.username, email.id, e
                );
            }
        }
    }
}

/// Runs the main loop for a single email receiver account.
///
/// This task periodically polls the source server for new emails,
/// processes them, and handles graceful shutdown.
async fn run_receiver_task(
    receiver_config: ReceiverConfig,
    sender: Arc<SmtpSender>,
    forward_to: String,
    notifications: Arc<Vec<Box<dyn Notification>>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
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

    // Note: For POP3, if delete_after_forward is false, restarting the program
    // will cause all existing emails to be forwarded again because seen_ids is not persisted.
    // For IMAP, it only fetches UNSEEN emails, so it's less of an issue.
    // Also, if delete_after_forward is false, seen_ids will grow indefinitely,
    // which could be a memory leak for long-running processes with many emails.
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

        match receiver.fetch_emails(&seen_ids).await {
            Ok(emails) => {
                let mut ctx = ProcessContext {
                    username: &username,
                    sender: &sender,
                    forward_to: &forward_to,
                    receiver: receiver.as_mut(),
                    seen_ids: &mut seen_ids,
                    delete_after_forward,
                    notifications: &notifications,
                };
                process_emails(&mut ctx, emails).await;
            }
            Err(e) => {
                error!("[{}] Error fetching emails: {:?}", username, e);
            }
        }
    }
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

    let notifications = Arc::new(notifications::create_notifications(&config.notifications));
    let sender = Arc::new(SmtpSender::new(config.sender.clone()));
    let (shutdown_tx, _) = broadcast::channel(1);
    let mut handles = vec![];

    for receiver_config in config.receivers {
        let sender = sender.clone();
        let forward_to = config.forward_to.clone();
        let notifications = notifications.clone();
        let shutdown_rx = shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            run_receiver_task(
                receiver_config,
                sender,
                forward_to,
                notifications,
                shutdown_rx,
            )
            .await;
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
