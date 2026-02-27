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

Create a `config.toml` file in the working directory:

```toml
# Destination email address
forward_to = "target@example.com"

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

## Usage

Run the binary:

```bash
./target/release/mail-forwarder

# Or with a specific config file
./target/release/mail-forwarder --config /path/to/config.toml
```

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
