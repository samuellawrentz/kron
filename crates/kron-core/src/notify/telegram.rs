use crate::error::CoreError;

/// Send a message via the Telegram Bot API.
///
/// # Errors
/// Returns `CoreError::Notification` if the request fails or the API returns an error status.
pub async fn send(token: &str, chat_id: &str, subject: &str, body: &str) -> Result<(), CoreError> {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let text = format!("*{subject}*\n{body}");
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        }))
        .send()
        .await
        .map_err(|e| CoreError::Notification(e.to_string()))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CoreError::Notification(format!(
            "telegram API returned {status}: {body}"
        )));
    }
    Ok(())
}
