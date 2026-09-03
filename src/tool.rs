use async_trait::async_trait;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::FastMcpError;
use crate::protocol::{CallToolResult, ToolDefinition};

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn execute(&self, arguments: Value) -> Result<CallToolResult, FastMcpError>;

    /// Indicates whether tool execution results are deterministic and safe to cache.
    /// By default false to prevent returning stale data for mutable operations like file I/O.
    fn is_cacheable(&self) -> bool {
        false
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
        }
    }
}

pub type BoxedToolFn = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<CallToolResult, FastMcpError>> + Send>>
        + Send
        + Sync,
>;

pub struct SimpleTool {
    name: String,
    description: String,
    input_schema: Value,
    handler: BoxedToolFn,
    cacheable: bool,
}

impl SimpleTool {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        schema: Value,
        handler: F,
    ) -> Self
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<CallToolResult, FastMcpError>> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: schema,
            handler: Arc::new(move |args| Box::pin(handler(args))),
            cacheable: false,
        }
    }

    pub fn with_cacheable(mut self, cacheable: bool) -> Self {
        self.cacheable = cacheable;
        self
    }
}

#[async_trait]
impl Tool for SimpleTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn is_cacheable(&self) -> bool {
        self.cacheable
    }

    async fn execute(&self, arguments: Value) -> Result<CallToolResult, FastMcpError> {
        (self.handler)(arguments).await
    }
}
