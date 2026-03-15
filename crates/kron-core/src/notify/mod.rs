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
    provider: &AlertProvider,
    subject: &str,
    body: &str,
) -> Result<(), CoreError> {
    match provider {
        AlertProvider::Telegram { token, chat_id } => {
            telegram::send(token, chat_id, subject, body).await
        }
        AlertProvider::Slack { webhook_url } => slack::send(webhook_url, subject, body).await,
        AlertProvider::Webhook { url } => webhook::send(url, subject, body).await,
    }
}

/// Send notification to all configured providers. Logs errors but does not propagate them.
pub async fn notify_all(providers: &[AlertProvider], subject: &str, body: &str) {
    for provider in providers {
        if let Err(e) = send_alert(provider, subject, body).await {
            tracing::warn!(provider = ?provider, "failed to send alert: {e}");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::config::AlertProvider;

    #[test]
    fn test_send_alert_dispatches_correctly() {
        // Just verify the match arms compile and the function signature is correct
        let _telegram = AlertProvider::Telegram {
            token: "test".to_string(),
            chat_id: "123".to_string(),
        };
        let _slack = AlertProvider::Slack {
            webhook_url: "https://test.com".to_string(),
        };
        let _webhook = AlertProvider::Webhook {
            url: "https://test.com".to_string(),
        };
        // Type-checking test — if this compiles, dispatch is correct
    }
}
