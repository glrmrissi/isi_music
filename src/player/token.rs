use crate::config;
use crate::config::OFFICIAL_CLIENT_ID;
use anyhow::Result;
use tracing::{debug, info, warn};

pub(super) enum StreamingRefreshError {
    Rejected(String),
    Transient(anyhow::Error),
}

pub(super) fn refresh_http_error(
    status: reqwest::StatusCode,
    body: String,
) -> StreamingRefreshError {
    let msg = format!("streaming token refresh {status}: {body}");
    if matches!(status.as_u16(), 400 | 401 | 403) {
        StreamingRefreshError::Rejected(msg)
    } else {
        StreamingRefreshError::Transient(anyhow::anyhow!(msg))
    }
}

pub async fn ensure_streaming_auth() -> Result<()> {
    if let Some(rt) = config::load_streaming_refresh_token() {
        match refresh_streaming_token(&rt).await {
            Ok(_) => return Ok(()),
            Err(StreamingRefreshError::Rejected(msg)) => {
                warn!("Streaming refresh token rejected ({msg}); re-authenticating");
                config::clear_streaming_refresh_token();
            }
            Err(StreamingRefreshError::Transient(e)) => return Err(e),
        }
    }

    info!("Launching browser for streaming OAuth...");
    let (_, refresh_token, _) =
        crate::spotify::auth::SpotifyAuth::authenticate_with_client_id(OFFICIAL_CLIENT_ID).await?;
    config::save_streaming_refresh_token(&refresh_token);
    Ok(())
}

pub(super) async fn obtain_streaming_token() -> Result<String> {
    let Some(rt) = config::load_streaming_refresh_token() else {
        anyhow::bail!("Streaming authentication is not initialized; run setup-spotify first");
    };

    match refresh_streaming_token(&rt).await {
        Ok(token) => {
            debug!("Refreshed streaming token from stored streaming refresh token");
            Ok(token)
        }
        Err(StreamingRefreshError::Rejected(msg)) => {
            config::clear_streaming_refresh_token();
            Err(anyhow::anyhow!(msg)
                .context("Streaming authentication expired; run setup-spotify again"))
        }
        Err(StreamingRefreshError::Transient(e)) => Err(e),
    }
}

async fn refresh_streaming_token(refresh_token: &str) -> Result<String, StreamingRefreshError> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(1)
        .pool_idle_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let resp = http
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OFFICIAL_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|e| StreamingRefreshError::Transient(e.into()))?;

    let status = resp.status();
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| StreamingRefreshError::Transient(e.into()))?;

    if !status.is_success() {
        let body = serde_json::to_string(&json).unwrap_or_default();
        return Err(refresh_http_error(status, body));
    }

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| {
            StreamingRefreshError::Transient(anyhow::anyhow!(
                "no access_token in streaming refresh response"
            ))
        })?
        .to_string();

    if let Some(new_rt) = json["refresh_token"].as_str() {
        config::save_streaming_refresh_token(new_rt);
    }

    Ok(access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_http_error_marks_client_rejection_as_fatal() {
        for raw in [400u16, 401, 403] {
            let status = reqwest::StatusCode::from_u16(raw).unwrap();
            assert!(matches!(
                refresh_http_error(status, String::new()),
                StreamingRefreshError::Rejected(_)
            ));
        }
    }

    #[test]
    fn refresh_http_error_keeps_transient_statuses_retryable() {
        for raw in [408u16, 429, 500, 502, 503] {
            let status = reqwest::StatusCode::from_u16(raw).unwrap();
            assert!(matches!(
                refresh_http_error(status, String::new()),
                StreamingRefreshError::Transient(_)
            ));
        }
    }
}
