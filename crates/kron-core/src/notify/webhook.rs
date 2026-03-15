use crate::error::CoreError;

/// Send a JSON payload to a generic webhook URL.
///
/// # Errors
/// Returns `CoreError::Notification` if the request fails or the server returns an error status.
pub async fn send(url: &str, subject: &str, body: &str) -> Result<(), CoreError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .json(&serde_json::json!({
            "subject": subject,
            "body": body,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
        .send()
        .await
        .map_err(|e| CoreError::Notification(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(CoreError::Notification(format!(
            "webhook returned {}",
            resp.status()
        )));
    }
    Ok(())
}
