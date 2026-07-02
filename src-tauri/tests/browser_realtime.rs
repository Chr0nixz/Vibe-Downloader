//! S-1.1: WS 浏览器实时桥敏感设置拒绝测试。
//!
//! `is_sensitive_settings_update` 是纯函数，检查 `updateSettings` 载荷是否
//! 包含敏感字段（forwardHeaders/forwardHeadersMode/experimentalCaptureEnabled/
//! allowIntranetHandoff）。这些字段必须由用户在主窗口 UI 中显式操作，不能
//! 由持有 WS bootstrap token 的本地进程直接修改。

use serde_json::json;
use tauri_app_lib::commands::browser::is_sensitive_settings_update;

#[test]
fn test_is_sensitive_settings_update_detects_sensitive_fields() {
    // 包含全部 4 个敏感字段的 payload。
    let payload = json!({
        "forwardHeaders": true,
        "forwardHeadersMode": "enabled",
        "experimentalCaptureEnabled": true,
        "allowIntranetHandoff": true,
        // 同时包含一个安全字段，确保不影响检测。
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

    // 只包含部分敏感字段。
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
    // 仅包含安全字段的 payload — 不应触发任何拒绝。
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

    // 空 payload 也应返回空。
    let payload = json!({});
    let map = payload.as_object().unwrap();
    let conflicts = is_sensitive_settings_update(map);
    assert!(conflicts.is_empty(), "empty payload must return empty");
}
