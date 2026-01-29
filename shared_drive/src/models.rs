//! Data models for Google Drive API responses.

use serde::{Deserialize, Serialize};

/// Metadata for a file or folder in Google Drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub web_view_link: Option<String>,
    #[serde(default, deserialize_with = "deserialize_size")]
    pub size: Option<u64>,
}

fn deserialize_size<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) => s.parse::<u64>().map(Some).map_err(serde::de::Error::custom),
        None => Ok(None),
    }
}

impl std::fmt::Display for FileMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size_str = self
            .size
            .map(|s| format_size(s))
            .unwrap_or_else(|| "-".to_string());
        let mime = self.mime_type.as_deref().unwrap_or("-");
        write!(f, "{}\t{}\t{}\t{}", self.id, size_str, mime, self.name)
    }
}

/// Format bytes into human-readable size.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format seconds into human-readable time (e.g., "2m 15s", "1h 5m", "< 1s").
pub fn format_eta(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--".to_string();
    }

    let secs = seconds.round() as u64;

    if secs == 0 {
        return "< 1s".to_string();
    }

    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let remaining_secs = secs % 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, remaining_secs)
    } else {
        format!("{}s", remaining_secs)
    }
}

/// Response from the files.list API endpoint.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileListResponse {
    #[serde(default)]
    pub files: Vec<FileMetadata>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

/// Shared Drive metadata.
#[derive(Debug, Deserialize)]
pub struct Drive {
    pub id: String,
    pub name: String,
}

/// User information from the about API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub email_address: Option<String>,
    pub display_name: Option<String>,
}

/// Storage quota information.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageQuota {
    pub limit: Option<String>,
    pub usage: Option<String>,
    pub usage_in_drive: Option<String>,
    pub usage_in_drive_trash: Option<String>,
}

/// About response from the Drive API.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub user: User,
    pub storage_quota: StorageQuota,
}

/// Google API error response.
#[derive(Debug, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
pub struct ApiErrorDetail {
    pub code: u16,
    pub message: String,
}

/// Service account credentials from JSON file.
#[derive(Debug, Deserialize)]
pub struct ServiceAccountCredentials {
    pub client_email: String,
    pub private_key: String,
    pub token_uri: Option<String>,
}

/// OAuth2 token response.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(1048576), "1.00 MB");
        assert_eq!(format_size(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_eta() {
        assert_eq!(format_eta(0.4), "< 1s");
        assert_eq!(format_eta(5.0), "5s");
        assert_eq!(format_eta(65.0), "1m 5s");
        assert_eq!(format_eta(3665.0), "1h 1m");
        assert_eq!(format_eta(f64::INFINITY), "--");
        assert_eq!(format_eta(-5.0), "--");
        assert_eq!(format_eta(f64::NAN), "--");
    }

    #[test]
    fn test_file_metadata_deserialize() {
        let json = r#"{
            "id": "abc123",
            "name": "test.txt",
            "mimeType": "text/plain",
            "webViewLink": "https://drive.google.com/file/d/abc123/view",
            "size": "1024"
        }"#;

        let metadata: FileMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.id, "abc123");
        assert_eq!(metadata.name, "test.txt");
        assert_eq!(metadata.mime_type, Some("text/plain".to_string()));
        assert_eq!(metadata.size, Some(1024));
    }

    #[test]
    fn test_file_metadata_display() {
        let metadata = FileMetadata {
            id: "abc123".to_string(),
            name: "test.txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            web_view_link: None,
            size: Some(1024),
        };

        let display = format!("{}", metadata);
        assert!(display.contains("abc123"));
        assert!(display.contains("test.txt"));
        assert!(display.contains("1.00 KB"));
    }
}
