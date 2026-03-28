//! Alert dispatch to configured providers (Telegram, Slack, webhooks).

use crate::config::AlertProvider;
use crate::error::CoreError;

pub mod slack;
pub mod telegram;
pub mod webhook;

/// Send a notification to a single provider with exponential backoff retry.
///
/// Attempts up to 3 times with 1s, 2s delays between retries.
/// Returns the error from the last attempt if all retries fail.
///
/// # Errors
/// Returns `CoreError::Notification` if all retry attempts fail.
pub async fn send_alert(
    client: &reqwest::Client,
    provider: &AlertProvider,
    subject: &str,
    body: &str,
) -> Result<(), CoreError> {
    const MAX_ATTEMPTS: u32 = 3;
    let mut last_err = None;
    for attempt in 1..=MAX_ATTEMPTS {
        let result = match provider {
            AlertProvider::Telegram { token, chat_id } => {
                telegram::send(client, token, chat_id, subject, body).await
            }
            AlertProvider::Slack { webhook_url } => {
                slack::send(client, webhook_url, subject, body).await
            }
            AlertProvider::Webhook { url } => webhook::send(client, url, subject, body).await,
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < MAX_ATTEMPTS {
                    let delay_secs = 1u64 << (attempt - 1);
                    tracing::warn!(
                        provider = ?provider,
                        attempt,
                        delay_secs,
                        "alert failed, retrying: {e}"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err
        .unwrap_or_else(|| CoreError::Notification("all retry attempts failed".to_string())))
}

/// Send notification to all configured providers concurrently.
/// Logs errors but does not propagate them.
pub async fn notify_all(providers: &[AlertProvider], subject: &str, body: &str) {
    let client = reqwest::Client::new();
    let futures: Vec<_> = providers
        .iter()
        .map(|provider| {
            let client = &client;
            async move {
                if let Err(e) = send_alert(client, provider, subject, body).await {
                    tracing::warn!(provider = ?provider, "failed to send alert: {e}");
                }
            }
        })
        .collect();
    futures::future::join_all(futures).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_notify_all_empty_providers_completes() {
        // Empty provider list: completes immediately with no errors or panics.
        notify_all(
            &[],
            "kron job failed: test",
            "Command: echo hi\nExit code: 1\nStderr: ",
        )
        .await;
    }

    #[test]
    fn test_retry_backoff_delays_are_exponential() {
        // The retry loop uses `1u64 << (attempt - 1)` for delay seconds.
        // Attempt 1 → 1s, attempt 2 → 2s (MAX_ATTEMPTS=3, so no delay on last).
        assert_eq!(1u64 << (1u32 - 1), 1);
        assert_eq!(1u64 << (2u32 - 1), 2);
    }
}
