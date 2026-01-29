//! Google Drive API client for Shared Drive operations.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::auth::Authenticator;
use crate::error::{DriveError, Result};
use crate::models::{ApiErrorResponse, FileListResponse, FileMetadata};

/// Base URL for Google Drive API v3.
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

/// Upload URL for Google Drive API.
const UPLOAD_API_BASE: &str = "https://www.googleapis.com/upload/drive/v3";

/// Threshold for resumable upload (50 MB).
/// Files larger than this use chunked resumable upload with progress reporting.
const RESUMABLE_THRESHOLD: u64 = 50 * 1024 * 1024;

/// Chunk size for resumable uploads (8 MB).
/// Google recommends multiples of 256 KB; larger chunks are more efficient.
const CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Progress information for file transfers (uploads and downloads).
#[derive(Debug, Clone)]
pub struct TransferProgress {
    /// Number of bytes transferred so far.
    pub bytes_transferred: u64,
    /// Total file size in bytes.
    pub total_bytes: u64,
    /// Current transfer speed in bytes per second.
    pub bytes_per_second: f64,
}

impl TransferProgress {
    /// Calculate the remaining time in seconds based on current speed.
    /// Returns None if speed is zero or calculation is not possible.
    pub fn eta_seconds(&self) -> Option<f64> {
        if self.bytes_per_second <= 0.0 {
            return None;
        }
        let remaining = self.total_bytes.saturating_sub(self.bytes_transferred);
        Some(remaining as f64 / self.bytes_per_second)
    }

    /// Calculate the progress percentage (0.0 - 100.0).
    pub fn percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        (self.bytes_transferred as f64 / self.total_bytes as f64) * 100.0
    }
}

/// Type alias for backward compatibility.
pub type UploadProgress = TransferProgress;

