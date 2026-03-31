use anyhow::{Context, Result, bail};
use rspotify::{AuthCodeSpotify, Config, Credentials, OAuth, scopes};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::config::{self, AppConfig};

const CALLBACK_PORT: u16 = 8888;

pub struct SpotifyAuth;

impl SpotifyAuth {
    pub fn build_client() -> Result<AuthCodeSpotify> {
        let cfg = AppConfig::load()?;

        let client_id = cfg
            .get_client_id()
            .context("SPOTIFY_CLIENT_ID not found.\nSet it in ~/.config/isi-music/config.toml or as an environment variable.")?;
        let client_secret = cfg
            .get_client_secret()
            .context("SPOTIFY_CLIENT_SECRET not found.\nSet it in ~/.config/isi-music/config.toml or as an environment variable.")?;

        let cache_path = config::cache_path()?;

        let creds = Credentials::new(&client_id, &client_secret);
        let oauth = OAuth {
            redirect_uri: format!("http://127.0.0.1:{CALLBACK_PORT}/callback"),
            scopes: scopes!(
                "streaming",
                "user-read-private",
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing",
                "user-library-read",
                "user-library-modify",
                "playlist-read-private",
                "playlist-modify-private",
                "playlist-modify-public"
            ),
            ..Default::default()
        };
        let rspotify_config = Config {
            token_cached: true,
            token_refreshing: true,
            cache_path,
            ..Default::default()
        };

        Ok(AuthCodeSpotify::with_config(creds, oauth, rspotify_config))
    }

    /// Opens the browser and waits for the OAuth callback via local server.
    /// Returns the `code` extracted from the redirect URL.
    pub async fn run_oauth_flow(authorize_url: &str) -> Result<String> {
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

        Self::extract_code_from_request(&request)
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
