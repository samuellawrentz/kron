use anyhow::Result;
use clap::Subcommand;

mod add;
mod daemon;
mod history;
mod list;
mod logs;
mod remove;
mod run_job;
mod status;

#[derive(Subcommand)]
pub enum Command {
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
    },
    /// List all jobs
    List,
    /// Show job status summary
    Status,
    /// Show run history for a job
    History {
        /// Job ID or name
        job: String,
        /// Number of runs to show
        #[arg(short = 'n', long, default_value = "10")]
        count: usize,
    },
    /// Show logs from a job run
    Logs {
        /// Job ID or name
        job: String,
        /// Run number (1 = most recent)
        #[arg(long, default_value = "1")]
        run: usize,
    },
    /// Force-run a job immediately
    Run {
        /// Job ID or name
        job: String,
    },
    /// Remove a job
    Remove {
        /// Job ID or name
        job: String,
    },
    /// Start the scheduler daemon
    Daemon {
        /// Run in foreground (default: background)
        #[arg(short, long)]
        foreground: bool,
    },
}

pub async fn run(cmd: Command) -> Result<()> {
    match cmd {
        Command::Add {
            schedule,
            command,
            name,
            working_dir,
        } => add::execute(schedule, command, name, working_dir),
        Command::List => list::execute(),
        Command::Status => status::execute(),
        Command::History { job, count } => history::execute(&job, count),
        Command::Logs { job, run } => logs::execute(&job, run),
        Command::Run { job } => run_job::execute(&job).await,
        Command::Remove { job } => remove::execute(&job),
        Command::Daemon { foreground } => daemon::execute(foreground).await,
    }
}
