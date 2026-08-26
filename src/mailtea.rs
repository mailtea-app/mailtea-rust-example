//! A minimal Mailtea API client.
//!
//! There is no official Mailtea SDK for Rust, so this module is the example:
//! reqwest plus serde over the transactional email endpoints. It is meant to be
//! copied into your project and grown — the API is plain JSON over HTTPS, so
//! adding an endpoint is adding a struct and a method.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Production API. Override it for local dev or a self-hosted Mailtea.
pub const DEFAULT_BASE_URL: &str = "https://api.mailtea.app";

/// How long a single call may take before it is given up on.
///
/// `reqwest::Client::new()` has no timeout at all, and a send that hangs
/// forever is worse than one that fails: nothing retries it and nothing logs
/// it. Thirty seconds is generous for this API and still bounded.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

// Hand-written so a stray `dbg!(&client)` cannot print the API key.
impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl Client {
    /// `base_url` is only needed for local dev or a self-hosted Mailtea. Pass
    /// `None` — as an unset `MAILTEA_API_BASE_URL` does — in production.
    pub fn new(api_key: impl Into<String>, base_url: Option<String>) -> Self {
        let base_url = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                // Only fails if the TLS backend cannot start, which is what
                // `reqwest::Client::new()` panics on too.
                .build()
                .expect("could not build the HTTP client"),
            // A trailing slash in the env var would otherwise produce `//v1/emails`.
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    /// `POST /v1/emails` — send now, or schedule it with `scheduled_at`.
    pub async fn send(&self, email: &SendEmail) -> Result<SentEmail, MailteaError> {
        self.send_idempotent(email, None).await
    }

    /// The same send, tagged with a key of your choosing.
    ///
    /// This is what makes "back off and try again" safe. A timeout or a 5xx
    /// does not tell you whether the message went out — the API may have
    /// accepted it and the answer got lost on the way back — so a bare retry
    /// can deliver it twice. Replaying the SAME key with the SAME body returns
    /// the original result instead of sending again; the same key with a
    /// different body is refused with a 409.
    ///
    /// Use an id your own system already has and will reproduce on the retry:
    /// the order, the job, the row you are notifying about. A fresh random key
    /// per attempt protects nothing.
    pub async fn send_idempotent(
        &self,
        email: &SendEmail,
        idempotency_key: Option<&str>,
    ) -> Result<SentEmail, MailteaError> {
        let mut request = self
            .request(reqwest::Method::POST, "/v1/emails")
            .json(email);
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        self.execute(request).await
    }

    /// `GET /v1/emails/:id` — how a send is doing, without waiting for a webhook.
    pub async fn get(&self, id: &str) -> Result<Email, MailteaError> {
        self.execute(self.request(
            reqwest::Method::GET,
            &format!("/v1/emails/{}", path_segment(id)),
        ))
        .await
    }

    /// `POST /v1/emails/:id/cancel` — only works while the send is still scheduled.
    pub async fn cancel(&self, id: &str) -> Result<CanceledEmail, MailteaError> {
        self.execute(self.request(
            reqwest::Method::POST,
            &format!("/v1/emails/{}/cancel", path_segment(id)),
        ))
        .await
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base_url))
            .bearer_auth(&self.api_key)
    }

    async fn execute<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, MailteaError> {
        let response = request.send().await.map_err(MailteaError::Transport)?;
        let status = response.status();
        // Read the body as text first: a failed call carries a JSON error the
        // caller wants to see, and deserializing into `T` would throw it away.
        let body = response.text().await.map_err(MailteaError::Transport)?;

        if !status.is_success() {
            return Err(MailteaError::Api {
                status: status.as_u16(),
                message: error_message(&body, status),
            });
        }

        serde_json::from_str(&body).map_err(MailteaError::Decode)
    }
}

/// Escape one path segment.
///
/// A real `txemail_…` id passes through untouched. An id that came from
/// somewhere less trustworthy — a URL, a form, a row someone else writes — must
/// not be able to walk out of the segment it belongs in: interpolated raw, an
/// id of `../domains` calls a different endpoint than the method that took it.
/// `Url` does the escaping, because the set of characters that need it is not
/// obvious enough to write out by hand.
fn path_segment(value: &str) -> String {
    let mut url = reqwest::Url::parse("http://mailtea.invalid").expect("a literal URL parses");
    url.path_segments_mut()
        .expect("an http URL takes path segments")
        .clear()
        .push(value);
    url.path().trim_start_matches('/').to_string()
}

