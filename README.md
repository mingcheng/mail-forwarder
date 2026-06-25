# Mail Forwarder

A lightweight tool to forward emails from POP3/IMAP accounts to a specified SMTP destination written in Rust. 

Gmail DOES NOT support POP3 or Gmailify for the forwarding email from third-party mail services, for more details, please refer to the [official Gmail documentation](https://support.google.com/mail/answer/16604719). So, I wrote this tool to forward emails from POP3/IMAP accounts to a specified SMTP destination, which can be used with Gmail or any other email service that supports SMTP.

## Features

- Support for **POP3** and **IMAP** protocols.
- Monitor multiple email accounts simultaneously.
- TLS/SSL support.
- Send notifications on successful forwarding via Telegram, file logging, or email.
- Configurable check intervals (in seconds).

## Run with Docker(recommended)

You can also run the mail forwarder using per-compiled docker image, which is available on GitHub Container Registry:

```bash
docker pull ghcr.io/mingcheng/mail-forwarder
```

and suggest run it in the docker compose:

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
delete_after_forward = false

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

> **Note**: For services like Gmail or Outlook, please use an **App Password** instead of your login password for the security reasons. You can generate an App Password in your email account settings.

### Configuration reference

| Field                                           | Scope     | Required | Default | Description                                                               |
| ----------------------------------------------- | --------- | -------- | ------- | ------------------------------------------------------------------------- |
| `forward_to`                                    | top-level | yes      | –       | Destination address that all fetched emails are forwarded to.             |
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
| `receivers[].delete_after_forward`              | receiver  | no       | `false` | Delete emails from the source server after a successful forward.          |
| `receivers[].imap_folder`                       | receiver  | no       | `INBOX` | IMAP mailbox to monitor (ignored for POP3).                               |

> **Note**: When `delete_after_forward` is `false`, forwarded message IDs are tracked in memory only. For POP3, restarting the program may re-forward existing messages; IMAP avoids this by fetching only `UNSEEN` messages.

## Usage

Run the binary:

```bash
./target/release/mail-forwarder

# Or with a specific config file
./target/release/mail-forwarder --config /path/to/config.toml

# Check configuration syntax plus SMTP and POP3 account connectivity
./target/release/mail-forwarder --config /path/to/config.toml --check
```

Account checks time out after 30 seconds per SMTP or POP3 account, so one slow server will not block the entire check indefinitely.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
