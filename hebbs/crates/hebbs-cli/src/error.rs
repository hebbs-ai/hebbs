use std::fmt;

/// Exit codes that serve as a stable contract for scripting.
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GENERAL_ERROR: i32 = 1;
pub const EXIT_USAGE_ERROR: i32 = 2;
pub const EXIT_CONNECTION_ERROR: i32 = 3;
pub const EXIT_NOT_FOUND: i32 = 4;
pub const EXIT_SERVER_ERROR: i32 = 5;

#[derive(Debug)]
pub enum CliError {
    ConnectionFailed { endpoint: String, source: String },
    ServerUnavailable { endpoint: String, message: String },
    NotFound { message: String },
    InvalidArgument { message: String },
    ServerError { message: String },
    DeadlineExceeded { timeout_ms: u64 },
    Internal { message: String },
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ConnectionFailed { .. } => EXIT_CONNECTION_ERROR,
            Self::ServerUnavailable { .. } => EXIT_CONNECTION_ERROR,
            Self::NotFound { .. } => EXIT_NOT_FOUND,
            Self::InvalidArgument { .. } => EXIT_USAGE_ERROR,
            Self::ServerError { .. } => EXIT_SERVER_ERROR,
            Self::DeadlineExceeded { .. } => EXIT_SERVER_ERROR,
            Self::Internal { .. } => EXIT_GENERAL_ERROR,
        }
    }

    #[cfg(feature = "grpc")]
    pub fn from_status(status: tonic::Status, endpoint: &str) -> Self {
        match status.code() {
            tonic::Code::Unavailable => Self::ServerUnavailable {
                endpoint: endpoint.to_string(),
                message: status.message().to_string(),
            },
            tonic::Code::NotFound => Self::NotFound {
                message: status.message().to_string(),
            },
            tonic::Code::InvalidArgument => Self::InvalidArgument {
                message: status.message().to_string(),
            },
            tonic::Code::DeadlineExceeded => Self::DeadlineExceeded { timeout_ms: 30000 },
            tonic::Code::Internal => Self::ServerError {
                message: status.message().to_string(),
            },
            _ => Self::ServerError {
                message: format!("{}: {}", status.code(), status.message()),
            },
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed { endpoint, source } => {
                write!(
                    f,
                    "Cannot connect to {}. Is hebbs-server running on that address? ({})",
                    endpoint, source
                )
            }
            Self::ServerUnavailable { endpoint, message } => {
                write!(
                    f,
                    "Server unavailable at {}. Is hebbs-server running? ({})",
                    endpoint, message
                )
            }
            Self::NotFound { message } => write!(f, "{}", message),
            Self::InvalidArgument { message } => write!(f, "{}", message),
            Self::ServerError { message } => {
                write!(f, "Server error: {}. This may be a bug in HEBBS.", message)
            }
            Self::DeadlineExceeded { timeout_ms } => {
                write!(
                    f,
                    "Request timed out after {}ms. Try increasing --timeout.",
                    timeout_ms
                )
            }
            Self::Internal { message } => write!(f, "Internal error: {}", message),
        }
    }
}

impl std::error::Error for CliError {}