/// A message to send. `Default` covers the optional half, so the usual call is
/// a struct literal with `..Default::default()`.
#[derive(Debug, Default, Serialize)]
pub struct SendEmail {
    /// `Name <you@your-verified-domain.com>`, or a bare address.
    pub from: String,
    /// The API takes a string or an array, so a `Vec` covers both. SES caps a
    /// message at 50 recipients combined across `to`, `cc`, and `bcc`.
    pub to: Vec<String>,
    pub subject: String,
    /// Send `html`, `text`, or both. Both is what inboxes prefer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cc: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bcc: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reply_to: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// RFC 3339 in UTC, e.g. `2026-09-01T09:00:00Z`. Omit to send immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<String>,
}

/// Arbitrary label carried with the send and echoed back on its events.
#[derive(Debug, Serialize)]
pub struct Tag {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct SentEmail {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct CanceledEmail {
    pub id: String,
}

/// A stored email. Unknown fields are ignored, so adding one server-side will
/// not break this — widen the struct when you want to read it.
#[derive(Debug, Deserialize)]
pub struct Email {
    pub id: String,
    #[serde(default)]
    pub subject: Option<String>,
    /// Recipients come back as the single string the message was addressed to.
    #[serde(default)]
    pub to: Option<String>,
    /// The latest thing that happened: `queued`, `scheduled`, `sent`,
    /// `delivered`, `delivery_delayed`, `bounced`, `complained`, `suppressed`,
    /// `canceled`, `failed`. A `String` rather than an enum on purpose — a
    /// status added server-side should not stop this from deserializing.
    #[serde(default)]
    pub last_event: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub scheduled_at: Option<String>,
    /// Why the send failed, when it did. The API redacts this before it leaves
    /// the building, so it is a reason to show an operator, not a provider
    /// diagnostic to parse.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum MailteaError {
    /// The API answered, and said no. `message` is its own `error` string —
    /// the thing worth logging or showing an operator.
    Api { status: u16, message: String },
    /// The request never got an answer: DNS, TLS, connection, timeout.
    Transport(reqwest::Error),
    /// A 2xx body that did not match the shape expected here.
    Decode(serde_json::Error),
}

impl MailteaError {
    /// The HTTP status, when the API is the one that refused. Retry logic reads
    /// this: 429 and 5xx are worth retrying, other 4xx mean fix the request.
    pub fn status(&self) -> Option<u16> {
        match self {
            MailteaError::Api { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl fmt::Display for MailteaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailteaError::Api { status, message } => {
                write!(f, "Mailtea API error (HTTP {status}): {message}")
            }
            MailteaError::Transport(err) => write!(f, "could not reach the Mailtea API: {err}"),
            MailteaError::Decode(err) => write!(f, "unexpected response from Mailtea: {err}"),
        }
    }
}

impl std::error::Error for MailteaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MailteaError::Api { .. } => None,
            MailteaError::Transport(err) => Some(err),
            MailteaError::Decode(err) => Some(err),
        }
    }
}

/// Errors come back as `{"error": "..."}`, and a 400 adds `details`: one entry
/// per field the API refused. Both go into the message — on its own, "Validation
/// failed" does not say which field failed, which is the only thing you need.
/// Fall back to the raw body so a proxy's HTML 502 is still legible in the log.
fn error_message(body: &str, status: reqwest::StatusCode) -> String {
    #[derive(Deserialize)]
    struct ApiError {
        error: Option<String>,
        details: Option<Value>,
    }

    if let Ok(ApiError {
        error: Some(message),
        details,
    }) = serde_json::from_str::<ApiError>(body)
    {
        let detail = details.as_ref().map(detail_summary).unwrap_or_default();
        return if detail.is_empty() {
            message
        } else {
            format!("{message}: {detail}")
        };
    }

    let body = body.trim();
    if body.is_empty() {
        status
            .canonical_reason()
            .unwrap_or("request failed")
            .to_string()
    } else {
        body.chars().take(500).collect()
    }
}

/// `details` flattened to one line. Each entry names a field in `path` and the
/// problem in `message`; anything shaped differently is kept as raw JSON rather
/// than dropped, because a detail you cannot read still beats one you never see.
fn detail_summary(details: &Value) -> String {
    let Some(entries) = details.as_array() else {
        return details.to_string();
    };

    entries
        .iter()
        .map(|entry| {
            let Some(message) = entry.get("message").and_then(Value::as_str) else {
                return entry.to_string();
            };
            let field = entry
                .get("path")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .map(|part| {
                            part.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| part.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(".")
                })
                .unwrap_or_default();

            if field.is_empty() {
                message.to_string()
            } else {
                format!("{field}: {message}")
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}