/// Callback type for transfer progress notifications.
pub type ProgressCallback = Arc<dyn Fn(TransferProgress) + Send + Sync>;

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
        self.upload_file_with_progress(local_path, parent_id, None).await
    }

    /// Upload a file to a folder with progress reporting.
    ///
    /// If a file with the same name exists, it will be overwritten.
    ///
    /// # Arguments
    /// * `local_path` - Path to the local file
    /// * `parent_id` - ID of the destination folder
    /// * `progress` - Optional callback for progress updates
    pub async fn upload_file_with_progress<P: AsRef<Path>>(
        &self,
        local_path: P,
        parent_id: &str,
        progress: Option<ProgressCallback>,
    ) -> Result<FileMetadata> {
        let local_path = local_path.as_ref();
        let path_str = local_path.display().to_string();
        let filename = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| DriveError::FileNotFound(path_str.clone()))?;

        // Check if file exists and delete it (overwrite behavior)
        if let Some(existing) = self.find_file(filename, parent_id).await? {
            self.delete_file(&existing.id).await?;
        }

        let file_size = std::fs::metadata(local_path)
            .map_err(|e| DriveError::FileReadError {
                path: path_str.clone(),
                source: e,
            })?
            .len();

        let mime_type = mime_guess::from_path(local_path)
            .first_or_octet_stream()
            .to_string();

        if file_size > RESUMABLE_THRESHOLD {
            self.upload_resumable(local_path, parent_id, filename, &mime_type, file_size, progress)
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
        let path_str = local_path.display().to_string();

        // Open file and create a stream instead of reading entire file into memory
        let file = File::open(local_path).await.map_err(|e| DriveError::FileReadError {
            path: path_str.clone(),
            source: e,
        })?;

        let stream = ReaderStream::new(file);
        let body = reqwest::Body::wrap_stream(stream);

        let metadata = serde_json::json!({
            "name": filename,
            "driveId": self.drive_id,
            "parents": [parent_id]
        });

        let metadata_part = Part::text(metadata.to_string())
            .mime_str("application/json")?;

        let file_part = Part::stream(body)
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
    /// Uploads in 8 MB chunks with progress reporting.
    async fn upload_resumable(
        &self,
        local_path: &Path,
        parent_id: &str,
        filename: &str,
        mime_type: &str,
        file_size: u64,
        progress: Option<ProgressCallback>,
    ) -> Result<FileMetadata> {
        let token = self.auth.get_access_token().await?;
        let path_str = local_path.display().to_string();

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

        // Step 2: Upload file in chunks with progress tracking
        let mut file = File::open(local_path).await.map_err(|e| DriveError::FileReadError {
            path: path_str.clone(),
            source: e,
        })?;

        let mut bytes_uploaded: u64 = 0;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let start_time = Instant::now();

        loop {
            // Read a chunk from the file
            let bytes_read = file.read(&mut buffer).await.map_err(|e| DriveError::FileReadError {
                path: path_str.clone(),
                source: e,
            })?;

            if bytes_read == 0 {
                break;
            }

            let chunk_data = &buffer[..bytes_read];
            let chunk_end = bytes_uploaded + bytes_read as u64 - 1;
            let content_range = format!("bytes {}-{}/{}", bytes_uploaded, chunk_end, file_size);

            // Upload this chunk
            let chunk_response = self
                .http
                .put(&upload_url)
                .header("Content-Type", mime_type)
                .header("Content-Length", bytes_read.to_string())
                .header("Content-Range", &content_range)
                .body(chunk_data.to_vec())
                .send()
                .await?;

            let chunk_status = chunk_response.status();

            // 308 Resume Incomplete means chunk was received, continue with next
            // 200 or 201 means upload is complete
            if chunk_status.as_u16() == 308 {
                bytes_uploaded += bytes_read as u64;

                // Report progress
                if let Some(ref callback) = progress {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed > 0.0 {
                        bytes_uploaded as f64 / elapsed
                    } else {
                        0.0
                    };

                    callback(TransferProgress {
                        bytes_transferred: bytes_uploaded,
                        total_bytes: file_size,
                        bytes_per_second: speed,
                    });
                }
            } else if chunk_status.is_success() {
                // Upload complete - report 100% progress
                if let Some(ref callback) = progress {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed = if elapsed > 0.0 {
                        file_size as f64 / elapsed
                    } else {
                        0.0
                    };

                    callback(TransferProgress {
                        bytes_transferred: file_size,
                        total_bytes: file_size,
                        bytes_per_second: speed,
                    });
                }

                let result_metadata: FileMetadata = chunk_response.json().await?;
                return Ok(result_metadata);
            } else {
                let error_body = chunk_response.text().await.unwrap_or_default();
                return Err(DriveError::ApiError {
                    status: chunk_status.as_u16(),
                    message: error_body,
                });
            }
        }

        // If we reach here, something went wrong - the last chunk should have returned 200/201
        Err(DriveError::ApiError {
            status: 500,
            message: "Upload completed but no final response received".to_string(),
        })
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
        self.download_file_with_progress(file_id, destination, None).await
    }

    /// Download a file to a local path with progress reporting.
    ///
    /// # Arguments
    /// * `file_id` - The ID of the file to download
    /// * `destination` - The local path to save the file
    /// * `progress` - Optional callback for progress updates
    pub async fn download_file_with_progress<P: AsRef<Path>>(
        &self,
        file_id: &str,
        destination: P,
        progress: Option<ProgressCallback>,
    ) -> Result<FileMetadata> {
        let token = self.auth.get_access_token().await?;
        let destination = destination.as_ref();

        // Get file metadata first
        let metadata = self.get_file(file_id).await?;
        let total_bytes = metadata.size.unwrap_or(0);

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

        // Stream to file with progress tracking
        let path_str = final_path.display().to_string();
        let mut file = File::create(&final_path).await.map_err(|e| DriveError::FileWriteError {
            path: path_str.clone(),
            source: e,
        })?;
        let mut stream = response.bytes_stream();
        let mut bytes_downloaded: u64 = 0;
        let start_time = Instant::now();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let chunk_len = chunk.len() as u64;
            file.write_all(&chunk).await.map_err(|e| DriveError::FileWriteError {
                path: path_str.clone(),
                source: e,
            })?;

            bytes_downloaded += chunk_len;

            // Report progress
            if let Some(ref callback) = progress {
                let elapsed = start_time.elapsed().as_secs_f64();
                let speed = if elapsed > 0.0 {
                    bytes_downloaded as f64 / elapsed
                } else {
                    0.0
                };

                callback(TransferProgress {
                    bytes_transferred: bytes_downloaded,
                    total_bytes,
                    bytes_per_second: speed,
                });
            }
        }

        file.flush().await.map_err(|e| DriveError::FileWriteError {
            path: path_str,
            source: e,
        })?;

        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    // Tests are in shared_drive/tests/client_test.rs
}
