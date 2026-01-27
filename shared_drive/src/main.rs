//! share_drive CLI - Interact with Google Shared Drive.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use glob::glob;

use share_drive::{extract_id, Authenticator, SharedDriveClient};

/// CLI tool for interacting with Google Shared Drive.
#[derive(Parser)]
#[command(name = "share_drive")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to service account JSON credentials file.
    #[arg(long, env = "GOOGLE_APPLICATION_CREDENTIALS")]
    credentials: PathBuf,

    /// Shared Drive ID (can also be set via SHARED_DRIVE_ID env var).
    #[arg(long, env = "SHARED_DRIVE_ID")]
    drive_id: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List files in a folder.
    List {
        /// Folder URL or ID.
        folder: String,
    },

    /// Upload files to a folder.
    Upload {
        /// File patterns to upload (supports glob patterns like *.tar, file_{1,2,3}.txt).
        #[arg(required = true)]
        patterns: Vec<String>,

        /// Destination folder URL or ID.
        #[arg(long, short = 't')]
        to: String,
    },

    /// Download a file to local filesystem.
    Download {
        /// File URL or ID to download.
        file: String,

        /// Local destination path (file or directory).
        #[arg(long, short = 't', default_value = ".")]
        to: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize authenticator
    let auth = Authenticator::from_file(&cli.credentials)
        .with_context(|| format!("Failed to load credentials from {:?}", cli.credentials))?;

    // Create client
    let client = SharedDriveClient::new(auth, cli.drive_id);

    match cli.command {
        Commands::List { folder } => {
            let folder_id = extract_id(&folder)
                .with_context(|| format!("Invalid folder URL or ID: {}", folder))?;

            let files = client
                .list_files(&folder_id)
                .await
                .with_context(|| format!("Failed to list files in folder: {}", folder_id))?;

            if files.is_empty() {
                println!("No files found.");
            } else {
                println!("{:<44} {:>10} {:<30} {}", "ID", "SIZE", "TYPE", "NAME");
                println!("{}", "-".repeat(100));
                for file in files {
                    println!("{}", file);
                }
            }
        }

        Commands::Upload { patterns, to } => {
            let folder_id = extract_id(&to)
                .with_context(|| format!("Invalid folder URL or ID: {}", to))?;

            // Expand glob patterns
            let mut files_to_upload: Vec<PathBuf> = Vec::new();

            for pattern in &patterns {
                // Handle brace expansion manually for patterns like file_{1,2,3}.txt
                let expanded_patterns = expand_braces(pattern);

                for expanded_pattern in expanded_patterns {
                    let matches: Vec<PathBuf> = glob(&expanded_pattern)
                        .with_context(|| format!("Invalid glob pattern: {}", expanded_pattern))?
                        .filter_map(|r| r.ok())
                        .filter(|p| p.is_file())
                        .collect();

                    if matches.is_empty() {
                        // If no glob matches, treat as literal path
                        let path = PathBuf::from(&expanded_pattern);
                        if path.is_file() {
                            files_to_upload.push(path);
                        } else {
                            eprintln!("Warning: No files matched pattern: {}", expanded_pattern);
                        }
                    } else {
                        files_to_upload.extend(matches);
                    }
                }
            }

            // Remove duplicates
            files_to_upload.sort();
            files_to_upload.dedup();

            if files_to_upload.is_empty() {
                anyhow::bail!("No files to upload");
            }

            println!("Uploading {} file(s) to {}...", files_to_upload.len(), folder_id);

            for (idx, file_path) in files_to_upload.iter().enumerate() {
                let filename = file_path.file_name().unwrap_or_default().to_string_lossy();
                print!("[{}/{}] Uploading {}... ", idx + 1, files_to_upload.len(), filename);

                match client.upload_file(file_path, &folder_id).await {
                    Ok(metadata) => {
                        println!("OK ({})", metadata.id);
                    }
                    Err(e) => {
                        println!("FAILED");
                        eprintln!("  Error: {}", e);
                    }
                }
            }

            println!("Done.");
        }

        Commands::Download { file, to } => {
            let file_id = extract_id(&file)
                .with_context(|| format!("Invalid file URL or ID: {}", file))?;

            // Ensure destination directory exists
            if to.is_dir() || to.to_string_lossy().ends_with('/') {
                std::fs::create_dir_all(&to)
                    .with_context(|| format!("Failed to create directory: {:?}", to))?;
            } else if let Some(parent) = to.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("Failed to create directory: {:?}", parent))?;
                }
            }

            print!("Downloading {}... ", file_id);

            let metadata = client
                .download_file(&file_id, &to)
                .await
                .with_context(|| format!("Failed to download file: {}", file_id))?;

            let final_path = if to.is_dir() {
                to.join(&metadata.name)
            } else {
                to
            };

            println!("OK");
            println!("Saved to: {:?}", final_path);
        }
    }

    Ok(())
}

/// Expand brace patterns like file_{1,2,3}.txt into multiple patterns.
fn expand_braces(pattern: &str) -> Vec<String> {
    // Find brace expression
    if let Some(start) = pattern.find('{') {
        if let Some(end) = pattern[start..].find('}') {
            let end = start + end;
            let prefix = &pattern[..start];
            let suffix = &pattern[end + 1..];
            let alternatives = &pattern[start + 1..end];

            return alternatives
                .split(',')
                .flat_map(|alt| {
                    let expanded = format!("{}{}{}", prefix, alt.trim(), suffix);
                    expand_braces(&expanded)
                })
                .collect();
        }
    }

    vec![pattern.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_braces_simple() {
        let result = expand_braces("file_{1,2,3}.txt");
        assert_eq!(result, vec!["file_1.txt", "file_2.txt", "file_3.txt"]);
    }

    #[test]
    fn test_expand_braces_no_braces() {
        let result = expand_braces("file.txt");
        assert_eq!(result, vec!["file.txt"]);
    }

    #[test]
    fn test_expand_braces_glob_pattern() {
        let result = expand_braces("*.tar");
        assert_eq!(result, vec!["*.tar"]);
    }

    #[test]
    fn test_expand_braces_nested() {
        let result = expand_braces("{a,b}_{1,2}.txt");
        assert_eq!(
            result,
            vec!["a_1.txt", "a_2.txt", "b_1.txt", "b_2.txt"]
        );
    }
}
