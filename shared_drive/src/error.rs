//! Error types for the share_drive crate.

use thiserror::Error;

/// Errors that can occur when interacting with Google Drive.
#[derive(Error, Debug)]
pub enum DriveError {
    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("Failed to read credentials file: {0}")]
    CredentialsFileError(#[from] std::io::Error),

    #[error("Failed to parse credentials JSON: {0}")]
    CredentialsParseError(#[from] serde_json::Error),

    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Invalid URL or ID: {0}")]
    InvalidUrlOrId(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("No files matched pattern: {0}")]
    NoFilesMatched(String),

    #[error("Glob pattern error: {0}")]
    GlobPatternError(#[from] glob::PatternError),

    #[error("JWT encoding error: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),

    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Token refresh failed: {0}")]
    TokenRefreshError(String),
}

/// Result type alias for DriveError.
pub type Result<T> = std::result::Result<T, DriveError>;
