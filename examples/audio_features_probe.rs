//! Diagnostic: whether Spotify still answers `/v1/audio-features` for this
//! app's shared web credential, and whether its tempo agrees with Deezer's
//! for the same recording.
//!
//!   cargo run --example audio_features_probe -- spotify:track:4uLU6hMCjMI75M1A2tKUQC
//!
//! The endpoint was deprecated for new apps on 27 November 2024 and only
//! apps holding a quota extension from before then may still use it. This
//! prints the status and body rather than assuming either answer.

use chanceify::api::models::Track;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    fastframe_log::Logging::new("chanceify", env!("CARGO_PKG_VERSION"))
        .filter("warn")
        .init()?;
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into());
    let id = arg.rsplit(':').next().unwrap_or_default().to_string();

    let dirs = chanceify::paths::AppDirs::discover();
    {
        let store = chanceify::credentials::Store::new(dirs);
        let loaded = store
            .lease(chanceify::credentials::Slot::Shared)
            .load()
            .await?;
        if let Some(warning) = loaded.warning {
            eprintln!("{warning}");
        }
        let Some(chanceify::credentials::Grant::Web(token)) = loaded.grant else {
            anyhow::bail!("No stored web sign-in found");
        };
        println!("client_id: {}", token.client_id);
        let client = reqwest::Client::builder()
            .user_agent("chanceify/0.1")
            .build()?;
        let mut token = token;
        if token.expired() {
            println!("refreshing…");
            let refreshed =
                chanceify::auth::refresh(&client, &token.client_id, &token.refresh_token).await?;
            token = chanceify::auth::StoredToken::from_response(
                &token.client_id,
                refreshed,
                Some(&token.refresh_token),
            )?;
        }

        let headers = {
            let mut map = reqwest::header::HeaderMap::new();
            map.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", token.access_token).parse()?,
            );
            map
        };

        // 1. The track itself, for the ISRC.
        let response = client
            .get(format!("https://api.spotify.com/v1/tracks/{id}"))
            .headers(headers.clone())
            .send()
            .await?;
        println!("\n/v1/tracks/{id} -> {}", response.status().as_u16());
        let body = response.text().await?;
        let track: Track = serde_json::from_str(&body).map_err(|error| {
            anyhow::anyhow!(
                "track decode: {error}; body: {}",
                &body[..body.len().min(400)]
            )
        })?;
        let isrc = track.external_ids.isrc.clone();
        println!("title:  {}", track.name);
        println!("isrc:   {}", isrc.as_deref().unwrap_or("(none)"));

        // 2. The audio features, which is the thing under test.
        let response = client
            .get(format!("https://api.spotify.com/v1/audio-features/{id}"))
            .headers(headers)
            .send()
            .await?;
        println!(
            "\n/v1/audio-features/{id} -> {}",
            response.status().as_u16()
        );
        let spotify_body = response.text().await?;
        println!("body: {}", &spotify_body[..spotify_body.len().min(600)]);

        // 3. Deezer's reading of the same recording, for comparison.
        if let Some(isrc) = isrc {
            let response = client
                .get(format!("https://api.deezer.com/track/isrc:{isrc}"))
                .send()
                .await?;
            println!(
                "\ndeezer track/isrc:{isrc} -> {}",
                response.status().as_u16()
            );
            let body = response.text().await?;
            let field = |name: &str| {
                body.split(&format!("\"{name}\":"))
                    .nth(1)
                    .map(|rest| rest.split(&[',', '}'][..]).next().unwrap_or("").to_string())
            };
            println!("deezer bpm:    {}", field("bpm").unwrap_or_default());
            println!("deezer gain:   {}", field("gain").unwrap_or_default());
            println!("deezer rank:   {}", field("rank").unwrap_or_default());
            println!(
                "deezer title:  {}",
                field("title_short").unwrap_or_default()
            );
        }
    }
    Ok(())
}
