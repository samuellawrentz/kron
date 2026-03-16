use anyhow::{Context, Result};
use kron_core::config::{self, AlertConfig, AlertProvider};

fn add_provider(provider: AlertProvider, description: &str) -> Result<()> {
    let mut cfg = config::load_alerts().unwrap_or(AlertConfig { provider: vec![] });
    cfg.provider.push(provider);
    config::save_alerts(&cfg).context("failed to save alert config")?;
    println!("Added {description} alert provider");
    Ok(())
}

pub fn add_telegram(token: String, chat_id: String) -> Result<()> {
    add_provider(AlertProvider::Telegram { token, chat_id }, "Telegram")
}

pub fn add_slack(webhook_url: String) -> Result<()> {
    add_provider(AlertProvider::Slack { webhook_url }, "Slack")
}

pub fn add_webhook(url: String) -> Result<()> {
    add_provider(AlertProvider::Webhook { url }, "webhook")
}

#[allow(clippy::unnecessary_wraps)]
pub fn list() -> Result<()> {
    let cfg = config::load_alerts().unwrap_or(AlertConfig { provider: vec![] });
    if cfg.provider.is_empty() {
        println!("No alert providers configured. Use 'kron alert add' to add one.");
        return Ok(());
    }
    println!("Configured alert providers:\n");
    for (i, provider) in cfg.provider.iter().enumerate() {
        match provider {
            AlertProvider::Telegram { chat_id, .. } => {
                println!("  {}. Telegram (chat_id: {chat_id})", i + 1);
            }
            AlertProvider::Slack { webhook_url } => {
                let masked = if webhook_url.len() > 30 {
                    format!("{}...", &webhook_url[..30])
                } else {
                    webhook_url.clone()
                };
                println!("  {}. Slack (webhook: {masked})", i + 1);
            }
            AlertProvider::Webhook { url } => {
                println!("  {}. Webhook ({url})", i + 1);
            }
        }
    }
    Ok(())
}

pub async fn test_alerts() -> Result<()> {
    let cfg = config::load_alerts().unwrap_or(AlertConfig { provider: vec![] });
    if cfg.provider.is_empty() {
        println!("No alert providers configured. Use 'kron alert add' to add one.");
        return Ok(());
    }
    println!(
        "Sending test notification to {} provider(s)...",
        cfg.provider.len()
    );
    let client = reqwest::Client::new();
    for provider in &cfg.provider {
        match kron_core::notify::send_alert(
            &client,
            provider,
            "kron test alert",
            "This is a test notification from kron.",
        )
        .await
        {
            Ok(()) => {
                let name = match provider {
                    AlertProvider::Telegram { .. } => "Telegram",
                    AlertProvider::Slack { .. } => "Slack",
                    AlertProvider::Webhook { .. } => "Webhook",
                };
                println!("  {name}: sent successfully");
            }
            Err(e) => {
                println!("  Failed: {e}");
            }
        }
    }
    Ok(())
}

pub fn remove(index: usize) -> Result<()> {
    let mut cfg = config::load_alerts().unwrap_or(AlertConfig { provider: vec![] });
    if index == 0 || index > cfg.provider.len() {
        anyhow::bail!("invalid provider index {index}. Use 'kron alert list' to see providers.");
    }
    let removed = cfg.provider.remove(index - 1);
    config::save_alerts(&cfg).context("failed to save alert config")?;
    let name = match removed {
        AlertProvider::Telegram { .. } => "Telegram",
        AlertProvider::Slack { .. } => "Slack",
        AlertProvider::Webhook { .. } => "Webhook",
    };
    println!("Removed {name} alert provider");
    Ok(())
}
