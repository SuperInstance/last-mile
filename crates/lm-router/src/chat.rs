//! What we ask a pool, and what it answers.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::pool::Pool;

/// Who is speaking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The instructions to the model.
    System,
    /// The human (or the squad) speaking.
    User,
    /// The model speaking.
    Assistant,
}

impl Role {
    /// The wire name of the role.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One message in the conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Who is speaking.
    pub role: Role,
    /// What they said.
    pub content: String,
}

impl ChatMessage {
    /// A message with a role and a text.
    pub fn new(role: Role, content: impl Into<String>) -> ChatMessage {
        ChatMessage {
            role,
            content: content.into(),
        }
    }
    /// A system message.
    pub fn system(content: impl Into<String>) -> ChatMessage {
        ChatMessage::new(Role::System, content)
    }

    /// A user message.
    pub fn user(content: impl Into<String>) -> ChatMessage {
        ChatMessage::new(Role::User, content)
    }

    /// An assistant message.
    pub fn assistant(content: impl Into<String>) -> ChatMessage {
        ChatMessage::new(Role::Assistant, content)
    }
}

/// A request to a pool.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatReq {
    /// Overrides the pool's configured model when it is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The conversation, oldest first.
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
    /// The most tokens the answer may take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// The options the answer must come from.
    ///
    /// System One answers typed questions, not free text, so a pool that
    /// speaks System One needs to know the options. The chat-completion
    /// providers ignore this.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl ChatReq {
    /// A request with one user message.
    pub fn user(content: impl Into<String>) -> ChatReq {
        ChatReq {
            messages: vec![ChatMessage::user(content)],
            ..ChatReq::default()
        }
    }

    /// A request with one system message and one user message.
    pub fn system_and_user(system: impl Into<String>, user: impl Into<String>) -> ChatReq {
        ChatReq {
            messages: vec![ChatMessage::system(system), ChatMessage::user(user)],
            ..ChatReq::default()
        }
    }

    /// Add a message.
    pub fn push(mut self, role: Role, content: impl Into<String>) -> ChatReq {
        self.messages.push(ChatMessage::new(role, content));
        self
    }

    /// Ask for another model than the pool's default.
    pub fn with_model(mut self, model: impl Into<String>) -> ChatReq {
        self.model = Some(model.into());
        self
    }

    /// Cap the answer's length.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> ChatReq {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Restrict the answer to these options (System One).
    pub fn with_choices(mut self, choices: &[&str]) -> ChatReq {
        self.choices = choices.iter().map(|c| (*c).to_string()).collect();
        self
    }

    /// The last user message, which is what a judge is asked.
    pub fn last_user_message(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
    }
}

/// What a call cost, as the provider counted it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// The tokens of the request.
    pub input_tokens: u32,
    /// The tokens of the answer.
    pub output_tokens: u32,
}

impl Usage {
    /// The tokens of the request and the answer together.
    pub fn total(&self) -> u64 {
        self.input_tokens as u64 + self.output_tokens as u64
    }
}

/// A pool's answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatResp {
    /// The pool that answered.
    pub pool: Pool,
    /// The model that answered.
    pub model: String,
    /// What it said.
    pub text: String,
    /// What it cost.
    pub usage: Usage,
    /// Why it stopped, in the provider's words.
    pub finish_reason: Option<String>,
    /// How sure it was, in thousandths (860 means 0.86), when the provider
    /// said.
    pub confidence: Option<u32>,
}

impl fmt::Display for ChatMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.role, self.content)
    }
}

impl fmt::Display for ChatReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChatReq({} messages", self.messages.len())?;
        if let Some(model) = &self.model {
            write!(f, ", model {model}")?;
        }
        if !self.choices.is_empty() {
            write!(f, ", {} choices", self.choices.len())?;
        }
        f.write_str(")")
    }
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Usage({} in, {} out)",
            self.input_tokens, self.output_tokens
        )
    }
}

impl fmt::Display for ChatResp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChatResp({}/{}: {})", self.pool, self.model, self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_carries_its_conversation() {
        let req = ChatReq::system_and_user("be brief", "what now?")
            .with_max_tokens(64)
            .with_choices(&["hold", "advance"]);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.max_tokens, Some(64));
        assert_eq!(req.choices, vec!["hold", "advance"]);
        assert_eq!(req.last_user_message(), Some("what now?"));
    }

    #[test]
    fn usage_totals_both_halves() {
        let u = Usage {
            input_tokens: 690,
            output_tokens: 123,
        };
        assert_eq!(u.total(), 813);
    }
}
