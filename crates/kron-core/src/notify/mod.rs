use crate::config::AlertProvider;
use crate::error::CoreError;

pub mod slack;
pub mod telegram;
pub mod webhook;

/// Send a notification to a single provider.
///
/// # Errors
/// Returns `CoreError::Notification` if the HTTP request fails or the provider returns an error.
pub async fn send_alert(
    client: &reqwest::Client,
    provider: &AlertProvider,
    subject: &str,
    body: &str,
) -> Result<(), CoreError> {
    match provider {
        AlertProvider::Telegram { token, chat_id } => {
            telegram::send(client, token, chat_id, subject, body).await
        }
        AlertProvider::Slack { webhook_url } => {
            slack::send(client, webhook_url, subject, body).await
        }
        AlertProvider::Webhook { url } => webhook::send(client, url, subject, body).await,
    }
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
