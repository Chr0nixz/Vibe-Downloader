fn main() {
    tauri_app_lib::export_typescript_bindings().expect("Failed to export TypeScript bindings");
}
