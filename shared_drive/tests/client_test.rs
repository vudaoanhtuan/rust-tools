//! Tests for SharedDriveClient with mocked HTTP responses.

#[allow(unused_imports)]
use mockito::Server;
use serde_json::json;
use share_drive::models::{FileListResponse, FileMetadata, ServiceAccountCredentials};
use share_drive::Authenticator;
use std::io::Write;
use tempfile::NamedTempFile;


mod models {
    use super::*;

    #[test]
    fn test_file_metadata_deserialization() {
        let json = json!({
            "id": "file123",
            "name": "document.pdf",
            "mimeType": "application/pdf",
            "webViewLink": "https://drive.google.com/file/d/file123/view",
            "size": "2048"
        });

        let metadata: FileMetadata = serde_json::from_value(json).unwrap();

        assert_eq!(metadata.id, "file123");
        assert_eq!(metadata.name, "document.pdf");
        assert_eq!(metadata.mime_type, Some("application/pdf".to_string()));
        assert_eq!(metadata.size, Some(2048));
    }

    #[test]
    fn test_file_metadata_without_size() {
        let json = json!({
            "id": "folder123",
            "name": "My Folder",
            "mimeType": "application/vnd.google-apps.folder"
        });

        let metadata: FileMetadata = serde_json::from_value(json).unwrap();

        assert_eq!(metadata.id, "folder123");
        assert_eq!(metadata.name, "My Folder");
        assert_eq!(metadata.size, None);
    }

    #[test]
    fn test_file_list_response_deserialization() {
        let json = json!({
            "files": [
                {"id": "f1", "name": "file1.txt"},
                {"id": "f2", "name": "file2.txt"}
            ],
            "nextPageToken": "token123"
        });

        let response: FileListResponse = serde_json::from_value(json).unwrap();

        assert_eq!(response.files.len(), 2);
        assert_eq!(response.next_page_token, Some("token123".to_string()));
    }

    #[test]
    fn test_file_list_response_empty() {
        let json = json!({
            "files": []
        });

        let response: FileListResponse = serde_json::from_value(json).unwrap();

        assert!(response.files.is_empty());
        assert!(response.next_page_token.is_none());
    }
}

mod credentials {
    use super::*;

    #[test]
    fn test_credentials_from_json() {
        let json = json!({
            "client_email": "test@project.iam.gserviceaccount.com",
            "private_key": "key",
            "token_uri": "https://oauth2.googleapis.com/token"
        });

        let creds: ServiceAccountCredentials = serde_json::from_value(json).unwrap();

        assert_eq!(creds.client_email, "test@project.iam.gserviceaccount.com");
        assert_eq!(creds.token_uri, Some("https://oauth2.googleapis.com/token".to_string()));
    }

    #[test]
    fn test_authenticator_from_file() {
        // Create a temporary credentials file
        let mut temp_file = NamedTempFile::new().unwrap();
        let creds_json = json!({
            "client_email": "test@project.iam.gserviceaccount.com",
            "private_key": "key"
        });

        temp_file.write_all(creds_json.to_string().as_bytes()).unwrap();

        let auth = Authenticator::from_file(temp_file.path());
        assert!(auth.is_ok());
    }

    #[test]
    fn test_authenticator_from_invalid_file() {
        let auth = Authenticator::from_file("/nonexistent/path/credentials.json");
        assert!(auth.is_err());
    }

    #[test]
    fn test_authenticator_from_invalid_json() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"not valid json").unwrap();

        let auth = Authenticator::from_file(temp_file.path());
        assert!(auth.is_err());
    }
}

mod error_handling {
    use share_drive::error::DriveError;

    #[test]
    fn test_error_display() {
        let err = DriveError::ApiError {
            status: 404,
            message: "File not found".to_string(),
        };

        let display = format!("{}", err);
        assert!(display.contains("404"));
        assert!(display.contains("File not found"));
    }

    #[test]
    fn test_invalid_url_error() {
        let err = DriveError::InvalidUrlOrId("bad-url".to_string());
        let display = format!("{}", err);
        assert!(display.contains("bad-url"));
    }
}

mod file_metadata_display {
    use share_drive::models::FileMetadata;

    #[test]
    fn test_display_with_all_fields() {
        let metadata = FileMetadata {
            id: "abc123".to_string(),
            name: "document.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
            web_view_link: Some("https://example.com".to_string()),
            size: Some(1048576), // 1 MB
        };

        let display = format!("{}", metadata);
        assert!(display.contains("abc123"));
        assert!(display.contains("document.pdf"));
        assert!(display.contains("1.00 MB"));
        assert!(display.contains("application/pdf"));
    }

    #[test]
    fn test_display_folder_no_size() {
        let metadata = FileMetadata {
            id: "folder123".to_string(),
            name: "My Folder".to_string(),
            mime_type: Some("application/vnd.google-apps.folder".to_string()),
            web_view_link: None,
            size: None,
        };

        let display = format!("{}", metadata);
        assert!(display.contains("folder123"));
        assert!(display.contains("My Folder"));
        assert!(display.contains("-")); // No size
    }
}
