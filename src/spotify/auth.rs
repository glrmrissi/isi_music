use anyhow::{Context, Result, bail};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::Write as _;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::config;

const SCOPES: &[&str] = &[
    "streaming",
    "user-read-private",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-library-read",
    "user-library-modify",
    "user-follow-read",
    "playlist-read-private",
    "playlist-read-collaborative",
    "playlist-modify-private",
    "playlist-modify-public",
];

fn base64url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn generate_code_verifier() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
    base64url_encode(&bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

fn build_authorize_url(client_id: &str, challenge: &str, redirect_uri: &str) -> String {
    let encoded_scopes = SCOPES.join("%20");
    let encoded_redirect = urlencoding::encode(redirect_uri);
    format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&\
         redirect_uri={}&code_challenge_method=S256&code_challenge={}&scope={}",
        client_id, encoded_redirect, challenge, encoded_scopes
    )
}

async fn exchange_code(
    code: &str,
    verifier: &str,
    client_id: &str,
    redirect_uri: &str,
) -> Result<(String, String, u64)> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let resp = http
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", verifier),
        ])
        .send()
        .await?;

    let status = resp.status();
    let json: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        let body = serde_json::to_string(&json).unwrap_or_default();
        bail!("token endpoint {status}: {body}");
    }

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no access_token in response"))?
        .to_string();
    let refresh_token = json["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no refresh_token in response"))?
        .to_string();
    let expires_in = json["expires_in"].as_u64().unwrap_or(3600);

    Ok((access_token, refresh_token, expires_in))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "linux")]
    {
        let is_wsl = std::fs::read_to_string("/proc/version")
            .map(|v| v.to_lowercase().contains("microsoft"))
            .unwrap_or(false);
        if is_wsl {
            let quoted_url = format!("\"{url}\"");
            let _ = std::process::Command::new("cmd.exe")
                .args(["/c", "start", "", &quoted_url])
                .spawn();
        } else {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
    }

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("powershell")
        .args(["-Command", &format!("Start-Process '{}'", url)])
        .spawn();
}

fn extract_code_from_request(request: &str) -> Result<String> {
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .context("Invalid HTTP request")?;

    let fake_url = format!("http://127.0.0.1{path}");
    let parsed = url::Url::parse(&fake_url).context("Invalid callback URL")?;

    for (key, val) in parsed.query_pairs() {
        if key == "code" {
            return Ok(val.into_owned());
        }
    }

    bail!("'code' parameter not found in callback")
}

async fn run_oauth_flow(authorize_url: &str, callback_port: u16) -> Result<String> {
    #[cfg(target_os = "linux")]
    let bind_addr: &str = {
        let is_wsl = std::fs::read_to_string("/proc/version")
            .map(|v| v.to_lowercase().contains("microsoft"))
            .unwrap_or(false);
        if is_wsl { "0.0.0.0" } else { "127.0.0.1" }
    };
    #[cfg(not(target_os = "linux"))]
    let bind_addr: &str = "127.0.0.1";

    let listener = TcpListener::bind((bind_addr, callback_port))
        .await
        .map_err(|e| {
            let pid_info = find_port_owner(callback_port);
            let hint = if let Some((pid, name)) = pid_info {
                format!(
                    "Port {callback_port} is in use by PID {pid} ({name}). \
                     Close that process and retry."
                )
            } else {
                format!(
                    "Port {callback_port} is in use. \
                     Close the process using it and retry."
                )
            };
            anyhow::anyhow!("{hint} (bind error: {e})")
        })?;

    open_browser(authorize_url);

    println!("Waiting for authorization in browser... (port {callback_port})");
    println!("If the browser doesn't open, visit:");
    println!("  {authorize_url}");

    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        let (mut stream, _) = tokio::select! {
            _ = &mut ctrl_c => bail!("Authentication cancelled by user"),
            result = listener.accept() => result?,
        };

        let mut buf = vec![0u8; 4096];
        let n = tokio::select! {
            _ = &mut ctrl_c => bail!("Authentication cancelled by user"),
            result = stream.read(&mut buf) => result?,
        };
        let request = String::from_utf8_lossy(&buf[..n]);

        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body><h2>isi-music authorized!</h2>\
            <p>You can close this tab.</p>\
            <script>window.close();</script></body></html>";
        tokio::select! {
            _ = &mut ctrl_c => bail!("Authentication cancelled by user"),
            result = stream.write_all(response.as_bytes()) => result?,
        }

        match extract_code_from_request(&request) {
            Ok(code) => return Ok(code),
            Err(_) => continue,
        }
    }
}

