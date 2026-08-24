use console::style;
use dialoguer::{Confirm, Input};

use crate::config::AppConfig;

use super::helpers::theme as dialog_theme;

pub(super) async fn configure_spotify(cfg: &mut AppConfig) -> anyhow::Result<()> {
    println!();
    println!("  {}", style("Configure Spotify").bold());
    println!();
    println!(
        "  {}",
        style("A custom Client ID is used for Spotify Web API requests.").dim()
    );
    println!(
        "  {}",
        style("The built-in client is reserved for librespot streaming.").dim()
    );
    println!();
    println!(
        "  {}",
        style("Leave the Client ID blank for streaming-only mode, or provide").dim()
    );
    println!(
        "  {}",
        style("your own Spotify Developer app Client ID instead.").dim()
    );
    println!();

    let use_custom = Confirm::with_theme(&dialog_theme())
        .with_prompt("Configure a Spotify Web API Client ID?")
        .default(false)
        .interact()?;

    if !use_custom {
        cfg.spotify.client_id = None;
        crate::config::clear_refresh_token();
        crate::config::clear_streaming_refresh_token();
    }

    if use_custom {
        let redirect_uri = "http://127.0.0.1:8888/callback";
        println!(
            "  {}  {}",
            style("[..]").cyan(),
            style("Opening Spotify Developer Dashboard in your browser...").dim()
        );
        let _ = open::that("https://developer.spotify.com/dashboard");

        let clipboard_msg = match arboard::Clipboard::new() {
            Ok(mut cb) => {
                if cb.set_text(redirect_uri).is_ok() {
                    "(already copied to your clipboard — just paste it)"
                } else {
                    "(copy it from below)"
                }
            }
            Err(_) => "(copy it from below)",
        };

        println!();
        println!(
            "  {}",
            style("Create a Spotify App to get your own Client ID:").dim()
        );
        println!(
            "  {}  {}",
            style("1.").cyan(),
            style("Click \"Create app\" (dashboard should be open in your browser)").dim()
        );
        println!(
            "  {}  {}",
            style("2.").cyan(),
            style("Give it any name & description, accept the terms").dim()
        );
        println!(
            "  {}  {}  {}",
            style("3.").cyan(),
            style("Add this Redirect URI:").dim(),
            style(clipboard_msg).yellow()
        );
        println!("  {}       {}", "", style(redirect_uri).yellow().bold());
        println!(
            "  {}  {}",
            style("4.").cyan(),
            style("Click \"Save\", then copy the Client ID and paste it below").dim()
        );
        println!();

        let client_id: String = Input::with_theme(&dialog_theme())
            .with_prompt("Spotify Client ID")
            .allow_empty(true)
            .interact_text()?;

        let trimmed = client_id.trim().to_string();
        if !trimmed.is_empty() {
            if trimmed.len() < 10 {
                println!(
                    "  {}  {}",
                    style("!").yellow(),
                    style(
                        "That doesn't look like a valid Client ID. It will be saved but may not work."
                    )
                    .dim()
                );
            }
            cfg.spotify.client_id = Some(trimmed);
        }
    }

    cfg.save()?;

    println!();
    let do_auth = Confirm::with_theme(&dialog_theme())
        .with_prompt("Authenticate with Spotify now? (opens browser)")
        .default(true)
        .interact()?;
    if do_auth {
        let has_web_api_client = cfg.get_client_id().is_some();

        if has_web_api_client {
            if let Some(cid) = cfg.get_client_id() {
                println!();
                println!(
                    "  {}  {}",
                    style("[..]").cyan(),
                    style("Starting authorization (2 steps: Web API + streaming)").dim()
                );
                match crate::spotify::auth::SpotifyAuth::authenticate_both(&cid).await {
                    Ok((web_api_refresh, streaming_refresh)) => {
                        crate::config::save_refresh_token(&web_api_refresh);
                        crate::config::save_streaming_refresh_token(&streaming_refresh);
                        println!();
                        println!(
                            "  {}  {}",
                            style("[OK]").green(),
                            style("Web API + Streaming authenticated.").bold()
                        );
                    }
                    Err(e) => {
                        if e.to_string().contains("Authentication cancelled") {
                            return Err(e);
                        }
                        println!("  {}  Authentication failed: {e}", style("[ERROR]").red());
                        println!(
                            "  {}",
                            style("You can authenticate later by launching isi-music normally.")
                                .dim()
                        );
                    }
                }
            }
        } else {
            println!();
            println!(
                "  {}  {}",
                style("[..]").cyan(),
                style("Opening Spotify authorization in your browser...").dim()
            );
            let result = crate::spotify::auth::SpotifyAuth::authenticate_with_client_id(
                crate::config::OFFICIAL_CLIENT_ID,
            )
            .await;

            match result {
                Ok((_access_token, refresh_token, _expires_in)) => {
                    crate::config::save_streaming_refresh_token(&refresh_token);
                    println!(
                        "  {}  {}",
                        style("[OK]").green(),
                        style("Streaming authenticated.").bold()
                    );
                }
                Err(e) => {
                    if e.to_string().contains("Authentication cancelled") {
                        return Err(e);
                    }
                    println!("  {}  Authentication failed: {e}", style("[ERROR]").red());
                    println!(
                        "  {}",
                        style("You can authenticate later by launching isi-music normally.").dim()
                    );
                }
            }
        }
    }

    Ok(())
}
