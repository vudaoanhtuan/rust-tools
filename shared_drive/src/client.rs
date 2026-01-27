//! Google Drive API client for Shared Drive operations.

use std::path::Path;

use futures::StreamExt;
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::auth::Authenticator;
use crate::error::{DriveError, Result};
use crate::models::{ApiErrorResponse, FileListResponse, FileMetadata};

/// Base URL for Google Drive API v3.
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

/// Upload URL for Google Drive API.
const UPLOAD_API_BASE: &str = "https://www.googleapis.com/upload/drive/v3";

/// Threshold for resumable upload (500 MB).
const RESUMABLE_THRESHOLD: u64 = 500 * 1024 * 1024;

/// Client for interacting with Google Shared Drive.
pub struct SharedDriveClient {
    drive_id: String,
    auth: Authenticator,
    http: Client,
}

impl SharedDriveClient {
    /// Create a new SharedDriveClient.
    ///
    /// # Arguments
    /// * `auth` - Authenticator for obtaining access tokens
    /// * `drive_id` - The ID of the Shared Drive
    pub fn new(auth: Authenticator, drive_id: String) -> Self {
        Self {
            drive_id,
            auth,
            http: Client::new(),
        }
    }

    /// Get the drive ID.
    pub fn drive_id(&self) -> &str {
        &self.drive_id
    }

    /// List all files in a folder.
    ///
    /// # Arguments
    /// * `parent_id` - The ID of the parent folder
    pub async fn list_files(&self, parent_id: &str) -> Result<Vec<FileMetadata>> {
        let query = format!("'{}' in parents and trashed = false", parent_id);
        self.query_files(&query).await
    }

    /// Query files using Google Drive query syntax.
    pub async fn query_files(&self, query: &str) -> Result<Vec<FileMetadata>> {
        let token = self.auth.get_access_token().await?;
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let mut request = self
                .http
                .get(format!("{}/files", DRIVE_API_BASE))
                .bearer_auth(&token)
                .query(&[
                    ("q", query),
                    ("driveId", &self.drive_id),
                    ("corpora", "drive"),
                    ("includeItemsFromAllDrives", "true"),
                    ("supportsAllDrives", "true"),
                    ("spaces", "drive"),
                    ("fields", "nextPageToken, files(id, name, size, mimeType, webViewLink)"),
                ]);

            if let Some(ref token) = page_token {
                request = request.query(&[("pageToken", token)]);
            }

            let response = request.send().await?;
            let status = response.status();

            if !status.is_success() {
                let error_body = response.text().await.unwrap_or_default();
                if let Ok(api_error) = serde_json::from_str::<ApiErrorResponse>(&error_body) {
                    return Err(DriveError::ApiError {
                        status: api_error.error.code,
                        message: api_error.error.message,
                    });
                }
                return Err(DriveError::ApiError {
                    status: status.as_u16(),
                    message: error_body,
                });
            }

            let list_response: FileListResponse = response.json().await?;
            all_files.extend(list_response.files);

            match list_response.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }

