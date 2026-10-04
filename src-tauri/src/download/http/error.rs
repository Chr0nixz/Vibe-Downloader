use reqwest::StatusCode;

use crate::models::AppErrorPayload;

pub(crate) fn format_http_status_with_retry_after(
    status: StatusCode,
    retry_after_at: Option<String>,
) -> String {
    match status.as_u16() {
        401 | 403 => AppErrorPayload::http_status(
            "http_denied",
            "The server denied access to this file.",
            false,
        )
        .command_error(),
        404 => AppErrorPayload::http_status(
            "http_not_found",
            "The file was not found on the server.",
            false,
        )
        .command_error(),
        429 => AppErrorPayload::http_status(
            "server_rate_limited",
            "The server is limiting requests. Try again later.",
            true,
        )
        .with_retry_after_at(retry_after_at)
        .command_error(),
        code if (500..=599).contains(&code) => AppErrorPayload::http_status(
            "server_error",
            format!("The server returned HTTP {code}."),
            true,
        )
        .with_retry_after_at(retry_after_at)
        .command_error(),
        code => format!("The server returned HTTP {code}."),
    }
}
