//! Every assertion here runs against the bundled mock in `mock_mailtea`, so the
//! suite needs no API key and touches no network.

mod mock_mailtea;

use mailtea::{Mailtea, SendEmail};
use mailtea_rust_example::{hello_email, scheduled_email};

const API_KEY: &str = "mt_pat_testkeytestkeytestkeytestkey00";
const FROM: &str = "Acme <hello@acme.com>";
const TO: &str = "reader@yourdomain.com";

async fn client() -> (mock_mailtea::MockMailtea, Mailtea) {
    let mock = mock_mailtea::start().await;
    let mailtea = Mailtea::builder()
        .api_key(API_KEY)
        .base_url(mock.url.clone())
        .build()
        .expect("an explicit key and base URL build");
    (mock, mailtea)
}

#[tokio::test]
async fn send_posts_the_email_and_returns_its_id() {
    let (mock, mailtea) = client().await;

    let sent = mailtea
        .emails
        .send(&hello_email(FROM, TO, "Hello from Rust"))
        .await
        .expect("send failed");

    let request = mock.last();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/emails");
    assert_eq!(
        request.authorization.as_deref(),
        Some(format!("Bearer {API_KEY}").as_str())
    );
    assert_eq!(request.body["from"], FROM);
    assert_eq!(request.body["to"], serde_json::json!([TO]));
    assert_eq!(request.body["subject"], "Hello from Rust");
    assert_eq!(
        request.body["html"],
        "<p>Sent with <strong>Rust</strong> and the Mailtea SDK.</p>"
    );
    assert_eq!(request.body["text"], "Sent with Rust and the Mailtea SDK.");
    assert_eq!(request.body["tags"][0]["name"], "example");
    assert_eq!(request.body["tags"][0]["value"], "rust");
    assert_eq!(sent.id, "txemail_00000000000000000000000000000000");
}

#[tokio::test]
async fn unset_fields_are_left_out_of_the_body() {
    let (mock, mailtea) = client().await;

    mailtea
        .emails
        .send(&hello_email(FROM, TO, "Hello from Rust"))
        .await
        .expect("send failed");

    // An empty `cc` or a null `scheduled_at` on the wire would turn an immediate
    // send into a rejected one, so the optional fields have to disappear.
    let body = mock.last().body;
    for field in ["cc", "bcc", "reply_to", "scheduled_at"] {
        assert_eq!(body.get(field), None, "{field} should not be serialized");
    }
}

#[tokio::test]
async fn a_scheduled_send_carries_scheduled_at() {
    let (mock, mailtea) = client().await;

    mailtea
        .emails
        .send(&scheduled_email(
            FROM,
            TO,
            "Hello from Rust",
            "2026-09-01T09:00:00Z",
        ))
        .await
        .expect("send failed");

    let body = mock.last().body;
    assert_eq!(body["scheduled_at"], "2026-09-01T09:00:00Z");
    assert_eq!(body["subject"], "Hello from Rust (scheduled)");
}

#[tokio::test]
async fn get_reads_the_status_back() {
    let (mock, mailtea) = client().await;

    let email = mailtea
        .emails
        .get("txemail_00000000000000000000000000000000")
        .await
        .expect("get failed");

    let request = mock.last();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path,
        "/v1/emails/txemail_00000000000000000000000000000000"
    );
    assert!(request
        .authorization
        .as_deref()
        .is_some_and(|value| value.starts_with("Bearer ")));
    assert_eq!(email.id, "txemail_00000000000000000000000000000000");
    // The SDK fills `status` in from the wire's `last_event`.
    assert_eq!(email.status.as_deref(), Some("delivered"));
    assert_eq!(email.subject.as_deref(), Some("Mock email"));
}

#[tokio::test]
async fn cancel_hits_the_cancel_route() {
    let (mock, mailtea) = client().await;

    let canceled = mailtea
        .emails
        .cancel("txemail_00000000000000000000000000000000")
        .await
        .expect("cancel failed");

    let request = mock.last();
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        "/v1/emails/txemail_00000000000000000000000000000000/cancel"
    );
    assert_eq!(canceled.id, "txemail_00000000000000000000000000000000");
}

