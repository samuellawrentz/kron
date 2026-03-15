use anyhow::{Context, Result, bail};

pub fn execute() -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("Current version: v{current}");
    println!("Checking for updates...");

    let status = std::process::Command::new("cargo")
        .args(["install", "kron"])
        .status()
        .context("failed to run cargo install — is cargo available?")?;

    if status.success() {
        println!("kron updated successfully.");
    } else {
        bail!(
            "cargo install failed with exit code: {}",
            status
                .code()
                .map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    Ok(())
}
