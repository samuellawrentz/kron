use anyhow::Result;
use clap::Subcommand;

mod add;
mod alert;
mod daemon;
mod history;
mod import;
mod list;
mod logs;
mod remove;
mod run_job;
mod status;
mod test_job;
mod update;

#[derive(Subcommand)]
pub enum AlertCommand {
    /// Add a Telegram alert provider
    AddTelegram {
        /// Bot token
        #[arg(long)]
        token: String,
        /// Chat ID
        #[arg(long)]
        chat_id: String,
    },
    /// Add a Slack alert provider
    AddSlack {
        /// Webhook URL
        #[arg(long)]
        webhook_url: String,
    },
    /// Add a webhook alert provider
    AddWebhook {
        /// Webhook URL
        #[arg(long)]
        url: String,
    },
    /// List configured alert providers
    List,
    /// Send a test notification to all providers
    Test,
    /// Remove an alert provider by index
    Remove {
        /// Provider index (from 'kron alert list')
        index: usize,
    },
}

#[derive(Subcommand)]
pub enum Command {
    /// Manage alert providers
    #[command(subcommand)]
    Alert(AlertCommand),
    /// Add a new scheduled job
    Add {
        /// Schedule (cron expression or human-readable like "every day at 2am")
        schedule: String,
        /// Command to execute
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
        /// Optional human-friendly job name
        #[arg(short, long)]
        name: Option<String>,
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
        /// Capture current environment variables
        #[arg(long)]
        capture_env: bool,
    },
    /// List all jobs
    List,
    /// Show job status summary
    Status,
    /// Show run history for a job
    History {
        /// Job ID or name (shows all jobs if omitted)
        job: Option<String>,
        /// Number of runs to show
        #[arg(short = 'n', long, default_value = "10")]
        count: usize,
    },
    /// Show logs from a job run
    Logs {
        /// Job ID or name (defaults to most recent run across all jobs)
        job: Option<String>,
        /// Run number (1 = most recent)
        #[arg(long, default_value = "1")]
        run: usize,
    },
    /// Force-run a job immediately
    Run {
        /// Job ID or name
        job: String,
    },
    /// Dry-run a job (execute without recording)
    Test {
        /// Job ID or name
        job: String,
    },
    /// Remove a job
    Remove {
        /// Job ID or name
        job: String,
    },
    /// Import jobs from system crontab
    Import {
        /// Import all entries without prompting for selection
        #[arg(long)]
        all: bool,
    },
    /// Manage the scheduler daemon
    #[command(subcommand)]
    Daemon(DaemonCommand),
    /// Update kron to the latest version
    Update,
}

#[derive(Subcommand)]
pub enum DaemonCommand {
    /// Start the daemon
    Start {
        /// Run in foreground
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Install as a system service (survives restarts)
    Install,
    /// Uninstall the system service
    Uninstall,
    /// Show service status
    Status,
}

pub async fn run(cmd: Command) -> Result<()> {
    match cmd {
        Command::Add {
            schedule,
            command,
            name,
            working_dir,
            capture_env,
        } => add::execute(schedule, command, name, working_dir, capture_env),
        Command::List => list::execute(),
        Command::Status => status::execute(),
        Command::History { job, count } => history::execute(job.as_deref(), count),
        Command::Logs { job, run } => logs::execute(job.as_deref(), run),
        Command::Run { job } => run_job::execute(&job).await,
        Command::Test { job } => test_job::execute(&job).await,
        Command::Remove { job } => remove::execute(&job),
        Command::Import { all } => import::execute(all),
        Command::Daemon(daemon_cmd) => match daemon_cmd {
            DaemonCommand::Start { foreground } => daemon::execute(foreground).await,
            DaemonCommand::Stop => daemon::stop(),
            DaemonCommand::Install => daemon::install(),
            DaemonCommand::Uninstall => daemon::uninstall(),
            DaemonCommand::Status => daemon::service_status(),
        },
        Command::Update => update::execute(),
        Command::Alert(alert_cmd) => match alert_cmd {
            AlertCommand::AddTelegram { token, chat_id } => alert::add_telegram(token, chat_id),
            AlertCommand::AddSlack { webhook_url } => alert::add_slack(webhook_url),
            AlertCommand::AddWebhook { url } => alert::add_webhook(url),
            AlertCommand::List => alert::list(),
            AlertCommand::Test => alert::test_alerts().await,
            AlertCommand::Remove { index } => alert::remove(index),
        },
    }
}
