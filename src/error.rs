use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use crate::scroll_trace::TraceError;

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
    ConfigChanged {
        path: PathBuf,
    },
    Trace {
        path: PathBuf,
        source: Box<TraceError>,
    },
    InvalidConfig(String),
    Permission(String),
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

    pub fn is_config_changed(&self) -> bool {
        matches!(self, Self::ConfigChanged { .. })
    }

    /// Stable coarse code for scripts and support reports. Human-readable
    /// messages may improve without forcing callers to parse prose.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "E_IO",
            Self::ConfigParse { .. } => "E_CONFIG_PARSE",
            Self::ConfigSerialize(_) => "E_CONFIG_SERIALIZE",
            Self::ConfigChanged { .. } => "E_CONFIG_CHANGED",
            Self::Trace { .. } => "E_TRACE",
            Self::InvalidConfig(_) => "E_CONFIG_INVALID",
            Self::Permission(_) => "E_PERMISSION",
            Self::Platform(_) => "E_PLATFORM",
            Self::Usage(_) => "E_USAGE",
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
            Self::ConfigChanged { path } => write!(
                f,
                "config `{}` changed in another process; reload it before saving again",
                path.display()
            ),
            Self::Trace { path, source } => {
                write!(f, "invalid scroll trace `{}`: {source}", path.display())
            }
            Self::InvalidConfig(message) => write!(f, "invalid config: {message}"),
            Self::Permission(message) => write!(f, "permission required: {message}"),
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
            Self::Trace { source, .. } => Some(source.as_ref()),
            Self::ConfigChanged { .. }
            | Self::InvalidConfig(_)
            | Self::Permission(_)
            | Self::Platform(_)
            | Self::Usage(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_family_has_a_stable_code() {
        let cases = [
            (
                AppError::io("read", "config.toml", std::io::Error::other("no")),
                "E_IO",
            ),
            (
                AppError::ConfigChanged {
                    path: PathBuf::from("config.toml"),
                },
                "E_CONFIG_CHANGED",
            ),
            (
                AppError::InvalidConfig("bad".to_string()),
                "E_CONFIG_INVALID",
            ),
            (AppError::Permission("missing".to_string()), "E_PERMISSION"),
            (AppError::Platform("bad".to_string()), "E_PLATFORM"),
            (AppError::Usage("bad".to_string()), "E_USAGE"),
        ];

        for (error, expected) in cases {
            assert_eq!(error.code(), expected);
        }
    }
}
