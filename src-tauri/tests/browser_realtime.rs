//! S-1.1: WS browser realtime bridge sensitive settings rejection tests.
//!
//! `is_sensitive_settings_update` is a pure function that checks whether the `updateSettings` payload
//! contains sensitive fields (forwardHeaders/forwardHeadersMode/experimentalCaptureEnabled/
//! allowIntranetHandoff). These fields must be explicitly operated by the user in the main window UI,
//! not directly by a local process holding the WS bootstrap token.

use serde_json::json;
use tauri_app_lib::commands::browser::is_sensitive_settings_update;

#[test]
fn test_is_sensitive_settings_update_detects_sensitive_fields() {
    // Payload containing all 4 sensitive fields.
    let payload = json!({
        "forwardHeaders": true,
        "forwardHeadersMode": "enabled",
        "experimentalCaptureEnabled": true,
        "allowIntranetHandoff": true,
        // Also includes a safe field to ensure it does not affect detection.
        "minSizeBytes": "1024"
    });
    let map = payload.as_object().unwrap();
    let mut conflicts = is_sensitive_settings_update(map);
    conflicts.sort();
    let mut expected = vec![
        "forwardHeaders".to_string(),
        "forwardHeadersMode".to_string(),
        "experimentalCaptureEnabled".to_string(),
        "allowIntranetHandoff".to_string(),
    ];
    expected.sort();
    assert_eq!(
        conflicts, expected,
        "all 4 sensitive fields must be detected"
    );

    // Contains only some sensitive fields.
    let payload = json!({
        "allowIntranetHandoff": true,
        "fileExtensions": ["zip", "exe"]
    });
    let map = payload.as_object().unwrap();
    let conflicts = is_sensitive_settings_update(map);
    assert_eq!(
        conflicts,
        vec!["allowIntranetHandoff".to_string()],
        "only allowIntranetHandoff should be flagged"
    );
}

#[test]
fn test_is_sensitive_settings_update_allows_safe_fields() {
    // Payload with only safe fields — should not trigger any rejection.
    let payload = json!({
        "minSizeBytes": "1024",
        "fileExtensions": ["zip", "exe", "pdf"],
        "siteRules": [],
        "autoIntercept": true
    });
    let map = payload.as_object().unwrap();
    let conflicts = is_sensitive_settings_update(map);
    assert!(
        conflicts.is_empty(),
        "safe fields must not trigger rejection, got: {conflicts:?}"
    );

    // An empty payload should also return empty.
    let payload = json!({});
    let map = payload.as_object().unwrap();
    let conflicts = is_sensitive_settings_update(map);
    assert!(conflicts.is_empty(), "empty payload must return empty");
}
