use crate::error::CoreError;

/// Send a message via a Slack incoming webhook.
///
/// # Errors
/// Returns `CoreError::Notification` if the request fails or the webhook returns an error status.
pub async fn send(
    client: &reqwest::Client,
    webhook_url: &str,
    subject: &str,
    body: &str,
) -> Result<(), CoreError> {
    // Slack requires &, <, > to be HTML-escaped in message text; job output is
    // untrusted, so escape it and keep bolding only on the (kron-generated) subject.
    let text = format!("*{}*\n{}", escape_slack(subject), escape_slack(body));
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

/// Escape the three characters Slack treats specially in message text.
fn escape_slack(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