/// Try to find which process owns a given TCP port.
/// Returns (pid, process_name) if found.
fn find_port_owner(port: u16) -> Option<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        // Try ss first (faster), fall back to lsof
        if let Ok(out) = std::process::Command::new("ss")
            .args(["-tlnp", "-H", &format!("sport = :{port}")])
            .output()
        {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                // ss output: "LISTEN 0 4096 0.0.0.0:8888 0.0.0.0:* users:(("process",pid=123,fd=4))"
                if let Some(pid_start) = line.find("pid=") {
                    let rest = &line[pid_start + 4..];
                    let pid_end = rest
                        .find(|c: char| !c.is_ascii_digit())
                        .unwrap_or(rest.len());
                    let pid = &rest[..pid_end];
                    let name = line
                        .find("users:((\"")
                        .and_then(|i| {
                            let after = &line[i + 9..];
                            after.find('"').map(|e| &after[..e])
                        })
                        .unwrap_or("unknown");
                    return Some((pid.to_string(), name.to_string()));
                }
            }
        }
        if let Ok(out) = std::process::Command::new("lsof")
            .args(["-i", &format!(":{port}"), "-t"])
            .output()
        {
            let pid = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !pid.is_empty() {
                return Some((pid, "unknown".to_string()));
            }
        }
        None
    }

    #[cfg(windows)]
    {
        if let Ok(out) = std::process::Command::new("netstat")
            .args(["-ano"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.contains(&format!(":{port}")) && line.contains("LISTENING") {
                    let pid = line.split_whitespace().last().unwrap_or("?");
                    return Some((pid.to_string(), "unknown".to_string()));
                }
            }
        }
        None
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = port;
        None
    }
}

pub struct SpotifyAuth;

impl SpotifyAuth {
    pub async fn authenticate() -> Result<(String, String, u64)> {
        let cfg = config::AppConfig::load()?;
        let client_id = cfg.get_client_id().ok_or_else(|| {
            anyhow::anyhow!("Spotify Web API Client ID is not configured; run setup-spotify first")
        })?;
        Self::authenticate_with_client_id(&client_id).await
    }

    pub async fn authenticate_with_client_id(client_id: &str) -> Result<(String, String, u64)> {
        let (redirect_uri, callback_port) = if client_id == config::OFFICIAL_CLIENT_ID {
            (config::OFFICIAL_REDIRECT_URI, 8898)
        } else {
            (config::CUSTOM_REDIRECT_URI, 8888)
        };

        loop {
            let verifier = generate_code_verifier();
            let challenge = generate_code_challenge(&verifier);

            let url = build_authorize_url(client_id, &challenge, redirect_uri);
            match run_oauth_flow(&url, callback_port).await {
                Ok(code) => {
                    return exchange_code(&code, &verifier, client_id, redirect_uri).await;
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    if msg.contains("Port") && msg.contains("in use") {
                        println!();
                        println!("  Error: {msg}");
                        print!("  Retry? (Y/n): ");
                        std::io::stdout().flush().ok();
                        let mut input = String::new();
                        std::io::stdin().read_line(&mut input).ok();
                        if input.trim().eq_ignore_ascii_case("n") {
                            return Err(anyhow::anyhow!("Authentication cancelled by user"));
                        }
                        println!("  Retrying...");
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }
}
