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

    let (chrome_id, edge_id, firefox_id) = if browser_profile == "release" {
        (
            require_chromium_extension_id("VIBE_CHROME_EXTENSION_ID"),
            require_chromium_extension_id("VIBE_EDGE_EXTENSION_ID"),
            require_firefox_extension_id(),
        )
    } else {
        (
            CANDIDATE_CHROMIUM_EXTENSION_ID.to_string(),
            CANDIDATE_CHROMIUM_EXTENSION_ID.to_string(),
            CANDIDATE_FIREFOX_EXTENSION_ID.to_string(),
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
        if browser_profile == "release" {
            ""
        } else {
            CANDIDATE_CHROMIUM_PUBLIC_KEY
        }
    );
    tauri_build::build()
}

fn require_chromium_extension_id(name: &str) -> String {
    let value = env::var(name).unwrap_or_else(|_| panic!("{name} is required for release builds"));
    let valid = value.len() == 32 && value.bytes().all(|byte| (b'a'..=b'p').contains(&byte));
    if !valid {
        panic!("{name} must be a 32-character Chromium extension ID using letters a-p");
    }
    value
}

fn require_firefox_extension_id() -> String {
    let value = env::var("VIBE_FIREFOX_EXTENSION_ID")
        .unwrap_or_else(|_| panic!("VIBE_FIREFOX_EXTENSION_ID is required for release builds"));
    let valid = !value.trim().is_empty()
        && !value.chars().any(char::is_whitespace)
        && value != "vibe-downloader@example.invalid"
        && (value.contains('@') || (value.starts_with('{') && value.ends_with('}')));
    if !valid {
        panic!("VIBE_FIREFOX_EXTENSION_ID must be a non-placeholder email-like ID or braced UUID");
    }
    value
}
