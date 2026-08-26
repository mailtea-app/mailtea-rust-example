//! Sends an email through Mailtea, reads its status back, then schedules a
//! second one and cancels it.

use std::env;
use std::process::ExitCode;

use chrono::{SecondsFormat, TimeDelta, Utc};
use mailtea_rust_example::mailtea::{Client, MailteaError, SendEmail, Tag};

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
    let api_key = required("MAILTEA_API_KEY")?;
    let from = required("MAILTEA_FROM")?;
    let to = required("MAILTEA_TO")?;
    let subject = env::var("MAILTEA_SUBJECT").unwrap_or_else(|_| "Hello from Rust".to_string());

    // `MAILTEA_API_BASE_URL` is only set for local dev or a self-hosted
    // Mailtea; unset, the client talks to https://api.mailtea.app.
    let mailtea = Client::new(api_key, env::var("MAILTEA_API_BASE_URL").ok());

    // One key per logical send, so a retry replays the first answer instead of
    // mailing the same person twice. A real app reuses an id it already has —
    // the order, the job, the row — because it has to survive a restart.
    let idempotency_key = format!("rust-example-{}", Utc::now().timestamp_millis());

    let sent = match mailtea
        .send_idempotent(
            &SendEmail {
                from: from.clone(),
                to: vec![to.clone()],
                subject: subject.clone(),
                html: Some(
                    "<p>Sent with <strong>Rust</strong> and the Mailtea API.</p>".to_string(),
                ),
                text: Some("Sent with Rust and the Mailtea API.".to_string()),
                tags: vec![Tag {
                    name: "example".to_string(),
                    value: "rust".to_string(),
                }],
                ..Default::default()
            },
            Some(&idempotency_key),
        )
        .await
    {
        Ok(sent) => sent,
        Err(error) => {
            // This is the branch worth copying. An API error means Mailtea
            // answered and refused, and says why; a transport error means the
            // request never landed, so nothing was sent.
            match &error {
                MailteaError::Api { status, message } => {
                    eprintln!("Mailtea refused the send (HTTP {status}): {message}");
                    if *status == 429 || *status >= 500 {
                        // Safe to repeat only because the send carried a key:
                        // without one, a retry after a lost answer sends twice.
                        eprintln!("Retryable — back off and send it again with the same key.");
                    }
                }
                other => eprintln!("Send failed: {other}"),
            }
            return Err(error.into());
        }
    };
    println!("Sent {}", sent.id);

    // Status without waiting for a webhook. A send that just left is usually
    // still `queued` — the delivery events land seconds later.
    let email = mailtea.get(&sent.id).await?;
    println!(
        "  subject: {}\n  status:  {}",
        email.subject.as_deref().unwrap_or("(none)"),
        email.last_event.as_deref().unwrap_or("(unknown)")
    );

    // Scheduling is the same call with `scheduled_at` set.
    let scheduled_at =
        (Utc::now() + TimeDelta::hours(1)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let scheduled = mailtea
        .send(&SendEmail {
            from,
            to: vec![to],
            subject: format!("{subject} (scheduled)"),
            html: Some("<p>This one was queued an hour ahead.</p>".to_string()),
            scheduled_at: Some(scheduled_at.clone()),
            ..Default::default()
        })
        .await?;
    println!("Scheduled {} for {scheduled_at}", scheduled.id);

    // Cancelling works while the send is still scheduled; after that the API
    // answers 422 and the message is already on its way.
    let canceled = mailtea.cancel(&scheduled.id).await?;
    println!("Canceled {}", canceled.id);

    Ok(())
}

fn required(name: &str) -> Result<String, String> {
    env::var(name)
        .map_err(|_| format!("{name} is not set. Copy .env.example to .env and fill it in."))
}
