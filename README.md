# Mail Forwarder

A lightweight tool to forward emails from POP3/IMAP accounts to a specified SMTP destination written in Rust. 

Gmail DOES NOT support POP3 or Gmailify for the forwarding email from third-party mail services, for more details, please refer to the [official Gmail documentation](https://support.google.com/mail/answer/16604719). So, I wrote this tool to forward emails from POP3/IMAP accounts to a specified SMTP destination, which can be used with Gmail or any other email service that supports SMTP.

## Features

- Support for **POP3** and **IMAP** protocols.
- Monitor multiple email accounts simultaneously.
- TLS/SSL support.
- Send notifications on successful forwarding via Telegram, file logging, or email.
- Configurable check intervals (in seconds).
- Save emails locally before deleting them from the source server, then retry forwarding before sending failure notifications.

## Run with Docker(recommended)

You can also run the mail forwarder using the precompiled Docker image, which is available on GitHub Container Registry:

```bash
docker pull ghcr.io/mingcheng/mail-forwarder
```

The recommended Docker Compose configuration is:

```yaml
services:
  mail-forwarder:
    image: ghcr.io/mingcheng/mail-forwarder
    network_mode: host
    environment:
      TZ: "Asia/Shanghai"
    volumes:
      - ./data:/app/
      - ./data/config.toml:/app/config.toml:ro # Mount the config file as read-only
```

## Build from source

Make sure you have Rust installed, then clone the repository and build the project:

```bash
cargo build --release
```

then run the binary if you have configured the `config.toml` file.

## Configuration

By default the application loads its configuration from `/etc/mail-forwarder/config.toml`.
You can point it at any other location with the `--config` flag (see [Usage](#usage)).
Create a `config.toml` file with the following content:

```toml
# Destination email address
forward_to = "target@example.com"

# Optional: local spool used to keep a copy before deleting from the source server.
local_mail_dir = "mail-forwarder-spool"
# Optional: number of SMTP forwarding attempts before failure notifications are sent.
forward_retry_attempts = 3

# Optional: Logging configuration
# log_level accepts the same syntax as the RUST_LOG environment variable
# (e.g. "info", "debug", "mail_forwarder=debug"). Defaults to "info".
log_level = "info"
# Optional: write logs to a file (in addition to stderr).
log_file = "mail-forwarder.log"
# Optional: when true, suppress stderr output (only the log file, if any, is written).
quiet = false

# Optional: Notifications when an email is successfully forwarded
[[notifications]]
type = "telegram"
chat_id = "your_telegram_chat_id"
token = "your_telegram_bot_token"

[[notifications]]
type = "file"
file_path = "forwarding_log.txt"

[[notifications]]
type = "email"
smtp_host = "smtp.gmail.com"
smtp_port = 587
smtp_username = "your_email@gmail.com"
smtp_password = "your_email_password"

# SMTP Sender Configuration (for sending forwarded emails)
[sender]
host = "smtp.gmail.com"
port = 587
username = "sender@gmail.com"
password = "app_password" 

# Receiver Example 1: POP3
[[receivers]]
protocol = "pop3"
host = "pop.gmail.com"
port = 995
username = "source1@gmail.com"
password = "app_password"
use_tls = true
check_interval_seconds = 60

# Receiver Example 2: IMAP
[[receivers]]
protocol = "imap"
host = "imap.outlook.com"
port = 993
username = "source2@outlook.com"
password = "app_password"
use_tls = true
imap_folder = "INBOX"
check_interval_seconds = 60
```

> **Note**: For services like Gmail or Outlook, please use an **App Password** instead of your login password for security reasons. You can generate an App Password in your email account settings.

### Configuration reference

| Field                                           | Scope     | Required | Default | Description                                                               |
| ----------------------------------------------- | --------- | -------- | ------- | ------------------------------------------------------------------------- |
| `forward_to`                                    | top-level | yes      | –       | Destination address that all fetched emails are forwarded to.             |
| `local_mail_dir`                                | top-level | no       | `mail-forwarder-spool` | Directory used to keep local `.eml` copies before deleting messages from the source server. |
| `forward_retry_attempts`                        | top-level | no       | `3`     | SMTP forwarding attempts before failure notifications are sent. Values below `1` are treated as `1`. |
| `log_level`                                     | top-level | no       | `info`  | Log verbosity, using `RUST_LOG` syntax. Overrides the `RUST_LOG` env var. |
| `log_file`                                      | top-level | no       | –       | Path to a log file; logs are written there in addition to stderr.         |
| `quiet`                                         | top-level | no       | `false` | Suppress stderr output (only the log file is written, if configured).     |
| `sender.host` / `sender.port`                   | sender    | yes      | –       | SMTP server used to send forwarded emails.                                |
| `sender.username` / `sender.password`           | sender    | yes      | –       | SMTP credentials. The username is also used as the envelope sender.       |
| `sender.use_tls`                                | sender    | no       | `true`  | Use an implicit TLS (wrapper) connection.                                 |
| `receivers[].protocol`                          | receiver  | no       | `pop3`  | `pop3` or `imap`.                                                         |
| `receivers[].host` / `receivers[].port`         | receiver  | yes      | –       | Source mail server.                                                       |
| `receivers[].username` / `receivers[].password` | receiver  | yes      | –       | Source account credentials.                                               |
| `receivers[].use_tls`                           | receiver  | no       | `true`  | Use a TLS connection to the source server.                                |
| `receivers[].check_interval_seconds`            | receiver  | no       | `300`   | Poll interval in seconds (minimum enforced value is `10`).                |
| `receivers[].imap_folder`                       | receiver  | no       | `INBOX` | IMAP mailbox to monitor (ignored for POP3).                               |

> **Note**: Each fetched message is first written to `local_mail_dir`, then deleted from the source server, then forwarded. If all forwarding attempts fail, failure notifications are sent and the local `.eml` copy is retained for manual recovery. If source deletion fails, forwarding still continues and the local copy is kept as a recovery trail.

## Usage

Run the binary:

```bash
./target/release/mail-forwarder

# Or with a specific config file
./target/release/mail-forwarder --config /path/to/config.toml

# Check configuration syntax, SMTP/POP3 account connectivity, and send test notifications
./target/release/mail-forwarder --config /path/to/config.toml --check
```

Account and notification checks time out after 30 seconds each, so one slow server will not block the entire check indefinitely. If notification handlers are configured, `--check` sends a test notification through each one.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
