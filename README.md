# Mailtea + Rust Example

This example shows how to use [Mailtea](https://mailtea.app) with Rust to send a
transactional email, read its status back, then schedule one and cancel it.

It uses the official Rust SDK,
[`mailtea`](https://github.com/mailtea-app/mailtea-rust) — a thin, typed, async
wrapper over the REST API on `tokio` and `reqwest`.

## Prerequisites

To get the most out of this guide, you'll need to:

- [Create an API key](https://studio.mailtea.app/api-keys)
- [Verify your domain](https://docs.mailtea.app/docs/documentation/domains)

You also need Rust 1.75 or newer (`rustup update stable`). On exactly 1.75,
resolve the lockfile with `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback
cargo generate-lockfile` first — the newest releases of some transitive
dependencies require a newer toolchain. Current stable needs none of that.

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

`Mailtea::from_env()` reads `MAILTEA_API_KEY`. `MAILTEA_API_BASE_URL` is only
needed for local dev or a self-hosted Mailtea — unset, the client talks to
`https://api.mailtea.app`.

```rust
use mailtea::{Mailtea, SendEmail};

let mailtea = Mailtea::from_env()?;

let sent = mailtea
    .emails
    .send(
        &SendEmail::new("Acme <hello@acme.com>", ["reader@yourdomain.com"], "Hello from Rust")
            .html("<p>Sent with Rust and the Mailtea SDK.</p>"),
    )
    .await?;

println!("{}", sent.id); // txemail_...
```

`SendEmail::new` covers the required half — From, recipients, subject — and the
chained setters the rest: `text`, `cc`, `bcc`, `reply_to`, `tag`, `header`,
`attachment`, `scheduled_at`. It also derives `Default`, so a struct literal
with `..Default::default()` works. Unset fields are left off the wire rather
than sent as `null`.

Add `scheduled_at` (RFC 3339, UTC) to schedule instead of sending now, and cancel
with `mailtea.emails.cancel(&id)` while it is still `scheduled`. Only a scheduled
send can be cancelled — an ordinary immediate one is `queued` and already on its
way, so it answers 422. SES caps a single message at 50 recipients combined
across `to`, `cc`, and `bcc`.

### Retrying safely

A timeout or a 5xx does not tell you whether the message went out, so a bare
retry can deliver it twice. `send_idempotent` attaches an `Idempotency-Key`:
replaying the same key with the same body returns the original result instead of
sending again, and the same key with a different body is refused with a 409.

```rust
// An id your system already has and will reproduce on the retry — not a fresh
// random one per attempt, which protects nothing.
let sent = mailtea
    .emails
    .send_idempotent(&email, Some(&format!("order-{order_id}")))
    .await?;
```

## Errors

Every call returns `Result<_, mailtea::Error>`. One accessor separates the two
failures that need different handling:

```rust
match mailtea.emails.send(&email).await {
    Ok(sent) => println!("Sent {}", sent.id),
    Err(error) if error.is_client_error() => {
        // The request never landed — DNS, TLS, connection, timeout, or a
        // missing key. Nothing was sent, so a retry is safe.
        eprintln!("{error}");
    }
    Err(error) => {
        // Mailtea answered and refused. `message()` is the API's own error
        // string and `details()` names the fields a 400 objected to —
        // "Validation failed" on its own does not tell you what to change.
        // `is_retryable()` covers 429 and 5xx; other 4xx mean fix the request.
        eprintln!("HTTP {}: {}", error.status(), error.message());
        if let Some(details) = error.details() {
            eprintln!("{details}");
        }
    }
}
```

`mailtea::Error` implements `std::error::Error`, so it works with `?`,
`Box<dyn Error>`, `anyhow`, and `thiserror`'s `#[from]`.

The SDK caps every call at 30 seconds. `reqwest`'s own client has no timeout, and
a send that hangs forever is worse than one that fails — nothing retries it and
nothing logs it.

## What this example covers

- Sending an email with `html`, `text`, and `tags` through the official SDK
- Reading a send's status back with `emails.get`
- Scheduling with `scheduled_at`, then cancelling before it goes out
- A typed error carrying the API's status, message, and the fields a 400 named,
  so a failed send is loud instead of silent
- An `Idempotency-Key` so a retried send does not arrive twice
- Resolving the key and base URL from the environment, so the same binary runs
  against production, a self-hosted instance, or a local dev API
- Keeping the API key in the environment — the client's `Debug` prints it
  redacted

## Tests

```bash
cargo test
```

The tests run against a bundled mock Mailtea server, so they need no API key and
make no network calls. The mock is
[`tests/mock_mailtea/mod.rs`](tests/mock_mailtea/mod.rs) — a tokio `TcpListener`
that records every request — and the assertions check the method, path,
`Authorization` header, and JSON body of each call the SDK makes.

## Learn more

- [Documentation](https://docs.mailtea.app)
- [API reference](https://docs.mailtea.app/docs/api-reference)
- [Rust SDK](https://github.com/mailtea-app/mailtea-rust) ·
  [Node.js SDK](https://github.com/mailtea-app/mailtea-node) ·
  [Python SDK](https://github.com/mailtea-app/mailtea-python) ·
  [MCP server](https://github.com/mailtea-app/mailtea-mcp)
