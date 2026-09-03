use async_trait::async_trait;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::FastMcpError;
use crate::protocol::{
    ContentItem, GetPromptResult, PromptArgument, PromptDefinition, PromptMessage,
};

#[async_trait]
pub trait Prompt: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn arguments(&self) -> Vec<PromptArgument>;
    async fn get(&self, arguments: Value) -> Result<GetPromptResult, FastMcpError>;

    fn definition(&self) -> PromptDefinition {
        PromptDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            arguments: Some(self.arguments()),
        }
    }
}

pub type BoxedPromptFn = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<GetPromptResult, FastMcpError>> + Send>>
        + Send
        + Sync,
>;

pub struct SimplePrompt {
    name: String,
    description: String,
    arguments: Vec<PromptArgument>,
    handler: BoxedPromptFn,
}

impl SimplePrompt {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        arguments: Vec<PromptArgument>,
        handler: F,
    ) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<GetPromptResult, FastMcpError>> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            arguments,
            handler: Arc::new(move |args| Box::pin(handler(args))),
        }
    }
}

#[async_trait]
impl Prompt for SimplePrompt {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn arguments(&self) -> Vec<PromptArgument> {
        self.arguments.clone()
    }

    async fn get(&self, arguments: Value) -> Result<GetPromptResult, FastMcpError> {
        (self.handler)(arguments).await
    }
}

pub fn create_code_review_prompt() -> Box<dyn Prompt> {
    Box::new(SimplePrompt::new(
        "code_review",
        "Perform a rigorous security, memory safety, and performance code review",
        vec![PromptArgument {
            name: "code".to_string(),
            description: Some("The code snippet or diff to review".to_string()),
            required: true,
        }],
        |args: Value| async move {
            let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let prompt_text = format!(
                "Please perform a deep systems-engineering code review of the following code:\n\n\
                ```\n{}\n```\n\n\
                Analyze:\n\
                1. Memory safety, zero-copy optimization, and allocation bottlenecks.\n\
                2. Error propagation, panics, and edge cases.\n\
                3. Security vulnerabilities (path traversal, input sanitization).\n\
                4. Idiomatic Rust / performance best practices.",
                code
            );

            Ok(GetPromptResult {
                description: Some("Rigorous Systems Code Review Prompt".to_string()),
                messages: vec![PromptMessage {
                    role: "user".to_string(),
                    content: ContentItem::Text { text: prompt_text },
                }],
            })
        },
    ))
}
