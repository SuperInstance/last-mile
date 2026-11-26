//! The provider dialects: how each pool is asked, and how its answer is read.
//!
//! Four shapes are spoken here:
//!
//! | Pool | Shape | Endpoint | Auth |
//! |---|---|---|---|
//! | Z.AI, MiniMax, Kimi | OpenAI chat completions | `{base}/chat/completions` | `Authorization: Bearer` |
//! | Claude | Anthropic Messages | `{base}/v1/messages` | `x-api-key` |
//! | Jev | System One | `{base}/v1/systemone` | `Authorization: Bearer` |
//! | Stub | none | none | none |
//!
//! The base URLs and the model names come from `router.toml`; nothing about
//! a provider is written down here.
//!
//! System One is not a chat model: it answers *typed questions* over a state
//! (see `docs/research/poc/jev/`). A chat request is mapped onto it as one
//! question: the conversation becomes the state, the last user message
//! becomes the question's instructions, and the request's `choices` become
//! the options of a choice question. With no `choices`, the question is a
//! noul ("is this true?"), whose answer is "yes" or "no".

use std::fmt;

use serde_json::{json, Map, Value};

use crate::chat::{ChatReq, ChatResp, Role, Usage};
use crate::config::{PoolConfig, ProviderKind};
use crate::error::RouterError;
use crate::pool::Pool;

/// The most tokens an answer may take when the request does not say.
pub const DEFAULT_MAX_TOKENS: u32 = 1024;

/// The question System One is asked, when the request has options.
pub const SYSTEM_ONE_QUESTION: &str = "q0";

/// The HTTP call a pool needs: where, with which headers, and what body.
///
/// The headers hold the key, so this type's `Debug` prints the *names* of
/// the headers and never their values.
#[derive(Clone, PartialEq)]
pub struct HttpCall {
    /// The full URL of the endpoint.
    pub url: String,
    /// The headers to send. The value of the auth header is the key.
    pub headers: Vec<(String, String)>,
    /// The JSON body to send.
    pub body: Value,
}

impl HttpCall {
    /// The value of a header, looked up without regard to case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl fmt::Debug for HttpCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self
            .headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        f.debug_struct("HttpCall")
            .field("url", &self.url)
            .field("headers", &names)
            .field("body", &self.body)
            .finish()
    }
}

impl fmt::Display for HttpCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "POST {}", self.url)
    }
}

/// The path the endpoint of a dialect adds to the base URL.
fn endpoint(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAi => "/chat/completions",
        ProviderKind::Anthropic => "/v1/messages",
        ProviderKind::SystemOne => "/v1/systemone",
        ProviderKind::Stub => "",
    }
}

/// Join the base URL and the endpoint's path.
pub fn endpoint_url(base_url: &str, kind: ProviderKind) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), endpoint(kind))
}

/// Build the call for a pool.
///
/// `key` is `None` for a pool that needs no key (the stub). It is put in
/// the auth header and nowhere else.
pub fn build_call(pool: Pool, cfg: &PoolConfig, req: &ChatReq, key: Option<&str>) -> HttpCall {
    let model = req.model.clone().unwrap_or_else(|| cfg.model.clone());
    let url = endpoint_url(&cfg.base_url, cfg.kind);
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];

    let body = match cfg.kind {
        ProviderKind::OpenAi => {
            if let Some(key) = key {
                headers.push(("authorization".to_string(), format!("Bearer {key}")));
            }
            let mut body = Map::new();
            body.insert("model".to_string(), json!(model));
            body.insert("messages".to_string(), json!(openai_messages(req)));
            if let Some(max) = req.max_tokens {
                body.insert("max_tokens".to_string(), json!(max));
            }
            Value::Object(body)
        }
        ProviderKind::Anthropic => {
            if let Some(key) = key {
                headers.push(("x-api-key".to_string(), key.to_string()));
            }
            headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
            let mut body = Map::new();
            body.insert("model".to_string(), json!(model));
            body.insert(
                "max_tokens".to_string(),
                json!(req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS)),
            );
            let system = req
                .messages
                .iter()
                .filter(|m| m.role == Role::System)
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            if !system.is_empty() {
                body.insert("system".to_string(), json!(system));
            }
            body.insert("messages".to_string(), json!(anthropic_messages(req)));
            Value::Object(body)
        }
        ProviderKind::SystemOne => {
            if let Some(key) = key {
                headers.push(("authorization".to_string(), format!("Bearer {key}")));
            }
            let mut body = Map::new();
            body.insert("model".to_string(), json!(model));
            body.insert("state".to_string(), json!(systemone_state(req)));
            let mut questions = Map::new();
            questions.insert(SYSTEM_ONE_QUESTION.to_string(), systemone_question(req));
            body.insert("questions".to_string(), Value::Object(questions));
            Value::Object(body)
        }
        ProviderKind::Stub => Value::Object(Map::new()),
    };

    HttpCall {
        url: match cfg.kind {
            // The stub never makes a call; its URL says so.
            ProviderKind::Stub => format!("stub://{pool}"),
            _ => url,
        },
        headers,
        body,
    }
}

