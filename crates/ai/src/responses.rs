//! The Responses API call that makes an image, with the plan-backed token.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use crate::chatgpt::RESOURCE;
use crate::{AiError, http};

/// What to make.
#[derive(Debug, Clone, Default)]
pub struct ImageRequest<'a> {
    /// The model that drives the request (the plan's models are listed by [`list_models`]).
    pub model: &'a str,
    pub prompt: &'a str,
    /// `1024x1024`, `1536x1024`, `1024x1536` or `auto`.
    pub size: Option<&'a str>,
    /// `transparent`, `opaque` or `auto`.
    pub background: Option<&'a str>,
}

/// The JSON body of a Responses request that asks for an image.
pub fn request_body(req: &ImageRequest<'_>) -> Value {
    let mut tool = json!({"type": "image_generation"});
    if let (Some(size), Some(obj)) = (req.size, tool.as_object_mut()) {
        obj.insert("size".into(), json!(size));
    }
    if let (Some(bg), Some(obj)) = (req.background, tool.as_object_mut()) {
        obj.insert("background".into(), json!(bg));
    }
    json!({
        "model": req.model,
        "input": req.prompt,
        "tools": [tool],
        "tool_choice": {"type": "image_generation"},
        // A plan-backed request is streamed and not stored.
        "store": false,
        "stream": true,
    })
}

/// The images (decoded) a Responses reply holds, or its error. Accepts the JSON of a finished
/// response and the server-sent events of a streamed one (read through `response.completed`).
pub fn parse_images(body: &str) -> Result<Vec<Vec<u8>>, AiError> {
    if body.trim_start().starts_with('{') {
        return images_in(&serde_json::from_str(body).map_err(|e| AiError::Reply(format!("the reply is not JSON: {e}")))?);
    }
    parse_stream(body)
}

/// The images of a `Value` with `output` items (a finished response).
fn images_in(v: &Value) -> Result<Vec<Vec<u8>>, AiError> {
    if let Some(msg) = v.pointer("/error/message").and_then(Value::as_str) {
        return Err(AiError::Reply(msg.to_string()));
    }
    let items = v.get("output").and_then(Value::as_array).ok_or_else(|| AiError::Reply("the reply has no output".into()))?;
    items.iter().filter_map(image_of).collect()
}

/// The decoded image of an `image_generation_call` item.
fn image_of(item: &Value) -> Option<Result<Vec<u8>, AiError>> {
    if item.get("type").and_then(Value::as_str) != Some("image_generation_call") {
        return None;
    }
    let b64 = item.get("result").and_then(Value::as_str)?;
    Some(STANDARD.decode(b64).map_err(|e| AiError::Reply(format!("the image is not base64: {e}"))))
}

/// Server-sent events: the finished items, and the failure when the stream reports one.
fn parse_stream(body: &str) -> Result<Vec<Vec<u8>>, AiError> {
    let mut done: Vec<Vec<u8>> = Vec::new();
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
        let Ok(event) = serde_json::from_str::<Value>(data) else { continue };
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.done") => {
                if let Some(image) = event.get("item").and_then(image_of) {
                    done.push(image?);
                }
            }
            Some("response.completed") => {
                if let Some(response) = event.get("response") {
                    let all = images_in(response)?;
                    return Ok(if all.is_empty() { done } else { all });
                }
            }
            Some("response.failed" | "error") => {
                let msg = event.pointer("/response/error/message").or_else(|| event.pointer("/error/message")).or_else(|| event.get("message"));
                return Err(AiError::Reply(msg.and_then(Value::as_str).unwrap_or("the request failed").to_string()));
            }
            _ => {}
        }
    }
    if done.is_empty() { Err(AiError::Reply("the stream ended without a completed response".into())) } else { Ok(done) }
}

