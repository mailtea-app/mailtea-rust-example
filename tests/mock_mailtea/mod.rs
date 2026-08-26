//! A tiny stand-in for the Mailtea API, so this example's tests run with no
//! credentials and no network. It records every request it receives, which is
//! what the assertions read.
//!
//! Point the client at `MockMailtea::url` to use it.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const EMAIL_ID: &str = "txemail_00000000000000000000000000000000";

#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub idempotency_key: Option<String>,
    /// `Value::Null` when the request had no JSON body.
    pub body: Value,
}

pub struct MockMailtea {
    pub url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl MockMailtea {
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }

    /// The most recent request, which is what most assertions want.
    pub fn last(&self) -> RecordedRequest {
        self.requests()
            .pop()
            .expect("the mock received no requests")
    }
}

/// Binds an ephemeral port and serves until the test process exits.
pub async fn start() -> MockMailtea {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("could not bind the mock server");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));

    let recorded = Arc::clone(&requests);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let recorded = Arc::clone(&recorded);
            tokio::spawn(async move { serve(socket, recorded).await });
        }
    });

    MockMailtea { url, requests }
}

async fn serve(mut socket: TcpStream, recorded: Arc<Mutex<Vec<RecordedRequest>>>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };

    let authorized = request
        .authorization
        .as_deref()
        .is_some_and(|value| value.starts_with("Bearer "));
    let route = (request.method.as_str(), request.path.as_str());
    recorded.lock().unwrap().push(request.clone());

    // Auth is checked first, the same way the real API does it — an example
    // that forgets the key should fail its test, not silently "send".
    let (status, payload) = if !authorized {
        (401, json!({ "error": "Unauthorized" }))
    } else {
        match route {
            ("POST", "/v1/emails") => {
                // The real API validates before it sends; so does this, because
                // the error path is half of what the tests are checking.
                let from = request.body.get("from").and_then(Value::as_str);
                if from.unwrap_or("").is_empty() {
                    // Shaped like the real 400: the API answers with `error`
                    // plus a `details` array naming each field it refused, and
                    // a client that shows only `error` throws that away.
                    (
                        400,
                        json!({
                            "error": "Validation failed",
                            "details": [{
                                "code": "too_small",
                                "path": ["from"],
                                "message": "String must contain at least 1 character(s)"
                            }]
                        }),
                    )
                } else {
                    (200, json!({ "id": EMAIL_ID }))
                }
            }
            ("GET", path) if email_id(path, "").is_some() => (
                200,
                json!({
                    "object": "email",
                    "id": email_id(path, "").unwrap(),
                    "to": "reader@yourdomain.com",
                    "subject": "Mock email",
                    "last_event": "delivered",
                    "created_at": "2026-01-01T00:00:00.000Z"
                }),
            ),
            ("POST", path) if email_id(path, "/cancel").is_some() => (
                200,
                json!({ "object": "email", "id": email_id(path, "/cancel").unwrap() }),
            ),
            _ => (404, json!({ "error": "Not Found", "path": request.path })),
        }
    };

    let body = payload.to_string();
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

/// `/v1/emails/:id{suffix}` → the id.
fn email_id(path: &str, suffix: &str) -> Option<String> {
    let id = path.strip_prefix("/v1/emails/")?.strip_suffix(suffix)?;
    (!id.is_empty() && !id.contains('/')).then(|| id.to_string())
}

async fn read_request(socket: &mut TcpStream) -> Option<RecordedRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];

    let head_end = loop {
        if let Some(index) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break index + 4;
        }
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut request_line = head.lines().next()?.split_whitespace();
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.split('?').next()?.to_string();
    let header = |name: &str| {
        head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };

    let length: usize = header("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }

    Some(RecordedRequest {
        method,
        path,
        authorization: header("authorization"),
        idempotency_key: header("idempotency-key"),
        body: serde_json::from_slice(&buffer[head_end..]).unwrap_or(Value::Null),
    })
}
