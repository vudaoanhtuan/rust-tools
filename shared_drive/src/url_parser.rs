//! URL parser for extracting Google Drive IDs from URLs.

use regex::Regex;
use std::sync::LazyLock;

use crate::error::{DriveError, Result};

/// Regex patterns for Google Drive URLs.
static FOLDER_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://drive\.google\.com/drive/(?:u/\d+/)?folders/([a-zA-Z0-9_-]+)")
        .expect("Invalid folder URL regex")
});

static FILE_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://drive\.google\.com/file/d/([a-zA-Z0-9_-]+)")
        .expect("Invalid file URL regex")
});

static OPEN_URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://drive\.google\.com/open\?id=([a-zA-Z0-9_-]+)")
        .expect("Invalid open URL regex")
});

/// Valid Google Drive ID pattern (alphanumeric, underscore, hyphen).
static ID_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").expect("Invalid ID regex"));

/// Extract a Google Drive ID from a URL or validate a raw ID.
///
/// Supports the following URL formats:
/// - `https://drive.google.com/drive/folders/<ID>`
/// - `https://drive.google.com/drive/u/0/folders/<ID>`
/// - `https://drive.google.com/file/d/<ID>/view`
/// - `https://drive.google.com/open?id=<ID>`
/// - Raw ID string
///
/// # Examples
///
/// ```
/// use share_drive::url_parser::extract_id;
///
/// let id = extract_id("https://drive.google.com/drive/folders/1abc123").unwrap();
/// assert_eq!(id, "1abc123");
///
/// let id = extract_id("1abc123").unwrap();
/// assert_eq!(id, "1abc123");
/// ```
pub fn extract_id(url_or_id: &str) -> Result<String> {
    let trimmed = url_or_id.trim();

    // Try folder URL pattern
    if let Some(captures) = FOLDER_URL_REGEX.captures(trimmed) {
        if let Some(id) = captures.get(1) {
            return Ok(id.as_str().to_string());
        }
    }

    // Try file URL pattern
    if let Some(captures) = FILE_URL_REGEX.captures(trimmed) {
        if let Some(id) = captures.get(1) {
            return Ok(id.as_str().to_string());
        }
    }

    // Try open URL pattern
    if let Some(captures) = OPEN_URL_REGEX.captures(trimmed) {
        if let Some(id) = captures.get(1) {
            return Ok(id.as_str().to_string());
        }
    }

    // Check if it's a raw ID
    if ID_REGEX.is_match(trimmed) && !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }

    Err(DriveError::InvalidUrlOrId(url_or_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_folder_url() {
        let url = "https://drive.google.com/drive/folders/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn test_extract_folder_url_with_user() {
        let url = "https://drive.google.com/drive/u/0/folders/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");

        let url = "https://drive.google.com/drive/u/2/folders/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn test_extract_file_url() {
        let url = "https://drive.google.com/file/d/1abc123XYZ/view";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");

        let url = "https://drive.google.com/file/d/1abc123XYZ/view?usp=sharing";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn test_extract_open_url() {
        let url = "https://drive.google.com/open?id=1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn test_extract_raw_id() {
        assert_eq!(extract_id("1abc123XYZ").unwrap(), "1abc123XYZ");
        assert_eq!(extract_id("abc-123_XYZ").unwrap(), "abc-123_XYZ");
    }

    #[test]
    fn test_extract_with_whitespace() {
        assert_eq!(extract_id("  1abc123XYZ  ").unwrap(), "1abc123XYZ");
    }

    #[test]
    fn test_invalid_url() {
        assert!(extract_id("https://example.com/folder/123").is_err());
        assert!(extract_id("").is_err());
        assert!(extract_id("   ").is_err());
    }
}
