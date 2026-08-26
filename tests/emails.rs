//! Every assertion here runs against the bundled mock in `mock_mailtea`, so the
//! suite needs no API key and touches no network.

mod mock_mailtea;

use mailtea_rust_example::mailtea::{Client, MailteaError, SendEmail, Tag};

const API_KEY: &str = "mt_pat_testkeytestkeytestkeytestkey00";

fn hello() -> SendEmail {
    SendEmail {
        from: "Acme <hello@acme.com>".to_string(),
        to: vec!["reader@yourdomain.com".to_string()],
        subject: "Hello from Rust".to_string(),
        html: Some("<p>Sent with Rust.</p>".to_string()),
        text: Some("Sent with Rust.".to_string()),
        tags: vec![Tag {
            name: "example".to_string(),
            value: "rust".to_string(),
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn send_posts_the_email_and_returns_its_id() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    let sent = mailtea.send(&hello()).await.expect("send failed");

    let request = mock.last();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/emails");
    assert_eq!(
        request.authorization.as_deref(),
        Some(format!("Bearer {API_KEY}").as_str())
    );
    assert_eq!(request.body["from"], "Acme <hello@acme.com>");
    assert_eq!(
        request.body["to"],
        serde_json::json!(["reader@yourdomain.com"])
    );
    assert_eq!(request.body["subject"], "Hello from Rust");
    assert_eq!(request.body["html"], "<p>Sent with Rust.</p>");
    assert_eq!(request.body["text"], "Sent with Rust.");
    assert_eq!(request.body["tags"][0]["name"], "example");
    assert_eq!(request.body["tags"][0]["value"], "rust");
    assert_eq!(sent.id, "txemail_00000000000000000000000000000000");
}

#[tokio::test]
async fn unset_fields_are_left_out_of_the_body() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    mailtea.send(&hello()).await.expect("send failed");

    // An empty `cc` or a null `scheduled_at` on the wire would turn an immediate
    // send into a rejected one, so the optional fields have to disappear.
    let body = mock.last().body;
    for field in ["cc", "bcc", "reply_to", "scheduled_at"] {
        assert_eq!(body.get(field), None, "{field} should not be serialized");
    }
}

#[tokio::test]
async fn a_scheduled_send_carries_scheduled_at() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    mailtea
        .send(&SendEmail {
            scheduled_at: Some("2026-09-01T09:00:00Z".to_string()),
            ..hello()
        })
        .await
        .expect("send failed");

    assert_eq!(mock.last().body["scheduled_at"], "2026-09-01T09:00:00Z");
}

#[tokio::test]
async fn get_reads_the_status_back() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    let email = mailtea
        .get("txemail_00000000000000000000000000000000")
        .await
        .expect("get failed");

    let request = mock.last();
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path,
        "/v1/emails/txemail_00000000000000000000000000000000"
    );
    assert!(
        request
            .authorization
            .as_deref()
            .is_some_and(|value| value.starts_with("Bearer "))
    );
    assert_eq!(email.id, "txemail_00000000000000000000000000000000");
    assert_eq!(email.last_event.as_deref(), Some("delivered"));
    assert_eq!(email.subject.as_deref(), Some("Mock email"));
}

#[tokio::test]
async fn cancel_hits_the_cancel_route() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    let canceled = mailtea
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
async fn a_rejected_send_surfaces_the_status_and_message() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    let error = mailtea
        .send(&SendEmail {
            from: String::new(),
            ..hello()
        })
        .await
        .expect_err("a send with no from address should fail");

    // The status and the API's own words, plus the `details` entry naming the
    // field. "Validation failed" alone does not say what to change.
    match &error {
        MailteaError::Api { status, message } => {
            assert_eq!(*status, 400);
            assert_eq!(
                message,
                "Validation failed: from: String must contain at least 1 character(s)"
            );
        }
        other => panic!("expected an API error, got {other:?}"),
    }
    assert_eq!(error.status(), Some(400));
    assert_eq!(
        error.to_string(),
        "Mailtea API error (HTTP 400): Validation failed: from: String must contain at least 1 character(s)"
    );
}

#[tokio::test]
async fn an_idempotency_key_rides_along_as_a_header() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    mailtea
        .send_idempotent(&hello(), Some("order-1138"))
        .await
        .expect("send failed");

    let request = mock.last();
    assert_eq!(request.idempotency_key.as_deref(), Some("order-1138"));
    // A header, not a body field — sending it as one would be a rejected send.
    assert_eq!(request.body.get("idempotency_key"), None);
}

#[tokio::test]
async fn a_plain_send_sets_no_idempotency_key() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    mailtea.send(&hello()).await.expect("send failed");

    assert_eq!(mock.last().idempotency_key, None);
}

#[tokio::test]
async fn an_id_cannot_walk_out_of_its_path_segment() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    // Interpolated raw, this would call `GET /v1/domains` instead.
    let _ = mailtea.get("../domains").await;

    assert_eq!(mock.last().path, "/v1/emails/..%2Fdomains");
}

#[tokio::test]
async fn a_base_url_with_a_trailing_slash_still_resolves() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(format!("{}/", mock.url)));

    mailtea.send(&hello()).await.expect("send failed");

    assert_eq!(mock.last().path, "/v1/emails");
}

#[tokio::test]
async fn every_request_is_recorded_in_order() {
    let mock = mock_mailtea::start().await;
    let mailtea = Client::new(API_KEY, Some(mock.url.clone()));

    let sent = mailtea.send(&hello()).await.expect("send failed");
    mailtea.get(&sent.id).await.expect("get failed");
    mailtea.cancel(&sent.id).await.expect("cancel failed");

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
async fn a_send_that_never_lands_is_a_transport_error() {
    // Port 1 is not listening, which is the closest thing to "the network is
    // down" a test can arrange without a network.
    let mailtea = Client::new(API_KEY, Some("http://127.0.0.1:1".to_string()));

    let error = mailtea
        .send(&hello())
        .await
        .expect_err("a send to a closed port should fail");

    assert!(matches!(error, MailteaError::Transport(_)));
    assert_eq!(error.status(), None);
}
