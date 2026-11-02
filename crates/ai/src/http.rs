//! The one HTTPS client: errors are replies (their status and body are read), not failures.

use std::time::Duration;

use crate::AiError;

/// A finished reply.
pub(crate) struct Reply {
    pub status: u16,
    pub body: String,
}

/// An image comes back inside the JSON as base64: allow a large reply.
const MAX_REPLY: u64 = 128 * 1024 * 1024;

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(300))).build();
    ureq::Agent::new_with_config(config)
}

fn finish(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Reply, AiError> {
    let mut resp = result.map_err(|e| AiError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().with_config().limit(MAX_REPLY).read_to_string().map_err(|e| AiError::Network(e.to_string()))?;
    Ok(Reply { status, body })
}

/// POST a form-encoded body.
pub(crate) fn post_form(url: &str, body: &str) -> Result<Reply, AiError> {
    finish(agent().post(url).header("Content-Type", "application/x-www-form-urlencoded").header("Accept", "application/json").send(body))
}

/// POST JSON with a bearer token.
pub(crate) fn post_json(url: &str, bearer: &str, body: &serde_json::Value) -> Result<Reply, AiError> {
    let text = body.to_string();
    finish(
        agent()
            .post(url)
            .header("Authorization", &format!("Bearer {bearer}"))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream, application/json")
            .send(&text),
    )
}

/// GET with a bearer token.
pub(crate) fn get(url: &str, bearer: &str) -> Result<Reply, AiError> {
    finish(agent().get(url).header("Authorization", &format!("Bearer {bearer}")).header("Accept", "application/json").call())
}

/// `reply` when it is a success, else its status and (a short part of) its body as the error.
pub(crate) fn ok(reply: Reply) -> Result<Reply, AiError> {
    if (200..300).contains(&reply.status) {
        return Ok(reply);
    }
    let body: String = reply.body.chars().take(600).collect();
    Err(AiError::Status { status: reply.status, body })
}
