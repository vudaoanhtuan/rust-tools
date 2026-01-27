//! share_drive - A CLI tool for interacting with Google Shared Drive.
//!
//! This library provides functionality to:
//! - List files in a Shared Drive folder
//! - Upload files to a Shared Drive folder (with glob pattern support)
//! - Download files from Shared Drive to local filesystem
//!
//! # Example
//!
//! ```no_run
//! use share_drive::{Authenticator, SharedDriveClient};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let auth = Authenticator::from_file("service-account.json")?;
//!     let client = SharedDriveClient::new(auth, "drive-id".to_string());
//!
//!     let files = client.list_files("folder-id").await?;
//!     for file in files {
//!         println!("{}", file);
//!     }
//!
//!     Ok(())
//! }
//! ```

pub mod auth;
pub mod client;
pub mod error;
pub mod models;
pub mod url_parser;

// Re-exports for convenience
pub use auth::Authenticator;
pub use client::SharedDriveClient;
pub use error::{DriveError, Result};
pub use models::FileMetadata;
pub use url_parser::extract_id;
