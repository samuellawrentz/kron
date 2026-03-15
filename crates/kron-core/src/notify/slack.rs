use crate::error::CoreError;

/// Send a message via a Slack incoming webhook.
///
/// # Errors
/// Returns `CoreError::Notification` if the request fails or the webhook returns an error status.
pub async fn send(webhook_url: &str, subject: &str, body: &str) -> Result<(), CoreError> {
    let text = format!("*{subject}*\n{body}");
    let client = reqwest::Client::new();
    let resp = client
        .post(webhook_url)
        .json(&serde_json::json!({ "text": text }))
        .send()
        .await
        .map_err(|e| CoreError::Notification(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(CoreError::Notification(format!(
            "slack webhook returned {}",
            resp.status()
        )));
    }
    Ok(())
}
