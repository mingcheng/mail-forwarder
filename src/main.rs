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
 * Last Modified: 2026-06-25 15:34:27
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
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::signal;
use tokio::sync::broadcast;
use traits::{MailReceiver, MailSender, Notification};

const ACCOUNT_CHECK_TIMEOUT_SECONDS: u64 = 30;
#[cfg(unix)]
async fn wait_for_shutdown_signal() -> anyhow::Result<&'static str> {
    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
    let mut sigquit = signal::unix::signal(signal::unix::SignalKind::quit())?;

    tokio::select! {
        result = signal::ctrl_c() => {
            result?;
            Ok("SIGINT/Ctrl+C")
        }
        _ = sigterm.recv() => Ok("SIGTERM"),
        _ = sigquit.recv() => Ok("SIGQUIT"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> anyhow::Result<&'static str> {
    signal::ctrl_c().await?;
    Ok("Ctrl+C")
}

// A simple writer that duplicates writes to multiple underlying writers.
struct MultiWriter {
    writers: Vec<Box<dyn Write + Send + 'static>>,
}

// Implement the Write trait for MultiWriter to forward writes to all underlying writers.
impl Write for MultiWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut first_error = None;

        for w in &mut self.writers {
            if let Err(err) = w.write_all(buf) {
                first_error.get_or_insert(err);
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut first_error = None;

        for w in &mut self.writers {
            if let Err(err) = w.flush() {
                first_error.get_or_insert(err);
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }

        Ok(())
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, help = "Path to the configuration file")]
    config: Option<String>,

    #[arg(long, help = "Check the configuration file and account connections")]
    check: bool,
}

// Initializes the logger based on the provided configuration.
fn initialize_logger(config: &AppConfig) -> anyhow::Result<()> {
    let mut builder = env_logger::Builder::new();

    // Determine log level: config > RUST_LOG env var > default to INFO
    if let Some(level) = &config.log_level {
        builder.parse_filters(level);
    } else if let Ok(env_level) = std::env::var("RUST_LOG") {
        builder.parse_filters(&env_level);
    } else {
        builder.filter_level(log::LevelFilter::Info);
    }

    // If a log file is specified, write logs to that file. If quiet mode is enabled, only write to the file and not stderr.
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

async fn check_account_with_timeout<F>(label: &str, check: F) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    match tokio::time::timeout(Duration::from_secs(ACCOUNT_CHECK_TIMEOUT_SECONDS), check).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(anyhow::anyhow!("{} check failed: {}", label, e)),
        Err(_) => Err(anyhow::anyhow!(
            "{} check timed out after {} seconds",
            label,
            ACCOUNT_CHECK_TIMEOUT_SECONDS
        )),
    }
}

async fn check_config(config: &AppConfig) -> anyhow::Result<()> {
    config
        .forward_to
        .parse::<lettre::Address>()
        .map_err(|e| anyhow::anyhow!("Invalid forward_to address: {}", e))?;

    info!("Checking SMTP sender account {}", config.sender.username);
    let sender = SmtpSender::new(config.sender.clone());
    check_account_with_timeout("SMTP sender account", sender.check_connection()).await?;

    notifications::check_email_notification_accounts(
        &config.notifications,
        Duration::from_secs(ACCOUNT_CHECK_TIMEOUT_SECONDS),
    )
    .await?;

    notifications::send_test_notifications(
        &config.notifications,
        &config.forward_to,
        Duration::from_secs(ACCOUNT_CHECK_TIMEOUT_SECONDS),
    )
    .await?;

    for receiver_config in &config.receivers {
        if receiver_config.protocol == "pop3" {
            info!(
                "Checking POP3 receiver account {} at {}:{}",
                receiver_config.username, receiver_config.host, receiver_config.port
            );
            let receiver = Pop3Receiver::new(receiver_config.clone());
            let label = format!("POP3 receiver account {}", receiver_config.username);
            check_account_with_timeout(&label, receiver.check_connection()).await?;
        }
    }

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
    /// The receiver instance, used to delete emails from the server after local save.
    receiver: &'a mut dyn MailReceiver,
    /// A set of email IDs that have already been processed, to prevent duplicate processing.
    seen_ids: &'a mut HashSet<String>,
    /// Directory used to keep local copies before deleting from the source server.
    local_mail_dir: &'a str,
    /// Number of SMTP forwarding attempts before failure notifications are sent.
    forward_retry_attempts: u32,
    /// A list of notification handlers to trigger after successful processing.
    notifications: &'a [Box<dyn Notification>],
}

