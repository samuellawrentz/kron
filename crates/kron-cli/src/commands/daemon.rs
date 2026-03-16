use anyhow::{Context, Result};
use std::process::Command as StdCommand;
use tokio_util::sync::CancellationToken;
use tracing::info;

use kron_core::{config, scheduler::Scheduler};
use kron_store::Store;

pub fn install() -> Result<()> {
    let exe = std::env::current_exe().context("failed to determine kron binary path")?;
    let exe_path = exe.display().to_string();

    #[cfg(target_os = "linux")]
    {
        install_systemd(&exe_path)
    }

    #[cfg(target_os = "macos")]
    {
        install_launchd(&exe_path)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!(
            "service installation is not supported on this platform. Run 'kron daemon start' manually."
        );
    }
}

pub fn uninstall() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        uninstall_systemd()
    }

    #[cfg(target_os = "macos")]
    {
        uninstall_launchd()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!(
            "service installation is not supported on this platform. Run 'kron daemon start' manually."
        );
    }
}

pub fn service_status() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        status_systemd()
    }

    #[cfg(target_os = "macos")]
    {
        status_launchd()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        anyhow::bail!(
            "service installation is not supported on this platform. Run 'kron daemon start' manually."
        );
    }
}

#[cfg(target_os = "linux")]
fn install_systemd(exe_path: &str) -> Result<()> {
    let service_dir = dirs::home_dir()
        .context("failed to determine home directory")?
        .join(".config/systemd/user");
    std::fs::create_dir_all(&service_dir).context("failed to create systemd user directory")?;

    let service_content = format!(
        "[Unit]\n\
         Description=kron - cron replacement with built-in observability\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe_path} daemon start --foreground\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    );

    let service_path = service_dir.join("kron.service");
    std::fs::write(&service_path, service_content).context("failed to write service file")?;

    run_command("systemctl", &["--user", "daemon-reload"])?;
    run_command("systemctl", &["--user", "enable", "kron"])?;
    run_command("systemctl", &["--user", "start", "kron"])?;

    println!("kron service installed and started.");
    println!("  Service file: {}", service_path.display());
    println!("  Check status: kron daemon status");
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_systemd() -> Result<()> {
    // Stop and disable — ignore errors if service isn't running
    let _ = run_command("systemctl", &["--user", "stop", "kron"]);
    let _ = run_command("systemctl", &["--user", "disable", "kron"]);

    let service_path = dirs::home_dir()
        .context("failed to determine home directory")?
        .join(".config/systemd/user/kron.service");

    if service_path.exists() {
        std::fs::remove_file(&service_path).context("failed to remove service file")?;
    }

    run_command("systemctl", &["--user", "daemon-reload"])?;

    println!("kron service uninstalled.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn status_systemd() -> Result<()> {
    let output = StdCommand::new("systemctl")
        .args(["--user", "status", "kron"])
        .output()
        .context("failed to run systemctl")?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_launchd(exe_path: &str) -> Result<()> {
    let launch_agents_dir = dirs::home_dir()
        .context("failed to determine home directory")?
        .join("Library/LaunchAgents");
    std::fs::create_dir_all(&launch_agents_dir)
        .context("failed to create LaunchAgents directory")?;

    let data_dir = kron_core::config::data_dir();
    let log_path = data_dir.join("daemon.log");

    let plist_content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
             <key>Label</key>\n\
             <string>com.kron.scheduler</string>\n\
             <key>ProgramArguments</key>\n\
             <array>\n\
                 <string>{exe_path}</string>\n\
                 <string>daemon</string>\n\
                 <string>start</string>\n\
                 <string>--foreground</string>\n\
             </array>\n\
             <key>RunAtLoad</key>\n\
             <true/>\n\
             <key>KeepAlive</key>\n\
             <true/>\n\
             <key>StandardOutPath</key>\n\
             <string>{log}</string>\n\
             <key>StandardErrorPath</key>\n\
             <string>{log}</string>\n\
         </dict>\n\
         </plist>\n",
        log = log_path.display()
    );

    let plist_path = launch_agents_dir.join("com.kron.scheduler.plist");
    std::fs::write(&plist_path, plist_content).context("failed to write plist file")?;

    run_command("launchctl", &["load", &plist_path.display().to_string()])?;

    println!("kron service installed and started.");
    println!("  Plist file: {}", plist_path.display());
    println!("  Check status: kron daemon status");
    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launchd() -> Result<()> {
    let plist_path = dirs::home_dir()
        .context("failed to determine home directory")?
        .join("Library/LaunchAgents/com.kron.scheduler.plist");

    if plist_path.exists() {
        let _ = run_command("launchctl", &["unload", &plist_path.display().to_string()]);
        std::fs::remove_file(&plist_path).context("failed to remove plist file")?;
    }

    println!("kron service uninstalled.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn status_launchd() -> Result<()> {
    let output = StdCommand::new("launchctl")
        .args(["list", "com.kron.scheduler"])
        .output()
        .context("failed to run launchctl")?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn run_command(program: &str, args: &[&str]) -> Result<()> {
    let output = StdCommand::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {program}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{program} {} failed: {stderr}", args.join(" "));
    }
    Ok(())
}

fn is_daemon_running() -> Option<u32> {
    let pid_path = kron_core::config::data_dir().join("daemon.pid");
    let pid_str = std::fs::read_to_string(&pid_path).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;
    #[cfg(target_os = "linux")]
    {
        let proc_path = format!("/proc/{pid}");
        if std::path::Path::new(&proc_path).exists() {
            return Some(pid);
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::time::SystemTime;
        let meta = std::fs::metadata(&pid_path).ok()?;
        let age = SystemTime::now()
            .duration_since(meta.modified().ok()?)
            .ok()?;
        if age.as_secs() < 3600 {
            Some(pid)
        } else {
            None
        }
    }
}

pub fn stop() -> Result<()> {
    let pid_path = kron_core::config::data_dir().join("daemon.pid");
    let pid_str =
        std::fs::read_to_string(&pid_path).context("no PID file found — is the daemon running?")?;
    let pid: u32 = pid_str.trim().parse().context("invalid PID file")?;

    let status = StdCommand::new("kill")
        .arg(pid.to_string())
        .status()
        .context("failed to send stop signal")?;
    if !status.success() {
        anyhow::bail!("failed to stop daemon (pid {pid}) — process may not be running");
    }

    let _ = std::fs::remove_file(&pid_path);

    println!("kron daemon stopped (pid {pid}).");
    Ok(())
}

pub async fn execute(foreground: bool) -> Result<()> {
    if !foreground {
        // Re-launch as a background process
        let exe = std::env::current_exe().context("failed to determine kron binary path")?;

        // Ensure data directory exists
        let data_dir = kron_core::config::data_dir();
        std::fs::create_dir_all(&data_dir).context("failed to create data directory")?;

        // Refuse to start if daemon is already running
        if let Some(pid) = is_daemon_running() {
            anyhow::bail!(
                "kron daemon is already running (pid {pid}). Stop it first with 'kron daemon stop'."
            );
        }

        // Open a log file for daemon output
        let log_path = data_dir.join("daemon.log");
        let log_file =
            std::fs::File::create(&log_path).context("failed to create daemon log file")?;
        let stderr_file = log_file
            .try_clone()
            .context("failed to clone log file handle")?;

        let child = StdCommand::new(exe)
            .args(["daemon", "start", "--foreground"])
            .stdout(log_file)
            .stderr(stderr_file)
            .stdin(std::process::Stdio::null())
            .spawn()
            .context("failed to start daemon process")?;

        let pid = child.id();

        // Save PID file for later use
        let pid_path = data_dir.join("daemon.pid");
        std::fs::write(&pid_path, pid.to_string()).context("failed to write PID file")?;

        println!("kron daemon started (pid {pid})");
        println!("  Logs: {}", log_path.display());
        println!("  PID file: {}", pid_path.display());
        println!("  Stop with: kill {pid}");

        return Ok(());
    }

    // Foreground mode
    let store = Store::open(&config::db_path()).context("failed to open database")?;
    let cancel = CancellationToken::new();
    let scheduler = Scheduler::new(store, cancel.clone());
    let reload_signal = scheduler.reload_handle();

    // Handle SIGINT (Ctrl+C), SIGTERM (stop), and SIGHUP (reload)
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        #[allow(clippy::expect_used)]
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        #[allow(clippy::expect_used)]
        let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            .expect("failed to register SIGHUP handler");

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("received SIGINT, shutting down");
                    cancel_clone.cancel();
                    return;
                },
                _ = sigterm.recv() => {
                    info!("received SIGTERM, shutting down");
                    cancel_clone.cancel();
                    return;
                },
                _ = sighup.recv() => {
                    info!("received SIGHUP, triggering config reload");
                    reload_signal.notify_one();
                },
            }
        }
    });

    println!("kron daemon started. Press Ctrl+C to stop.");
    scheduler.run().await.context("scheduler error")?;
    println!("kron daemon stopped.");

    Ok(())
}