fn openai_messages(req: &ChatReq) -> Vec<Value> {
    req.messages
        .iter()
        .map(|m| json!({"role": m.role.as_str(), "content": m.content}))
        .collect()
}

fn anthropic_messages(req: &ChatReq) -> Vec<Value> {
    req.messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| {
            let role = if m.role == Role::Assistant {
                "assistant"
            } else {
                "user"
            };
            json!({"role": role, "content": m.content})
        })
        .collect()
}

fn systemone_state(req: &ChatReq) -> Value {
    json!({
        "messages": req
            .messages
            .iter()
            .map(|m| json!({"role": m.role.as_str(), "content": m.content}))
            .collect::<Vec<_>>(),
    })
}

fn systemone_question(req: &ChatReq) -> Value {
    let instructions = req.last_user_message().unwrap_or("");
    if req.choices.is_empty() {
        json!({"type": "noul", "instructions": instructions})
    } else {
        json!({"type": "choice", "options": req.choices, "instructions": instructions})
    }
}

/// Read a provider's answer.
///
/// `model` is the model that was asked for; it is used when the provider
/// does not name the model back.
pub fn parse_response(
    pool: Pool,
    kind: ProviderKind,
    model: &str,
    body: &Value,
) -> Result<ChatResp, RouterError> {
    match kind {
        ProviderKind::OpenAi => parse_openai(pool, model, body),
        ProviderKind::Anthropic => parse_anthropic(pool, model, body),
        ProviderKind::SystemOne => parse_systemone(pool, model, body),
        ProviderKind::Stub => Err(RouterError::Malformed {
            pool,
            why: "the stub does not answer over HTTP".to_string(),
        }),
    }
}

fn malformed(pool: Pool, why: &str) -> RouterError {
    RouterError::Malformed {
        pool,
        why: why.to_string(),
    }
}

/// The text of a content block, whether it is a string or a list of parts.
fn content_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn number(value: &Value, path: &[&str]) -> Option<u32> {
    let mut node = value;
    for key in path {
        node = node.get(key)?;
    }
    node.as_f64().map(|v| v.round().max(0.0) as u32)
}

fn parse_openai(pool: Pool, model: &str, body: &Value) -> Result<ChatResp, RouterError> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| malformed(pool, "no choices in the answer"))?;
    let text = choice
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(content_text)
        .or_else(|| choice.get("text").and_then(content_text))
        .ok_or_else(|| malformed(pool, "no content in the answer"))?;
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_string);
    let usage = Usage {
        input_tokens: number(body, &["usage", "prompt_tokens"])
            .or_else(|| number(body, &["usage", "input_tokens"]))
            .unwrap_or(0),
        output_tokens: number(body, &["usage", "completion_tokens"])
            .or_else(|| number(body, &["usage", "output_tokens"]))
            .unwrap_or(0),
    };
    Ok(ChatResp {
        pool,
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_string(),
        text,
        usage,
        finish_reason,
        confidence: None,
    })
}

