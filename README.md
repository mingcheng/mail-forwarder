# Mail Forwarder

A lightweight tool to forward emails from POP3/IMAP accounts to a specified SMTP destination written in Rust. 

The Gmail DO NOT support POP3 or Gmailify for the forwarding email address, so you need to use an App Password for authentication. For more details, please refer to the [official Gmail documentation](https://support.google.com/mail/answer/16604719). 

So, I wrote this tool to forward emails from POP3/IMAP accounts to a specified SMTP destination, which can be used with Gmail or any other email service that supports SMTP.

## Features

- Support for **POP3** and **IMAP** protocols.
- Monitor multiple email accounts simultaneously.
- TLS/SSL support.
- Configurable check intervals (in seconds).

## Installation

```bash
cargo build --release
```

## Configuration

Create a `config.toml` file in the working directory:

```toml
# Destination email address
forward_to = "target@example.com"

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

> **Note**: For services like Gmail or Outlook, please use an **App Password** instead of your login password.

## Usage

Run the binary:

```bash
./target/release/mail-forwarder

# Or with a specific config file
./target/release/mail-forwarder --config /path/to/config.toml
```

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