fn safe_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' => c,
            _ => '_',
        })
        .collect()
}

fn local_email_path(local_mail_dir: &str, username: &str, email_id: &str) -> PathBuf {
    PathBuf::from(local_mail_dir)
        .join(safe_path_segment(username))
        .join(format!("{}.eml", safe_path_segment(email_id)))
}

async fn save_email_locally(
    local_mail_dir: &str,
    username: &str,
    email: &traits::Email,
) -> anyhow::Result<PathBuf> {
    let path = local_email_path(local_mail_dir, username, &email.id);
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid local mail path {}", path.display()))?;

    fs::create_dir_all(parent).await?;

    let temporary_path = path.with_extension("eml.tmp");
    fs::write(&temporary_path, &email.content).await?;
    fs::rename(&temporary_path, &path).await?;

    Ok(path)
}

async fn send_email_with_retry(
    sender: &SmtpSender,
    email: &traits::Email,
    forward_to: &str,
    attempts: u32,
) -> Result<(), anyhow::Error> {
    let attempts = attempts.max(1);
    let mut last_error = None;

    for attempt in 1..=attempts {
        match sender.send_email(email, forward_to).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                error!(
                    "Failed to forward email {} on attempt {}/{}: {:?}",
                    email.id, attempt, attempts, e
                );
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Forwarding was not attempted")))
}

async fn notify_forward_failure(ctx: &ProcessContext<'_>, email: &traits::Email, error: &str) {
    for notification in ctx.notifications {
        if let Err(e) = notification
            .notify_forward_failure(email, ctx.forward_to, error)
            .await
        {
            error!(
                "[{}] Failed to send failure notification for email {}: {:?}",
                ctx.username, email.id, e
            );
        }
    }
}

/// Processes a batch of fetched emails.
///
/// This function handles the core logic of:
/// 1. Saving each new email locally.
/// 2. Deleting saved emails from the source server.
/// 3. Forwarding local copies with retry.
/// 4. Triggering success or failure notifications.
async fn process_emails(ctx: &mut ProcessContext<'_>, emails: Vec<traits::Email>) {
    for email in emails {
        if ctx.seen_ids.contains(&email.id) {
            continue;
        }

        info!("[{}] Processing new email ID: {}", ctx.username, email.id);

        let local_path = match save_email_locally(ctx.local_mail_dir, ctx.username, &email).await {
            Ok(path) => path,
            Err(e) => {
                error!(
                    "[{}] Failed to save email {} locally, leaving it on the source server: {:?}",
                    ctx.username, email.id, e
                );
                continue;
            }
        };

        ctx.seen_ids.insert(email.id.clone());

        let mut source_delete_failed = false;
        match ctx.receiver.delete_email(&email.id).await {
            Ok(()) => info!(
                "[{}] Deleted email {} from source server after local save",
                ctx.username, email.id
            ),
            Err(e) => {
                error!(
                    "[{}] Failed to delete email {} from source server after local save: {:?}",
                    ctx.username, email.id, e
                );
                source_delete_failed = true;
            }
        }

        match send_email_with_retry(
            ctx.sender,
            &email,
            ctx.forward_to,
            ctx.forward_retry_attempts,
        )
        .await
        {
            Ok(()) => {
                info!(
                    "[{}] Successfully forwarded email {}",
                    ctx.username, email.id
                );
                if source_delete_failed {
                    warn!(
                        "[{}] Keeping local copy {} because source deletion failed before forwarding",
                        ctx.username,
                        local_path.display()
                    );
                } else {
                    if let Err(e) = fs::remove_file(&local_path).await {
                        warn!(
                            "[{}] Failed to remove local copy {} after successful forwarding: {:?}",
                            ctx.username,
                            local_path.display(),
                            e
                        );
                    }
                }

                for notification in ctx.notifications {
                    if let Err(e) = notification.notify(&email, ctx.forward_to).await {
                        error!(
                            "[{}] Failed to send notification for email {}: {:?}",
                            ctx.username, email.id, e
                        );
                    }
                }
            }
            Err(e) => {
                error!(
                    "[{}] Failed to forward email {} after {} attempts; local copy retained at {}: {:?}",
                    ctx.username,
                    email.id,
                    ctx.forward_retry_attempts.max(1),
                    local_path.display(),
                    e
                );
                notify_forward_failure(ctx, &email, &e.to_string()).await;
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
    local_mail_dir: String,
    forward_retry_attempts: u32,
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

    let mut receiver: Box<dyn MailReceiver> = match receiver_config.protocol.as_str() {
        "imap" => Box::new(ImapReceiver::new(receiver_config.clone())),
        "pop3" => Box::new(Pop3Receiver::new(receiver_config.clone())),
        protocol => {
            warn!(
                "Unknown receiver protocol '{}'; falling back to POP3",
                protocol
            );
            Box::new(Pop3Receiver::new(receiver_config.clone()))
        }
    };

    // Messages are saved locally before deletion from the source server. If source deletion fails,
    // the local copy is retained after forwarding so the operator has a recovery trail.
    let mut seen_ids: HashSet<String> = HashSet::new();

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

        let fetch_result = tokio::select! {
            result = receiver.fetch_emails(&seen_ids) => result,
            _ = shutdown_rx.recv() => {
                info!("[{}] Received shutdown signal while fetching emails. Stopping task...", username);
                break;
            }
        };

        match fetch_result {
            Ok(emails) => {
                let mut ctx = ProcessContext {
                    username: &username,
                    sender: &sender,
                    forward_to: &forward_to,
                    receiver: receiver.as_mut(),
                    seen_ids: &mut seen_ids,
                    local_mail_dir: &local_mail_dir,
                    forward_retry_attempts,
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
    // Ensure the Rustls crypto provider is initialized before any async tasks start.
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
        eprintln!("Please create a config file at the default path or specify one with --config.");
        std::process::exit(1);
    });

    initialize_logger(&config)?;

    if args.check {
        match check_config(&config).await {
            Ok(()) => {
                println!("Configuration file and account checks passed.");
                return Ok(());
            }
            Err(e) => {
                eprintln!("Configuration check failed: {:?}", e);
                std::process::exit(1);
            }
        }
    }

    info!("Starting Mail Forwarder...");
    info!("Forwarding to: {}", config.forward_to);

    let notifications = Arc::new(notifications::create_notifications(&config.notifications));
    let sender = Arc::new(SmtpSender::new(config.sender.clone()));
    let (shutdown_tx, _) = broadcast::channel(1);
    let mut handles = vec![];

    for receiver_config in config.receivers {
        let sender = sender.clone();
        let forward_to = config.forward_to.clone();
        let local_mail_dir = config.local_mail_dir.clone();
        let forward_retry_attempts = config.forward_retry_attempts.max(1);
        let notifications = notifications.clone();
        let shutdown_rx = shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            run_receiver_task(
                receiver_config,
                sender,
                forward_to,
                local_mail_dir,
                forward_retry_attempts,
                notifications,
                shutdown_rx,
            )
            .await;
        });

        handles.push(handle);
    }

    match wait_for_shutdown_signal().await {
        Ok(signal_name) => warn!(
            "Shutdown signal received ({}). Notifying tasks...",
            signal_name
        ),
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