#[tokio::test]
async fn a_rejected_send_surfaces_the_status_message_and_details() {
    let (mock, mailtea) = client().await;

    let error = mailtea
        .emails
        .send(&SendEmail::new("", [TO], "Hello from Rust"))
        .await
        .expect_err("a send with no from address should fail");

    // The status and the API's own words, plus the `details` entry naming the
    // field. "Validation failed" alone does not say what to change.
    assert_eq!(error.status(), 400);
    assert_eq!(error.message(), "Validation failed");
    assert_eq!(error.details().unwrap()[0]["path"], serde_json::json!(["from"]));
    assert!(!error.is_client_error());
    assert!(!error.is_retryable());
    assert_eq!(mock.last().path, "/v1/emails");
}

#[tokio::test]
async fn an_idempotency_key_rides_along_as_a_header() {
    let (mock, mailtea) = client().await;

    mailtea
        .emails
        .send_idempotent(&hello_email(FROM, TO, "Hello from Rust"), Some("order-1138"))
        .await
        .expect("send failed");

    let request = mock.last();
    assert_eq!(request.idempotency_key.as_deref(), Some("order-1138"));
    // A header, not a body field — sending it as one would be a rejected send.
    assert_eq!(request.body.get("idempotency_key"), None);
}

#[tokio::test]
async fn a_plain_send_sets_no_idempotency_key() {
    let (mock, mailtea) = client().await;

    mailtea
        .emails
        .send(&hello_email(FROM, TO, "Hello from Rust"))
        .await
        .expect("send failed");

    assert_eq!(mock.last().idempotency_key, None);
}

#[tokio::test]
async fn an_id_cannot_walk_out_of_its_path_segment() {
    let (mock, mailtea) = client().await;

    // Interpolated raw, this would call `GET /v1/domains` instead. The SDK
    // escapes every id into its own path segment.
    let _ = mailtea.emails.get("../domains").await;

    assert_eq!(mock.last().path, "/v1/emails/..%2Fdomains");
}

#[tokio::test]
async fn a_base_url_with_a_trailing_slash_still_resolves() {
    let mock = mock_mailtea::start().await;
    let mailtea = Mailtea::builder()
        .api_key(API_KEY)
        .base_url(format!("{}/", mock.url))
        .build()
        .expect("build failed");

    mailtea
        .emails
        .send(&hello_email(FROM, TO, "Hello from Rust"))
        .await
        .expect("send failed");

    assert_eq!(mock.last().path, "/v1/emails");
}

#[tokio::test]
async fn the_whole_run_hits_the_routes_the_readme_describes() {
    let (mock, mailtea) = client().await;

    let sent = mailtea
        .emails
        .send(&hello_email(FROM, TO, "Hello from Rust"))
        .await
        .expect("send failed");
    mailtea.emails.get(&sent.id).await.expect("get failed");
    mailtea.emails.cancel(&sent.id).await.expect("cancel failed");

    let routes: Vec<(String, String)> = mock
        .requests()
        .into_iter()
        .map(|request| (request.method, request.path))
        .collect();
    assert_eq!(
        routes,
        vec![
            ("POST".to_string(), "/v1/emails".to_string()),
            ("GET".to_string(), format!("/v1/emails/{}", sent.id)),
            ("POST".to_string(), format!("/v1/emails/{}/cancel", sent.id)),
        ]
    );
}

#[tokio::test]
async fn a_send_that_never_lands_is_a_client_error() {
    // Port 1 is not listening, which is the closest thing to "the network is
    // down" a test can arrange without a network.
    let mailtea = Mailtea::builder()
        .api_key(API_KEY)
        .base_url("http://127.0.0.1:1")
        .build()
        .expect("build failed");

    let error = mailtea
        .emails
        .send(&hello_email(FROM, TO, "Hello from Rust"))
        .await
        .expect_err("a send to a closed port should fail");

    // Status 0 means the request never reached the API, so nothing was sent.
    assert!(error.is_client_error());
    assert_eq!(error.status(), 0);
}

#[tokio::test]
async fn a_missing_api_key_fails_loudly_rather_than_silently() {
    let error = Mailtea::builder()
        .api_key("")
        .build()
        .expect_err("an empty key should not build");

    assert_eq!(error.code(), Some("missing_api_key"));
    assert!(error.message().contains("MAILTEA_API_KEY"));
}