fn parse_anthropic(pool: Pool, model: &str, body: &Value) -> Result<ChatResp, RouterError> {
    let blocks = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed(pool, "no content blocks in the answer"))?;
    let text = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        return Err(malformed(pool, "no text in the content blocks"));
    }
    Ok(ChatResp {
        pool,
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_string(),
        text,
        usage: Usage {
            input_tokens: number(body, &["usage", "input_tokens"]).unwrap_or(0),
            output_tokens: number(body, &["usage", "output_tokens"]).unwrap_or(0),
        },
        finish_reason: body
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        confidence: None,
    })
}

fn parse_systemone(pool: Pool, model: &str, body: &Value) -> Result<ChatResp, RouterError> {
    let answers = body
        .get("answers")
        .and_then(Value::as_object)
        .ok_or_else(|| malformed(pool, "no answers in the answer"))?;
    let answer = answers
        .get(SYSTEM_ONE_QUESTION)
        .or_else(|| answers.values().next())
        .ok_or_else(|| malformed(pool, "no answers in the answer"))?;

    let text = match answer.get("choice").and_then(Value::as_str) {
        Some(choice) => choice.to_string(),
        None => match answer.get("noul").and_then(Value::as_f64) {
            Some(noul) => {
                if noul >= 0.5 {
                    "yes".to_string()
                } else {
                    "no".to_string()
                }
            }
            None => answer
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| malformed(pool, "the answer is neither a choice nor a noul"))?,
        },
    };

    let confidence = answer
        .get("confidence")
        .and_then(Value::as_f64)
        .or_else(|| answer.get("noul").and_then(Value::as_f64))
        .map(|v| (v * 1000.0).round().clamp(0.0, 1000.0) as u32);

    Ok(ChatResp {
        pool,
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_string(),
        text,
        usage: Usage {
            input_tokens: number(body, &["usage", "input_tokens"]).unwrap_or(0),
            output_tokens: number(body, &["usage", "output_tokens"]).unwrap_or(0),
        },
        finish_reason: None,
        confidence,
    })
}

