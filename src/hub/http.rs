use std::sync::OnceLock;
use std::time::Duration;

use rand::Rng;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_IDEMPOTENT_ATTEMPTS: usize = 3;
pub(crate) const WEBSOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

static CLIENT: OnceLock<Client> = OnceLock::new();
static STREAMING_CLIENT: OnceLock<Client> = OnceLock::new();

pub(crate) fn client() -> Client {
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(REQUEST_TIMEOUT)
                .pool_idle_timeout(POOL_IDLE_TIMEOUT)
                .build()
                .expect("Portal Hub HTTP client configuration should be valid")
        })
        .clone()
}

pub(crate) fn streaming_client() -> Client {
    STREAMING_CLIENT
        .get_or_init(|| {
            Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .pool_idle_timeout(POOL_IDLE_TIMEOUT)
                .build()
                .expect("Portal Hub streaming HTTP client configuration should be valid")
        })
        .clone()
}

pub(crate) async fn json<T, F>(
    retry_idempotent: bool,
    build_request: F,
    send_error: &str,
    status_error: &str,
    parse_error: &str,
) -> Result<T, String>
where
    T: DeserializeOwned,
    F: Fn(&Client) -> RequestBuilder,
{
    let client = client();
    let response = send_with_retry(|| build_request(&client), retry_idempotent, send_error).await?;
    parse_json_response(response, status_error, parse_error).await
}

pub(crate) async fn authenticated_json<T, F>(
    hub_url: &str,
    retry_idempotent: bool,
    build_request: F,
    send_error: &str,
    status_error: &str,
    parse_error: &str,
) -> Result<T, String>
where
    T: DeserializeOwned,
    F: Fn(&Client, &str) -> RequestBuilder,
{
    let response =
        send_authenticated_response(hub_url, retry_idempotent, build_request, send_error).await?;
    parse_json_response(response, status_error, parse_error).await
}

pub(crate) async fn authenticated_empty<F>(
    hub_url: &str,
    retry_idempotent: bool,
    build_request: F,
    send_error: &str,
    status_error: &str,
) -> Result<(), String>
where
    F: Fn(&Client, &str) -> RequestBuilder,
{
    let response =
        send_authenticated_response(hub_url, retry_idempotent, build_request, send_error).await?;
    response
        .error_for_status()
        .map_err(|error| format!("{status_error}: {error}"))?;
    Ok(())
}

pub(crate) async fn authenticated_streaming_response<F>(
    hub_url: &str,
    retry_idempotent: bool,
    build_request: F,
    send_error: &str,
) -> Result<Response, String>
where
    F: Fn(&Client, &str) -> RequestBuilder,
{
    send_authenticated_response_with_client(
        streaming_client(),
        hub_url,
        retry_idempotent,
        build_request,
        send_error,
    )
    .await
}

async fn send_authenticated_response<F>(
    hub_url: &str,
    retry_idempotent: bool,
    build_request: F,
    send_error: &str,
) -> Result<Response, String>
where
    F: Fn(&Client, &str) -> RequestBuilder,
{
    send_authenticated_response_with_client(
        client(),
        hub_url,
        retry_idempotent,
        build_request,
        send_error,
    )
    .await
}

async fn send_authenticated_response_with_client<F>(
    client: Client,
    hub_url: &str,
    retry_idempotent: bool,
    build_request: F,
    send_error: &str,
) -> Result<Response, String>
where
    F: Fn(&Client, &str) -> RequestBuilder,
{
    let token = crate::hub::auth::load_access_token(hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    let response = send_with_retry(
        || build_request(&client, &token),
        retry_idempotent,
        send_error,
    )
    .await?;

    if response.status() != StatusCode::UNAUTHORIZED {
        return Ok(response);
    }

    let token = crate::hub::auth::refresh_access_token(hub_url)
        .await?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    send_with_retry(
        || build_request(&client, &token),
        retry_idempotent,
        send_error,
    )
    .await
}

async fn send_with_retry<F>(
    mut build_request: F,
    retry_idempotent: bool,
    send_error: &str,
) -> Result<Response, String>
where
    F: FnMut() -> RequestBuilder,
{
    let max_attempts = if retry_idempotent {
        MAX_IDEMPOTENT_ATTEMPTS
    } else {
        1
    };

    for attempt in 0..max_attempts {
        match build_request().send().await {
            Ok(response)
                if retry_idempotent
                    && attempt + 1 < max_attempts
                    && is_transient_status(response.status()) =>
            {
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Ok(response) => return Ok(response),
            Err(error)
                if retry_idempotent
                    && attempt + 1 < max_attempts
                    && (error.is_timeout() || error.is_connect() || error.is_request()) =>
            {
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(format!("{send_error}: {error}")),
        }
    }

    Err(format!("{send_error}: retry attempts exhausted"))
}

async fn parse_json_response<T: DeserializeOwned>(
    response: Response,
    status_error: &str,
    parse_error: &str,
) -> Result<T, String> {
    response
        .error_for_status()
        .map_err(|error| format!("{status_error}: {error}"))?
        .json()
        .await
        .map_err(|error| format!("{parse_error}: {error}"))
}

fn is_transient_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(attempt: usize) -> Duration {
    let capped = attempt.min(4) as u32;
    let base_ms = 150u64.saturating_mul(2u64.saturating_pow(capped));
    let jitter_ms = rand::thread_rng().gen_range(0..=100);
    Duration::from_millis(base_ms.saturating_add(jitter_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_statuses_are_retryable() {
        assert!(is_transient_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_transient_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_transient_status(StatusCode::BAD_GATEWAY));
        assert!(!is_transient_status(StatusCode::UNAUTHORIZED));
        assert!(!is_transient_status(StatusCode::CONFLICT));
    }

    #[test]
    fn retry_delay_is_bounded_and_grows() {
        let first = retry_delay(0);
        let later = retry_delay(3);

        assert!(first >= Duration::from_millis(150));
        assert!(first <= Duration::from_millis(250));
        assert!(later >= Duration::from_millis(1200));
        assert!(later <= Duration::from_millis(1300));
    }
}
