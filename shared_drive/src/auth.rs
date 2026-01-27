//! Service account authentication for Google APIs.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Client;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::error::{DriveError, Result};
use crate::models::{ServiceAccountCredentials, TokenResponse};

/// Google OAuth2 token endpoint.
const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// Google Drive API scope.
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive";

/// JWT claims for service account authentication.
#[derive(Debug, Serialize)]
struct Claims {
    iss: String,   // Issuer (service account email)
    scope: String, // OAuth scope
    aud: String,   // Audience (token endpoint)
    exp: u64,      // Expiration time
    iat: u64,      // Issued at
}

/// Cached access token with expiration.
#[derive(Clone)]
struct CachedToken {
    access_token: String,
    expires_at: SystemTime,
}

/// Authenticator for Google APIs using service account credentials.
#[derive(Clone)]
pub struct Authenticator {
    credentials: Arc<ServiceAccountCredentials>,
    client: Client,
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

impl Authenticator {
    /// Create a new authenticator from a service account JSON file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let credentials: ServiceAccountCredentials = serde_json::from_str(&content)?;
        Ok(Self::new(credentials))
    }

    /// Create a new authenticator from credentials.
    pub fn new(credentials: ServiceAccountCredentials) -> Self {
        Self {
            credentials: Arc::new(credentials),
            client: Client::new(),
            cached_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Get a valid access token, refreshing if necessary.
    pub async fn get_access_token(&self) -> Result<String> {
        // Check if we have a valid cached token
        {
            let cached = self.cached_token.read().await;
            if let Some(token) = cached.as_ref() {
                // Add 60 second buffer before expiration
                let buffer = Duration::from_secs(60);
                if token.expires_at > SystemTime::now() + buffer {
                    return Ok(token.access_token.clone());
                }
            }
        }

        // Refresh the token
        let new_token = self.refresh_token().await?;

        // Cache the new token
        {
            let mut cached = self.cached_token.write().await;
            *cached = Some(new_token.clone());
        }

        Ok(new_token.access_token)
    }

    /// Refresh the access token using JWT assertion.
    async fn refresh_token(&self) -> Result<CachedToken> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        let claims = Claims {
            iss: self.credentials.client_email.clone(),
            scope: DRIVE_SCOPE.to_string(),
            aud: TOKEN_URI.to_string(),
            iat: now,
            exp: now + 3600, // 1 hour
        };

        // Create JWT
        let header = Header::new(Algorithm::RS256);
        let key = EncodingKey::from_rsa_pem(self.credentials.private_key.as_bytes())?;
        let jwt = encode(&header, &claims, &key)?;

        // Exchange JWT for access token
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ];

        let response = self
            .client
            .post(TOKEN_URI)
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(DriveError::TokenRefreshError(format!(
                "Status {}: {}",
                status, body
            )));
        }

        let token_response: TokenResponse = response.json().await?;

        let expires_at =
            SystemTime::now() + Duration::from_secs(token_response.expires_in);

        Ok(CachedToken {
            access_token: token_response.access_token,
            expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claims_serialization() {
        let claims = Claims {
            iss: "test@example.iam.gserviceaccount.com".to_string(),
            scope: DRIVE_SCOPE.to_string(),
            aud: TOKEN_URI.to_string(),
            iat: 1234567890,
            exp: 1234571490,
        };

        let json = serde_json::to_string(&claims).unwrap();
        assert!(json.contains("test@example.iam.gserviceaccount.com"));
        assert!(json.contains(DRIVE_SCOPE));
    }
}
