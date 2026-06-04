use std::fmt;

#[derive(Debug)]
pub enum HonchoError {
    Http { status: u16, body: String },
    Request(reqwest::Error),
    Json(serde_json::Error),
}

impl fmt::Display for HonchoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { status, body } => write!(f, "HTTP error {status}: {body}"),
            Self::Request(e) => write!(f, "Request failed: {e}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for HonchoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for HonchoError {
    fn from(e: reqwest::Error) -> Self {
        Self::Request(e)
    }
}

impl From<serde_json::Error> for HonchoError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}


pub type Result<T> = std::result::Result<T, HonchoError>;
