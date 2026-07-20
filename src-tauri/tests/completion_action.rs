//! PERF-07: completion-action commands must time out without pinning Tokio.

use std::time::{Duration, Instant};

use tauri_app_lib::platform::{run_user_command_with_timeout, validate_user_command};

fn hanging_command() -> &'static str {
    if cfg!(target_os = "windows") {
        "ping -n 60 127.0.0.1"
    } else {
        "sleep 60"
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hanging_completion_command_times_out_without_blocking_runtime() {
    validate_user_command(hanging_command()).expect("hanging fixture must pass S-4 validation");

    let hanging = tokio::spawn(async {
        run_user_command_with_timeout(hanging_command(), Duration::from_secs(2)).await
    });

    // While the command is waiting, the runtime must still schedule other work.
    let mut ticks = 0_u32;
    let deadline = Instant::now() + Duration::from_millis(1_500);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
        ticks = ticks.saturating_add(1);
    }
    assert!(
        ticks >= 50,
        "expected concurrent async ticks while command is pending, got {ticks}"
    );

    let err = hanging
        .await
        .expect("join")
        .expect_err("hanging command must time out");
    assert!(
        err.contains("completion_command_timeout"),
        "expected structured timeout payload, got: {err}"
    );
}
