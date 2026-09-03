use thiserror::Error;

#[derive(Error, Debug)]
pub enum FastMcpError {
    #[error("JSON-RPC serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Tool execution failed: {0}")]
    ToolExecution(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl FastMcpError {
    pub fn error_code(&self) -> i32 {
        match self {
            FastMcpError::Serialization(_) => -32700,  // Parse error
            FastMcpError::InvalidRequest(_) => -32600, // Invalid Request
            FastMcpError::MethodNotFound(_) => -32601, // Method not found
            FastMcpError::ToolNotFound(_) => -32602,   // Invalid params
            FastMcpError::ToolExecution(_) => -32000,  // Server error
            FastMcpError::Internal(_) => -32603,       // Internal error
            FastMcpError::Io(_) => -32001,
        }
    }
}
