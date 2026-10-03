use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    Config { message: String },

    #[error("{message}")]
    Git { message: String },

    #[error("{message}")]
    Io { message: String },
}

impl AppError {
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    pub fn git(message: impl Into<String>) -> Self {
        Self::Git {
            message: message.into(),
        }
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Config { .. } => "config_error",
            Self::Git { .. } => "git_error",
            Self::Io { .. } => "io_error",
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Config { .. } => 2,
            Self::Git { .. } | Self::Io { .. } => 3,
        }
    }
}