/// Make one image. Fails when the plan does not allow it (the status and OpenAI's reason come back).
pub fn generate_image(access_token: &str, req: &ImageRequest<'_>) -> Result<Vec<u8>, AiError> {
    let reply = http::ok(http::post_json(&format!("{RESOURCE}/responses"), access_token, &request_body(req))?)?;
    parse_images(&reply.body)?.into_iter().next().ok_or_else(|| AiError::Reply("the reply holds no image".into()))
}

/// The model ids the connection can use.
pub fn list_models(access_token: &str) -> Result<Vec<String>, AiError> {
    let reply = http::ok(http::get(&format!("{RESOURCE}/models"), access_token)?)?;
    let v: Value = serde_json::from_str(&reply.body).map_err(|e| AiError::Reply(format!("the model list is not JSON: {e}")))?;
    let list =
        v.get("data").and_then(Value::as_array).map(|a| a.iter().filter_map(|m| m.get("id").and_then(Value::as_str)).map(str::to_string).collect());
    list.ok_or_else(|| AiError::Reply("the model list has no data".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_asks_for_the_image_tool() {
        let body = request_body(&ImageRequest { model: "m", prompt: "a fox", size: Some("1024x1024"), background: Some("transparent") });
        assert_eq!(body["model"], "m");
        assert_eq!(body["input"], "a fox");
        assert_eq!(body["tools"][0], json!({"type": "image_generation", "size": "1024x1024", "background": "transparent"}));
        assert_eq!(body["tool_choice"], json!({"type": "image_generation"}));
        assert_eq!((body["stream"].as_bool(), body["store"].as_bool()), (Some(true), Some(false)));
        let plain = request_body(&ImageRequest { model: "m", prompt: "p", ..Default::default() });
        assert_eq!(plain["tools"][0], json!({"type": "image_generation"}));
    }

    #[test]
    fn images_are_decoded_from_the_output() {
        let body = json!({"output": [
            {"type": "message", "content": []},
            {"type": "image_generation_call", "result": STANDARD.encode(b"PNGDATA")},
        ]})
        .to_string();
        assert_eq!(parse_images(&body).unwrap(), [b"PNGDATA".to_vec()]);
        assert_eq!(parse_images(&json!({"output": []}).to_string()).unwrap().len(), 0);
    }

    #[test]
    fn a_stream_is_read_to_its_completed_response() {
        let img = STANDARD.encode(b"FINAL");
        let events = format!(
            "event: response.created\ndata: {}\n\nevent: response.image_generation_call.partial_image\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
            json!({"type": "response.created"}),
            json!({"type": "response.image_generation_call.partial_image", "partial_image_b64": "AAAA"}),
            json!({"type": "response.completed", "response": {"output": [{"type": "image_generation_call", "result": img}]}}),
        );
        assert_eq!(parse_images(&events).unwrap(), [b"FINAL".to_vec()]);
        // A finished item is enough when the completed event leaves the output empty.
        let item = format!(
            "data: {}\n\ndata: {}\n",
            json!({"type": "response.output_item.done", "item": {"type": "image_generation_call", "result": STANDARD.encode(b"ITEM")}}),
            json!({"type": "response.completed", "response": {"output": []}}),
        );
        assert_eq!(parse_images(&item).unwrap(), [b"ITEM".to_vec()]);
        let failed = format!("data: {}\n", json!({"type": "response.failed", "response": {"error": {"message": "plan limit"}}}));
        assert!(matches!(parse_images(&failed), Err(AiError::Reply(m)) if m == "plan limit"));
        assert!(parse_images("data: {}\n").is_err());
    }

    #[test]
    fn errors_and_junk_are_reported() {
        assert!(matches!(parse_images(&json!({"error": {"message": "nope"}}).to_string()), Err(AiError::Reply(m)) if m == "nope"));
        assert!(parse_images("{").is_err());
        assert!(parse_images("{}").is_err());
        let bad = json!({"output": [{"type": "image_generation_call", "result": "!!!"}]}).to_string();
        assert!(parse_images(&bad).is_err());
    }
}
