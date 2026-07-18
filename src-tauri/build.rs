use std::env;

const CANDIDATE_CHROMIUM_EXTENSION_ID: &str = "fcjcenonhhfdblnoafphpcddpmppdeag";
const CANDIDATE_FIREFOX_EXTENSION_ID: &str = "vibe-downloader-candidate@local";
const CANDIDATE_CHROMIUM_PUBLIC_KEY: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAnzbGr4WRbECji+lvxIQXGmL+oTLh0ErT02tiYsam6DTPsaTxmTRaeUvrfjQEPLDNDbdTEBF5IsP0txSoU2BpLdyMOvjgwYspG6vptreDbEwozqqV4ZiM2pKMCEvfXWzpCQPBxqR0JaMonWmK5909iv5tQz2ynsa0o43qyid6VFF9y8o4YVADUck3RLqiRSykvnnvJGiXYuDFlNYRAJhE8rj0UBkA0oeqWYLWAJicagfNnRKpxeccdXkSd1eSSbaQ1y4hkVDissCPAxe9Yl1kwKNFc6RYcQ/98HvuMSnul/eGg/z0Ob+KfovgIsU3Hiubrl6MUHBNJFkj6X2sMQ/inwIDAQAB";

fn main() {
    // Tell Cargo to re-run the build script (and thus re-expand
    // `sqlx::migrate!()`) whenever any file under `src/db/migrations/`
    // changes — without this, adding a new migration file would not be
    // picked up until a Rust source file also changes, because proc
    // macros cannot themselves watch external directories on stable
    // Rust. See https://docs.rs/sqlx/0.9.0/sqlx/macro.migrate.html for
    // the official recommendation.
    println!("cargo:rerun-if-changed=src/db/migrations");
    for name in [
        "VIBE_BROWSER_PROFILE",
        "VIBE_BROWSER_EXPERIMENTAL_CAPTURE",
        "VIBE_ALLOW_CANDIDATE_EXTENSION_IDS",
        "VIBE_CHROME_EXTENSION_ID",
        "VIBE_EDGE_EXTENSION_ID",
        "VIBE_FIREFOX_EXTENSION_ID",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }

    let cargo_profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let browser_profile = env::var("VIBE_BROWSER_PROFILE").unwrap_or_else(|_| {
        if cargo_profile == "debug" {
            "dev".to_string()
        } else {
            "candidate".to_string()
        }
    });
    if !matches!(browser_profile.as_str(), "dev" | "candidate" | "release") {
        panic!("Unsupported VIBE_BROWSER_PROFILE: {browser_profile}");
    }

    let capture_requested = env::var("VIBE_BROWSER_EXPERIMENTAL_CAPTURE")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
    if browser_profile != "dev" && capture_requested {
        panic!("Experimental browser capture is only available in the dev profile");
    }

    let allow_candidate_extension_ids = env::var("VIBE_ALLOW_CANDIDATE_EXTENSION_IDS")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });

    let formal_configured = [
        env::var("VIBE_CHROME_EXTENSION_ID").ok(),
        env::var("VIBE_EDGE_EXTENSION_ID").ok(),
        env::var("VIBE_FIREFOX_EXTENSION_ID").ok(),
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.trim().is_empty());

    let (chrome_id, edge_id, firefox_id, use_candidate_public_key) = if browser_profile == "release"
    {
        if !formal_configured && allow_candidate_extension_ids {
            (
                CANDIDATE_CHROMIUM_EXTENSION_ID.to_string(),
                CANDIDATE_CHROMIUM_EXTENSION_ID.to_string(),
                CANDIDATE_FIREFOX_EXTENSION_ID.to_string(),
                true,
            )
        } else {
            (
                require_chromium_extension_id("VIBE_CHROME_EXTENSION_ID"),
                require_chromium_extension_id("VIBE_EDGE_EXTENSION_ID"),
                require_firefox_extension_id(),
                false,
            )
        }
    } else {
        (
            CANDIDATE_CHROMIUM_EXTENSION_ID.to_string(),
            CANDIDATE_CHROMIUM_EXTENSION_ID.to_string(),
            CANDIDATE_FIREFOX_EXTENSION_ID.to_string(),
            true,
        )
    };

    println!("cargo:rustc-env=VIBE_BROWSER_PROFILE_RESOLVED={browser_profile}");
    println!(
        "cargo:rustc-env=VIBE_BROWSER_CAPTURE_AVAILABLE={}",
        browser_profile == "dev" && capture_requested
    );
    println!("cargo:rustc-env=VIBE_CHROME_EXTENSION_ID_RESOLVED={chrome_id}");
    println!("cargo:rustc-env=VIBE_EDGE_EXTENSION_ID_RESOLVED={edge_id}");
    println!("cargo:rustc-env=VIBE_FIREFOX_EXTENSION_ID_RESOLVED={firefox_id}");
    println!(
        "cargo:rustc-env=VIBE_CHROMIUM_PUBLIC_KEY_RESOLVED={}",
        if use_candidate_public_key {
            CANDIDATE_CHROMIUM_PUBLIC_KEY
        } else {
            ""
        }
    );

    // On Windows, test binaries and non-app binaries (like vibe-native-host)
    // link against Tauri's tray/window code, which imports TaskDialogIndirect
    // from comctl32.dll. That export only exists in Common Controls v6, which
    // requires an application manifest. The main app binary gets a manifest
    // via tauri-build, but test/binaries do not — so the loader fails with
    // STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139). Injecting the v6 dependency
    // via /MANIFESTDEPENDENCY makes the linker embed the required assembly
    // reference into every target's manifest.
    #[cfg(target_os = "windows")]
    {
        let dep = "type='win32' \
            name='Microsoft.Windows.Common-Controls' \
            version='6.0.0.0' \
            processorArchitecture='*' \
            publicKeyToken='6595b64144ccf1df' \
            language='*'";
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:{dep}");
    }

    tauri_build::build()
}

fn is_chromium_extension_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

fn is_firefox_extension_id(value: &str) -> bool {
    !value.trim().is_empty()
        && !value.chars().any(char::is_whitespace)
        && value != "vibe-downloader@example.invalid"
        && (value.contains('@') || (value.starts_with('{') && value.ends_with('}')))
}

fn optional_chromium_extension_id(name: &str) -> Option<String> {
    env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else if is_chromium_extension_id(trimmed) {
            Some(trimmed.to_string())
        } else {
            panic!("{name} must be a 32-character Chromium extension ID using letters a-p");
        }
    })
}

fn optional_firefox_extension_id() -> Option<String> {
    env::var("VIBE_FIREFOX_EXTENSION_ID").ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else if is_firefox_extension_id(trimmed) {
            Some(trimmed.to_string())
        } else {
            panic!("VIBE_FIREFOX_EXTENSION_ID must be a non-placeholder email-like ID or braced UUID");
        }
    })
}

fn require_chromium_extension_id(name: &str) -> String {
    optional_chromium_extension_id(name)
        .unwrap_or_else(|| panic!("{name} is required for release builds"))
}

fn require_firefox_extension_id() -> String {
    optional_firefox_extension_id()
        .unwrap_or_else(|| panic!("VIBE_FIREFOX_EXTENSION_ID is required for release builds"))
}
