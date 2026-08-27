//! The example is a binary, but the messages it builds live behind a library
//! target so the integration tests in `tests/` exercise exactly the payloads
//! `main.rs` sends.

use mailtea::SendEmail;

/// The message this example sends.
///
/// `SendEmail::new` covers the required half — From, recipients, subject — and
/// the chained setters the rest. Anything left unset is left off the wire
/// rather than sent as `null`: an empty `cc` or a null `scheduled_at` would
/// turn an immediate send into a rejected one.
///
/// SES caps a single message at 50 recipients combined across `to`, `cc` and
/// `bcc`.
pub fn hello_email(from: &str, to: &str, subject: &str) -> SendEmail {
    SendEmail::new(from, [to], subject)
        // Send `html`, `text`, or both. Both is what inboxes prefer.
        .html("<p>Sent with <strong>Rust</strong> and the Mailtea SDK.</p>")
        .text("Sent with Rust and the Mailtea SDK.")
        // An arbitrary label, echoed back on the send's delivery events.
        .tag("example", "rust")
}

/// The same message, queued for later.
///
/// Scheduling is the same call with `scheduled_at` set (RFC 3339, UTC).
pub fn scheduled_email(from: &str, to: &str, subject: &str, scheduled_at: &str) -> SendEmail {
    SendEmail::new(from, [to], format!("{subject} (scheduled)"))
        .html("<p>This one was queued an hour ahead.</p>")
        .scheduled_at(scheduled_at)
}