        Ok(all_files)
    }

    /// Find a file by name in a folder.
    pub async fn find_file(&self, name: &str, parent_id: &str) -> Result<Option<FileMetadata>> {
        let query = format!(
            "name = '{}' and '{}' in parents and trashed = false",
            name.replace('\'', "\\'"),
            parent_id
        );
        let files = self.query_files(&query).await?;
        Ok(files.into_iter().last())
    }

    /// Get file metadata by ID.
    pub async fn get_file(&self, file_id: &str) -> Result<FileMetadata> {
        let token = self.auth.get_access_token().await?;

        let response = self
            .http
            .get(format!("{}/files/{}", DRIVE_API_BASE, file_id))
            .bearer_auth(&token)
            .query(&[
                ("supportsAllDrives", "true"),
                ("fields", "id, name, size, mimeType, webViewLink"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            if let Ok(api_error) = serde_json::from_str::<ApiErrorResponse>(&error_body) {
                return Err(DriveError::ApiError {
                    status: api_error.error.code,
                    message: api_error.error.message,
                });
            }
            return Err(DriveError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let metadata: FileMetadata = response.json().await?;
        Ok(metadata)
    }

    /// Delete a file by ID.
    pub async fn delete_file(&self, file_id: &str) -> Result<()> {
        let token = self.auth.get_access_token().await?;

        let response = self
            .http
            .delete(format!("{}/files/{}", DRIVE_API_BASE, file_id))
            .bearer_auth(&token)
            .query(&[("supportsAllDrives", "true")])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() && status.as_u16() != 404 {
            let error_body = response.text().await.unwrap_or_default();
            return Err(DriveError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        Ok(())
    }

    /// Upload a file to a folder.
    ///
    /// If a file with the same name exists, it will be overwritten.
    ///
    /// # Arguments
    /// * `local_path` - Path to the local file
    /// * `parent_id` - ID of the destination folder
    pub async fn upload_file<P: AsRef<Path>>(
        &self,
        local_path: P,
        parent_id: &str,
    ) -> Result<FileMetadata> {
        let local_path = local_path.as_ref();
        let filename = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DriveError::FileNotFound(local_path.display().to_string()))?;

        // Check if file exists and delete it (overwrite behavior)
        if let Some(existing) = self.find_file(filename, parent_id).await? {
            self.delete_file(&existing.id).await?;
        }

        let file_size = std::fs::metadata(local_path)?.len();
        let mime_type = mime_guess::from_path(local_path)
            .first_or_octet_stream()
            .to_string();

        if file_size > RESUMABLE_THRESHOLD {
            self.upload_resumable(local_path, parent_id, filename, &mime_type)
                .await
        } else {
            self.upload_multipart(local_path, parent_id, filename, &mime_type)
                .await
        }
    }

    /// Upload a file using multipart upload (for smaller files).
    async fn upload_multipart(
        &self,
        local_path: &Path,
        parent_id: &str,
        filename: &str,
        mime_type: &str,
    ) -> Result<FileMetadata> {
        let token = self.auth.get_access_token().await?;
        let file_content = std::fs::read(local_path)?;

        let metadata = serde_json::json!({
            "name": filename,
            "driveId": self.drive_id,
            "parents": [parent_id]
        });

        let metadata_part = Part::text(metadata.to_string())
            .mime_str("application/json")?;

        let file_part = Part::bytes(file_content)
            .file_name(filename.to_string())
            .mime_str(mime_type)?;

        let form = Form::new()
            .part("metadata", metadata_part)
            .part("file", file_part);

        let response = self
            .http
            .post(format!("{}/files", UPLOAD_API_BASE))
            .bearer_auth(&token)
            .query(&[
                ("uploadType", "multipart"),
                ("supportsAllDrives", "true"),
                ("fields", "id, name, size, mimeType, webViewLink"),
            ])
            .multipart(form)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            if let Ok(api_error) = serde_json::from_str::<ApiErrorResponse>(&error_body) {
                return Err(DriveError::ApiError {
                    status: api_error.error.code,
                    message: api_error.error.message,
                });
            }
            return Err(DriveError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let metadata: FileMetadata = response.json().await?;
        Ok(metadata)
    }

    /// Upload a file using resumable upload (for larger files).
    async fn upload_resumable(
        &self,
        local_path: &Path,
        parent_id: &str,
        filename: &str,
        mime_type: &str,
    ) -> Result<FileMetadata> {
        let token = self.auth.get_access_token().await?;
        let file_content = std::fs::read(local_path)?;
        let file_size = file_content.len();

        let metadata = serde_json::json!({
            "name": filename,
            "driveId": self.drive_id,
            "parents": [parent_id]
        });

        // Step 1: Initiate resumable upload
        let init_response = self
            .http
            .post(format!("{}/files", UPLOAD_API_BASE))
            .bearer_auth(&token)
            .query(&[
                ("uploadType", "resumable"),
                ("supportsAllDrives", "true"),
            ])
            .header("Content-Type", "application/json")
            .header("X-Upload-Content-Type", mime_type)
            .header("X-Upload-Content-Length", file_size.to_string())
            .json(&metadata)
            .send()
            .await?;

        let status = init_response.status();
        if !status.is_success() {
            let error_body = init_response.text().await.unwrap_or_default();
            return Err(DriveError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let upload_url = init_response
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                DriveError::ApiError {
                    status: 500,
                    message: "No upload URL in response".to_string(),
                }
            })?
            .to_string();

        // Step 2: Upload the file content
        let upload_response = self
            .http
            .put(&upload_url)
            .header("Content-Type", mime_type)
            .header("Content-Length", file_size.to_string())
            .query(&[("fields", "id, name, size, mimeType, webViewLink")])
            .body(file_content)
            .send()
            .await?;

        let status = upload_response.status();
        if !status.is_success() {
            let error_body = upload_response.text().await.unwrap_or_default();
            return Err(DriveError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        let metadata: FileMetadata = upload_response.json().await?;
        Ok(metadata)
    }

    /// Download a file to a local path.
    ///
    /// # Arguments
    /// * `file_id` - The ID of the file to download
    /// * `destination` - The local path to save the file
    pub async fn download_file<P: AsRef<Path>>(
        &self,
        file_id: &str,
        destination: P,
    ) -> Result<FileMetadata> {
        let token = self.auth.get_access_token().await?;
        let destination = destination.as_ref();

        // Get file metadata first
        let metadata = self.get_file(file_id).await?;

        // Determine the final path
        let final_path = if destination.is_dir() {
            destination.join(&metadata.name)
        } else {
            destination.to_path_buf()
        };

        // Download the file
        let response = self
            .http
            .get(format!("{}/files/{}", DRIVE_API_BASE, file_id))
            .bearer_auth(&token)
            .query(&[("alt", "media"), ("supportsAllDrives", "true")])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(DriveError::ApiError {
                status: status.as_u16(),
                message: error_body,
            });
        }

        // Stream to file
        let mut file = File::create(&final_path).await?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }

        file.flush().await?;

        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    // Tests are in shared_drive/tests/client_test.rs
}
