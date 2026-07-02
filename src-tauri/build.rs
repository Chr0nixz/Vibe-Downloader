fn main() {
    // Tell Cargo to re-run the build script (and thus re-expand
    // `sqlx::migrate!()`) whenever any file under `src/db/migrations/`
    // changes — without this, adding a new migration file would not be
    // picked up until a Rust source file also changes, because proc
    // macros cannot themselves watch external directories on stable
    // Rust. See https://docs.rs/sqlx/0.9.0/sqlx/macro.migrate.html for
    // the official recommendation.
    println!("cargo:rerun-if-changed=src/db/migrations");
    tauri_build::build()
}
