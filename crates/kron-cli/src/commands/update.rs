use anyhow::{Context, Result, bail};

const INSTALL_SCRIPT_URL: &str =
    "https://raw.githubusercontent.com/samuellawrentz/kron/main/install.sh";

pub fn execute() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("Current version: v{current}");
    println!("Updating kron...");

    // Use the install script which downloads the latest release binary
    let shell = if cfg!(target_os = "windows") {
        bail!("self-update is not supported on Windows — download from GitHub releases");
    } else {
        "sh"
    };

    let status = std::process::Command::new(shell)
        .args(["-c", &format!("curl -fsSL {INSTALL_SCRIPT_URL} | sh")])
        .status()
        .context("failed to run install script — is curl available?")?;

    if status.success() {
        println!("kron updated successfully.");
    } else {
        bail!(
            "update failed with exit code: {}",
            status
                .code()
                .map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    Ok(())
}
