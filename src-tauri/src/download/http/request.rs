use std::time::Duration;

use reqwest::{
    header::{ACCEPT_ENCODING, RANGE, RETRY_AFTER},
    Client, Response, StatusCode,
};

pub(super) async fn send_head_with_retry(
    client: &Client,
    url: &str,
) -> Result<Response, reqwest::Error> {
    let mut last_error = None;
    for attempt in 0..3 {
        match client
            .head(url)
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error) => {
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }
    Err(last_error.expect("head request attempted"))
}

pub(super) async fn send_get_with_retry(
    client: &Client,
    url: &str,
    range: Option<String>,
) -> Result<Response, String> {
    let mut last_error = None;
    for attempt in 0..3 {
        let mut request = client.get(url).header(ACCEPT_ENCODING, "identity");
        if let Some(range) = range.as_deref() {
            request = request.header(RANGE, range);
        }
        match request.send().await {
            Ok(response) => return Ok(response),
            Err(error) => {
                last_error = Some(error);
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }

    Err(format!(
        "Could not connect to the server: {}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "request failed".to_string())
    ))
}

pub(super) fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

pub(super) fn retry_after_duration(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .map(|duration| duration.min(Duration::from_secs(60)))
}
