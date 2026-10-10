//! Compiles translations, embeds Windows resources and links GLEW for libprojectM.

/// Known names for vcpkg's static GLEW library, in preferred order.
/// Names differ across vcpkg versions and triplets, so use the installed one.
#[cfg(windows)]
const GLEW_NAMES: &[&str] = &["glew32s", "libglew32", "glew32"];

/// Returns the vcpkg triplet matching the target architecture and CRT mode.
#[cfg(windows)]
fn vcpkg_triplet() -> Option<String> {
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x64",
        Ok("aarch64") => "arm64",
        _ => return None,
    };
    let static_crt = std::env::var("CARGO_CFG_TARGET_FEATURE")
        .unwrap_or_default()
        .split(',')
        .any(|feature| feature == "crt-static");
    let suffix = if static_crt { "static" } else { "static-md" };
    Some(format!("{arch}-windows-{suffix}"))
}

/// Returns the known GLEW library installed in `lib`.
#[cfg(windows)]
fn glew_library(lib: &std::path::Path) -> Option<&'static str> {
    let present: Vec<String> = std::fs::read_dir(lib)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.eq_ignore_ascii_case("lib") {
                Some(path.file_stem()?.to_str()?.to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect();
    GLEW_NAMES
        .iter()
        .copied()
        .find(|name| present.iter().any(|found| found == name))
}

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

fn main() {
    fastframe_i18n::build::compile_catalogs("assets/i18n");
    build_stamp();
    lastfm_keys();
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/chanceify.ico");
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/chanceify.ico")
            .set("ProductName", "chanceify")
            .set("FileDescription", "music reimagined .");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources not embedded: {error}");
        }
        // Static libprojectM requires the GLEW library installed by vcpkg.
        if std::env::var_os("CARGO_FEATURE_MILKDROP").is_some() {
            println!("cargo:rerun-if-env-changed=VCPKG_INSTALLATION_ROOT");
            if let (Some(root), Some(triplet)) =
                (std::env::var_os("VCPKG_INSTALLATION_ROOT"), vcpkg_triplet())
            {
                let lib = std::path::Path::new(&root)
                    .join("installed")
                    .join(triplet)
                    .join("lib");
                println!("cargo:rustc-link-search=native={}", lib.display());
                match glew_library(&lib) {
                    Some(name) => println!("cargo:rustc-link-lib=static={name}"),
                    None => {
                        // Include the directory listing in the error for diagnosis.
                        let listing = std::fs::read_dir(&lib)
                            .map(|entries| {
                                entries
                                    .flatten()
                                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                            .unwrap_or_else(|error| format!("unreadable: {error}"));
                        println!(
                            "cargo:warning=no GLEW library in {}; it holds: {listing}",
                            lib.display()
                        );
                    }
                }
            }
        }
    }
}
