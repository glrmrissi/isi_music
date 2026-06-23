use anyhow::{Context, Result, bail};
use base64::Engine;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::config;

const CALLBACK_PORT: u16 = 8888;
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
    "playlist-modify-private",
    "playlist-modify-public",
];

fn base64url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn generate_code_verifier() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::random()).collect();
    base64url_encode(&bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

fn build_authorize_url(client_id: &str, challenge: &str) -> String {
    let encoded_scopes = SCOPES.join("%20");
    format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&\
         redirect_uri=http%3A%2F%2F127.0.0.1%3A{CALLBACK_PORT}%2Fcallback&\
         code_challenge_method=S256&code_challenge={}&scope={}",
        client_id, challenge, encoded_scopes
    )
}

async fn exchange_code(
    code: &str,
    verifier: &str,
    client_id: &str,
) -> Result<(String, String, u64)> {
    let http = reqwest::Client::new();
    let resp = http
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            (
                "redirect_uri",
                &format!("http://127.0.0.1:{CALLBACK_PORT}/callback"),
            ),
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
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", url])
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

async fn run_oauth_flow(authorize_url: &str) -> Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .await
        .with_context(|| format!("Port {CALLBACK_PORT} is already in use"))?;

    open_browser(authorize_url);

    println!("Waiting for authorization in browser... (port {CALLBACK_PORT})");

    let (mut stream, _) = listener.accept().await?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h2>isi-music authorized!</h2>\
        <p>You can close this tab.</p>\
        <script>window.close();</script></body></html>";
    stream.write_all(response.as_bytes()).await?;

    extract_code_from_request(&request)
}

pub struct SpotifyAuth;

impl SpotifyAuth {
    pub async fn authenticate() -> Result<(String, String, u64)> {
        let cfg = config::AppConfig::load()?;

        let client_id = cfg.get_client_id().context(
            "SPOTIFY_CLIENT_ID not found.\nSet it in ~/.config/isi-music/config.toml \
             or as an environment variable.",
        )?;

        let verifier = generate_code_verifier();
        let challenge = generate_code_challenge(&verifier);

        let url = build_authorize_url(&client_id, &challenge);
        let code = run_oauth_flow(&url).await?;
        exchange_code(&code, &verifier, &client_id).await
    }
}
