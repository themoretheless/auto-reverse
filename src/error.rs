use std::error::Error;
use std::fmt;
use std::path::PathBuf;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    ConfigSerialize(toml::ser::Error),
    InvalidConfig(String),
    Platform(String),
    Usage(String),
}

impl AppError {
    pub fn io(action: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "failed to {action} `{}`: {source}", path.display()),
            Self::ConfigParse { path, source } => {
                write!(f, "failed to parse config `{}`: {source}", path.display())
            }
            Self::ConfigSerialize(source) => write!(f, "failed to serialize config: {source}"),
            Self::InvalidConfig(message) => write!(f, "invalid config: {message}"),
            Self::Platform(message) => write!(f, "platform error: {message}"),
            Self::Usage(message) => write!(f, "{message}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ConfigParse { source, .. } => Some(source),
            Self::ConfigSerialize(source) => Some(source),
            Self::InvalidConfig(_) | Self::Platform(_) | Self::Usage(_) => None,
        }
    }
}
