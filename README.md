# Mailtea + Rust Example

This example shows how to use [Mailtea](https://mailtea.app) with Rust to send a
transactional email, read its status back, then schedule one and cancel it.

There is no official Mailtea SDK for Rust. The API is plain JSON over HTTPS, so
this example ships its own client instead: [`src/mailtea.rs`](src/mailtea.rs) is
about 300 lines of `reqwest` and `serde` that you can copy into your project and
extend one struct at a time.

## Prerequisites

To get the most out of this guide, you'll need to:

- [Create an API key](https://studio.mailtea.app/api-keys)
- [Verify your domain](https://docs.mailtea.app/docs/documentation/domains)

You also need Rust 1.85 or newer (`rustup update stable`).

## Instructions

1. Install dependencies:
   ```bash
   cargo build
   ```
2. Copy `.env.example` to `.env` and add your API key:
   ```bash
   cp .env.example .env
   ```
3. Run it:
   ```bash
   cargo run
   ```

Output:

```
Sent txemail_d319be210e4b4b2ab9873cc24a7bcbff
  subject: Hello from Rust
  status:  queued
Scheduled txemail_d17754e1a0df4274bacc168424d8ff60 for 2026-09-01T09:00:00Z
Canceled txemail_d17754e1a0df4274bacc168424d8ff60
```

## Sending

The client takes the API key and an optional base URL. `MAILTEA_API_BASE_URL` is
only needed for local dev or a self-hosted Mailtea — unset, it falls back to
`https://api.mailtea.app`.

```rust
use mailtea_rust_example::mailtea::{Client, SendEmail};

let mailtea = Client::new(
    std::env::var("MAILTEA_API_KEY")?,
    std::env::var("MAILTEA_API_BASE_URL").ok(),
);

let sent = mailtea
    .send(&SendEmail {
        from: "Acme <hello@acme.com>".to_string(),
        to: vec!["reader@yourdomain.com".to_string()],
        subject: "Hello from Rust".to_string(),
        html: Some("<p>Sent with Rust and the Mailtea API.</p>".to_string()),
        ..Default::default()
    })
    .await?;

println!("{}", sent.id); // txemail_...
```

`SendEmail` derives `Default`, so `..Default::default()` covers everything you
are not setting: `text`, `cc`, `bcc`, `reply_to`, `tags`, `scheduled_at`. Unset
fields are left off the wire rather than sent as `null`.

Add `scheduled_at` (RFC 3339, UTC) to schedule instead of sending now, and cancel
with `mailtea.cancel(&id)` while it is still `scheduled`. Only a scheduled send
can be cancelled — an ordinary immediate one is `queued` and already on its way,
so it answers 422. SES caps a single message at 50 recipients combined across
`to`, `cc`, and `bcc`.

### Retrying safely

A timeout or a 5xx does not tell you whether the message went out, so a bare
retry can deliver it twice. `send_idempotent` attaches an `Idempotency-Key`:
replaying the same key with the same body returns the original result instead of
sending again, and the same key with a different body is refused with a 409.

```rust
// An id your system already has and will reproduce on the retry — not a fresh
// random one per attempt, which protects nothing.
let sent = mailtea
    .send_idempotent(&email, Some(&format!("order-{order_id}")))
    .await?;
```

## Errors

Every call returns `Result<_, MailteaError>`. The enum separates the two failures
that need different handling:

```rust
match mailtea.send(&email).await {
    Ok(sent) => println!("Sent {}", sent.id),
    Err(MailteaError::Api { status, message }) => {
        // Mailtea answered and refused. `message` is the API's own error string,
        // plus the fields named in a 400's `details` — "Validation failed" on
        // its own does not tell you what to change.
        // 429 and 5xx are worth retrying; other 4xx mean fix the request.
        eprintln!("HTTP {status}: {message}");
    }
    Err(other) => {
        // The request never landed — DNS, TLS, connection, timeout.
        eprintln!("{other}");
    }
}
```

`MailteaError` implements `std::error::Error`, so it works with `?`,
`Box<dyn Error>`, `anyhow`, and `thiserror`'s `#[from]`.

Every call is capped at 30 seconds. `reqwest`'s client has no timeout of its
own, and a send that hangs forever is worse than one that fails — nothing
retries it and nothing logs it.

## What this example covers

- Sending an email with `html`, `text`, and `tags`
- Reading a send's `last_event` back with `GET /v1/emails/:id`
- Scheduling with `scheduled_at`, then cancelling before it goes out
- A typed error carrying the API's status, message, and the fields a 400 named,
  so a failed send is loud instead of silent
- A bounded request timeout, an `Idempotency-Key` so a retried send does not
  arrive twice, and ids escaped into their path segment rather than
  interpolated raw
- Resolving the base URL from `MAILTEA_API_BASE_URL`, so the same binary runs
  against production, a self-hosted instance, or a local dev API
- Keeping the API key in the environment — `Client`'s `Debug` prints it redacted

## Tests

```bash
cargo test
```

The tests run against a bundled mock Mailtea server, so they need no API key and
make no network calls. The mock is
[`tests/mock_mailtea/mod.rs`](tests/mock_mailtea/mod.rs) — a tokio `TcpListener`
that records every request — and the assertions check the method, path,
`Authorization` header, and JSON body of each call.

## Learn more

- [Documentation](https://docs.mailtea.app)
- [API reference](https://docs.mailtea.app/docs/api-reference)
- [Node.js SDK](https://github.com/mailtea-app/mailtea-node) ·
  [Python SDK](https://github.com/mailtea-app/mailtea-python) ·
  [MCP server](https://github.com/mailtea-app/mailtea-mcp)
