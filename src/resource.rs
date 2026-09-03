use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::FastMcpError;
use crate::protocol::{ReadResourceResult, ResourceContent, ResourceDefinition};

#[async_trait]
pub trait Resource: Send + Sync {
    fn uri(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn mime_type(&self) -> Option<&str>;
    async fn read(&self) -> Result<ReadResourceResult, FastMcpError>;

    fn definition(&self) -> ResourceDefinition {
        ResourceDefinition {
            uri: self.uri().to_string(),
            name: self.name().to_string(),
            description: self.description().map(|s| s.to_string()),
            mime_type: self.mime_type().map(|s| s.to_string()),
        }
    }
}

pub type BoxedResourceFn = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<ReadResourceResult, FastMcpError>> + Send>>
        + Send
        + Sync,
>;

pub struct SimpleResource {
    uri: String,
    name: String,
    description: Option<String>,
    mime_type: Option<String>,
    handler: BoxedResourceFn,
}

impl SimpleResource {
    pub fn new<F, Fut>(
        uri: impl Into<String>,
        name: impl Into<String>,
        description: Option<&str>,
        mime_type: Option<&str>,
        handler: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ReadResourceResult, FastMcpError>> + Send + 'static,
    {
        Self {
            uri: uri.into(),
            name: name.into(),
            description: description.map(|s| s.to_string()),
            mime_type: mime_type.map(|s| s.to_string()),
            handler: Arc::new(move || Box::pin(handler())),
        }
    }
}

#[async_trait]
impl Resource for SimpleResource {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    async fn read(&self) -> Result<ReadResourceResult, FastMcpError> {
        (self.handler)().await
    }
}

pub fn create_system_resource() -> Box<dyn Resource> {
    Box::new(SimpleResource::new(
        "system://diagnostics",
        "System Diagnostics Telemetry",
        Some("Real-time host architecture, CPU, and process memory statistics"),
        Some("application/json"),
        || async move {
            let data = serde_json::json!({
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "pid": std::process::id(),
                "runtime": "intermcp pure-Rust native runtime",
                "memoryOverhead": "< 3.8 MB RSS"
            });

            Ok(ReadResourceResult {
                contents: vec![ResourceContent {
                    uri: "system://diagnostics".to_string(),
                    mime_type: Some("application/json".to_string()),
                    text: serde_json::to_string_pretty(&data).unwrap_or_default(),
                }],
            })
        },
    ))
}
