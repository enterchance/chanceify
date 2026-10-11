//! Compiles translations and embeds Windows resources.

/// Emit the short revision and build time the About box shows, so a build
/// identifies itself. `SOURCE_DATE_EPOCH` is honoured for reproducible builds;
/// without it the time is this build's, which is the point of a local build.
fn build_stamp() {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|text| !text.trim().is_empty());
    let revision = if dirty {
        format!("{revision}+dirty")
    } else {
        revision
    };
    println!("cargo:rustc-env=CHANCEIFY_REVISION={revision}");
    // Seconds since the epoch, from SOURCE_DATE_EPOCH when the build is meant
    // to be reproducible. A build script cannot format a date without a crate,
    // so the app formats this itself.
    let epoch = std::env::var("SOURCE_DATE_EPOCH").unwrap_or_else(|_| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or(0)
            .to_string()
    });
    println!("cargo:rustc-env=CHANCEIFY_BUILD_EPOCH={epoch}");
    // Rebuild when the revision changes so the stamp never lags the checkout.
    println!("cargo:rerun-if-changed=.git/HEAD");
}

/// The Last.fm key and secret come from `lastfm-keys.txt` (two lines: key,
/// then secret), a file that is never committed. Without it the app asks
/// each person for a key of their own.
fn lastfm_keys() {
    println!("cargo:rerun-if-changed=lastfm-keys.txt");
    let text = std::fs::read_to_string("lastfm-keys.txt").unwrap_or_default();
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    if let (Some(key), Some(secret)) = (lines.next(), lines.next()) {
        println!("cargo:rustc-env=CHANCEIFY_LASTFM_KEY={key}");
        println!("cargo:rustc-env=CHANCEIFY_LASTFM_SECRET={secret}");
    }
}

/// The address of the builder's own Last.fm helper (a Cloudflare Worker),
/// from `lastfm-proxy.txt`: one line, never committed. Without it the source
/// has no address at all and the Last.fm key goes in `lastfm-keys.txt`.
fn lastfm_proxy() {
    println!("cargo:rerun-if-changed=lastfm-proxy.txt");
    let text = std::fs::read_to_string("lastfm-proxy.txt").unwrap_or_default();
    let address = text.lines().map(str::trim).find(|line| !line.is_empty());
    if let Some(address) = address {
        println!("cargo:rustc-env=CHANCEIFY_LASTFM_PROXY={address}");
    }
}

fn main() {
    fastframe_i18n::build::compile_catalogs("assets/i18n");
    build_stamp();
    lastfm_keys();
    lastfm_proxy();
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/chanceify.ico");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/chanceify.ico")
            .set("ProductName", "chanceify™")
            .set("FileDescription", "chanceify™ - music reimagined");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources not embedded: {error}");
        }
    }
}