/// Whether a provider's error body says the pool is over its limit.
///
/// A 429 is a limit by itself. Anything else is a limit when the body says
/// so: the word "limit" is what the providers use for a spent plan, a
/// rate limit or a quota, and no provider tells us which.
pub fn mentions_limit(body: &str) -> bool {
    body.to_ascii_lowercase().contains("limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoolConfig;

    fn openai_pool() -> PoolConfig {
        PoolConfig {
            kind: ProviderKind::OpenAi,
            base_url: "https://example.invalid/v1".to_string(),
            model: "glm-test".to_string(),
            key_name: Some("ZAI_API_KEY".to_string()),
            ..PoolConfig::default()
        }
    }

    #[test]
    fn the_openai_call_asks_the_chat_endpoint() {
        let req = ChatReq::system_and_user("be brief", "what now?");
        let call = build_call(Pool::Zai, &openai_pool(), &req, Some("sk-secret"));
        assert_eq!(call.url, "https://example.invalid/v1/chat/completions");
        assert_eq!(call.header("authorization"), Some("Bearer sk-secret"));
        assert_eq!(call.header("content-type"), Some("application/json"));
        assert_eq!(call.body["model"], "glm-test");
        assert_eq!(call.body["messages"][0]["role"], "system");
        assert_eq!(call.body["messages"][1]["content"], "what now?");
    }

    #[test]
    fn the_call_never_shows_the_key() {
        let req = ChatReq::user("hello");
        let call = build_call(Pool::Zai, &openai_pool(), &req, Some("sk-secret"));
        assert!(!format!("{call:?}").contains("sk-secret"));
        assert!(!format!("{call}").contains("sk-secret"));
        assert!(format!("{call:?}").contains("authorization"));
    }

    #[test]
    fn the_anthropic_call_sends_the_key_its_own_way() {
        let cfg = PoolConfig {
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-test".to_string(),
            ..PoolConfig::default()
        };
        let req = ChatReq::system_and_user("be brief", "what now?");
        let call = build_call(Pool::Claude, &cfg, &req, Some("sk-secret"));
        assert_eq!(call.url, "https://api.anthropic.com/v1/messages");
        assert_eq!(call.header("x-api-key"), Some("sk-secret"));
        assert_eq!(call.header("anthropic-version"), Some("2023-06-01"));
        assert_eq!(call.body["system"], "be brief");
        assert_eq!(call.body["messages"][0]["role"], "user");
        assert_eq!(call.body["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn the_system_one_call_asks_one_question() {
        let cfg = PoolConfig {
            kind: ProviderKind::SystemOne,
            base_url: "https://api.typesafe.ai".to_string(),
            model: "jev-latest".to_string(),
            ..PoolConfig::default()
        };
        let req =
            ChatReq::system_and_user("be brief", "is the plan good?").with_choices(&["yes", "no"]);
        let call = build_call(Pool::Jev, &cfg, &req, Some("sk-secret"));
        assert_eq!(call.url, "https://api.typesafe.ai/v1/systemone");
        let question = &call.body["questions"]["q0"];
        assert_eq!(question["type"], "choice");
        assert_eq!(question["options"][0], "yes");
        assert_eq!(question["instructions"], "is the plan good?");
        assert_eq!(call.body["state"]["messages"][0]["role"], "system");
    }

    #[test]
    fn a_system_one_request_without_options_asks_a_noul() {
        let cfg = PoolConfig {
            kind: ProviderKind::SystemOne,
            base_url: "https://api.typesafe.ai".to_string(),
            model: "jev-latest".to_string(),
            ..PoolConfig::default()
        };
        let call = build_call(Pool::Jev, &cfg, &ChatReq::user("is it true?"), Some("k"));
        assert_eq!(call.body["questions"]["q0"]["type"], "noul");
    }

    #[test]
    fn the_openai_shape_is_read() {
        let body = json!({
            "model": "glm-4.6",
            "choices": [{"message": {"role": "assistant", "content": "hold the beacon"},
                         "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 690, "completion_tokens": 123, "total_tokens": 813}
        });
        let resp = parse_response(Pool::Zai, ProviderKind::OpenAi, "glm-4.6", &body).unwrap();
        assert_eq!(resp.text, "hold the beacon");
        assert_eq!(resp.usage.input_tokens, 690);
        assert_eq!(resp.usage.output_tokens, 123);
        assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.model, "glm-4.6");
        assert_eq!(resp.pool, Pool::Zai);
    }

    #[test]
    fn the_anthropic_shape_is_read() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "content": [{"type": "text", "text": "hold "}, {"type": "text", "text": "the beacon"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 42, "output_tokens": 7}
        });
        let resp =
            parse_response(Pool::Claude, ProviderKind::Anthropic, "claude-test", &body).unwrap();
        assert_eq!(resp.text, "hold the beacon");
        assert_eq!(resp.usage.total(), 49);
        assert_eq!(resp.finish_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn the_system_one_shape_is_read() {
        let body = json!({
            "answers": {"q0": {"type": "choice", "choice": "advance", "confidence": 0.91}},
            "usage": {"input_tokens": 690, "output_tokens": 123}
        });
        let resp = parse_response(Pool::Jev, ProviderKind::SystemOne, "jev-latest", &body).unwrap();
        assert_eq!(resp.text, "advance");
        assert_eq!(resp.confidence, Some(910));
        assert_eq!(resp.usage.total(), 813);
    }

    #[test]
    fn a_system_one_noul_is_read_as_yes_or_no() {
        let body = json!({"answers": {"q0": {"type": "noul", "noul": 0.86}}});
        let resp = parse_response(Pool::Jev, ProviderKind::SystemOne, "jev-latest", &body).unwrap();
        assert_eq!(resp.text, "yes");
        assert_eq!(resp.confidence, Some(860));
    }

    #[test]
    fn an_answer_without_content_is_malformed() {
        let body = json!({"choices": []});
        let err = parse_response(Pool::Zai, ProviderKind::OpenAi, "glm-test", &body).unwrap_err();
        assert!(matches!(err, RouterError::Malformed { .. }));
    }

    #[test]
    fn a_limit_is_recognised() {
        assert!(mentions_limit("rate limit reached"));
        assert!(mentions_limit("Your LIMIT has been reached"));
        assert!(!mentions_limit("the model is thinking"));
    }
}
