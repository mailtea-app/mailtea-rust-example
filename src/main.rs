//! Sends an email through Mailtea, reads its status back, then schedules a
//! second one and cancels it.

use std::env;
use std::process::ExitCode;

use chrono::{SecondsFormat, TimeDelta, Utc};
use mailtea::Mailtea;
use mailtea_rust_example::{hello_email, scheduled_email};

#[tokio::main]
async fn main() -> ExitCode {
    // Development convenience. In production the environment already has these.
    let _ = dotenvy::dotenv();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let from = required("MAILTEA_FROM")?;
    let to = required("MAILTEA_TO")?;
    let subject = env::var("MAILTEA_SUBJECT").unwrap_or_else(|_| "Hello from Rust".to_string());

    // Reads MAILTEA_API_KEY, and MAILTEA_API_BASE_URL when it is set —
    // that one is only for local dev or a self-hosted Mailtea; unset, the
    // client talks to https://api.mailtea.app.
    let mailtea = Mailtea::from_env()?;

    // One key per logical send, so a retry replays the first answer instead of
    // mailing the same person twice. A real app reuses an id it already has —
    // the order, the job, the row — because it has to survive a restart.
    let idempotency_key = format!("rust-example-{}", Utc::now().timestamp_millis());

    let sent = match mailtea
        .emails
        .send_idempotent(&hello_email(&from, &to, &subject), Some(&idempotency_key))
        .await
    {
        Ok(sent) => sent,
        Err(error) => {
            // This is the branch worth copying. A client error means the
            // request never landed, so nothing was sent; anything else means
            // Mailtea answered and refused, and says why.
            if error.is_client_error() {
                eprintln!("Send failed before it left: {error}");
            } else {
                eprintln!(
                    "Mailtea refused the send (HTTP {}): {}",
                    error.status(),
                    error.message()
                );
                // "Validation failed" alone does not say what to change.
                if let Some(details) = error.details() {
                    eprintln!("  {details}");
                }
                if error.is_retryable() {
                    // Safe to repeat only because the send carried a key:
                    // without one, a retry after a lost answer sends twice.
                    eprintln!("Retryable — back off and send it again with the same key.");
                }
            }
            return Err(error.into());
        }
    };
    println!("Sent {}", sent.id);

    // Status without waiting for a webhook. A send that just left is usually
    // still `queued` — the delivery events land seconds later.
    let email = mailtea.emails.get(&sent.id).await?;
    println!(
        "  subject: {}\n  status:  {}",
        email.subject.as_deref().unwrap_or("(none)"),
        email.status.as_deref().unwrap_or("(unknown)")
    );

    let scheduled_at = (Utc::now() + TimeDelta::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let scheduled = mailtea
        .emails
        .send(&scheduled_email(&from, &to, &subject, &scheduled_at))
        .await?;
    println!("Scheduled {} for {scheduled_at}", scheduled.id);

    // Cancelling works while the send is still scheduled; after that the API
    // answers 422 and the message is already on its way.
    let canceled = mailtea.emails.cancel(&scheduled.id).await?;
    println!("Canceled {}", canceled.id);

    Ok(())
}

fn required(name: &str) -> Result<String, String> {
    env::var(name)
        .map_err(|_| format!("{name} is not set. Copy .env.example to .env and fill it in."))
}
