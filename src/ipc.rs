use anyhow::Result;
use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isi-music.sock")
}

/// Send a command to a running daemon and return its response.
pub async fn send_command(cmd: &str) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let path = socket_path();
    let mut stream = UnixStream::connect(&path).await
        .map_err(|_| anyhow::anyhow!(
            "Daemon not running — start with: isi-music --daemon"
        ))?;

    stream.write_all(format!("{cmd}\n").as_bytes()).await?;
    stream.shutdown().await?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).await?;
    Ok(buf.trim().to_string())
}
