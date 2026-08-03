//! Cross-platform IPC between the CLI and the daemon.
//!
//! Unix: Unix domain socket at `$XDG_RUNTIME_DIR/isi-music.sock`.
//! Windows: named pipe at `\\.\pipe\isi-music` (no firewall prompt, auto-cleaned).

use anyhow::Result;

#[cfg(unix)]
pub use unix::IpcListener;

#[cfg(windows)]
pub use windows::IpcListener;

#[cfg(unix)]
pub fn socket_path() -> std::path::PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("isi-music.sock")
}

#[cfg(windows)]
pub const PIPE_NAME: &str = r"\\.\pipe\isi-music";

/// Send a command to a running daemon and return its response.
pub async fn send_command(cmd: &str) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = connect().await?;
    stream.write_all(format!("{cmd}\n").as_bytes()).await?;
    stream.shutdown().await?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).await?;
    Ok(buf.trim().to_string())
}

#[cfg(unix)]
async fn connect() -> Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(socket_path())
        .await
        .map_err(|_| anyhow::anyhow!("Daemon not running — start with: isi-music --daemon"))
}

#[cfg(windows)]
async fn connect() -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;

    const ERROR_PIPE_BUSY: i32 = 109;

    for attempt in 0..20 {
        match ClientOptions::new().open(PIPE_NAME) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempt < 19 => {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Daemon not running — start with: isi-music --daemon"
                ));
            }
        }
    }
    unreachable!()
}

#[cfg(unix)]
mod unix {
    use std::io;
    use tokio::net::{UnixListener as TokioListener, UnixStream};

    pub struct IpcListener {
        inner: TokioListener,
    }

    impl IpcListener {
        pub fn bind() -> io::Result<Self> {
            let path = super::socket_path();
            if path.exists() {
                std::fs::remove_file(&path).ok();
            }
            Ok(Self {
                inner: TokioListener::bind(&path)?,
            })
        }

        pub async fn accept(&mut self) -> io::Result<UnixStream> {
            let (stream, _) = self.inner.accept().await?;
            Ok(stream)
        }

        /// Remove the socket file on shutdown (Unix leaves it behind).
        pub fn cleanup() {
            std::fs::remove_file(super::socket_path()).ok();
        }

        pub fn describe(&self) -> String {
            super::socket_path().display().to_string()
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    /// Each client connection gets its own pipe server instance:
    /// `accept` waits for a client on the current instance, then immediately
    /// creates the next one so further CLI commands can connect.
    pub struct IpcListener {
        server: NamedPipeServer,
    }

    impl IpcListener {
        pub fn bind() -> io::Result<Self> {
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(super::PIPE_NAME)?;
            Ok(Self { server })
        }

        pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
            self.server.connect().await?;
            let connected = std::mem::replace(
                &mut self.server,
                ServerOptions::new().create(super::PIPE_NAME)?,
            );
            Ok(connected)
        }

        /// Named pipes vanish once all handles close — nothing to clean up.
        pub fn cleanup() {}

        pub fn describe(&self) -> String {
            super::PIPE_NAME.to_string()
        }
    }
}
