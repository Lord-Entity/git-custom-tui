mod accounts;
mod config;
mod git;
mod gitconfig;
mod input;
mod mascot;
mod model;
#[path = "terminal.rs"]
mod screen;

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use mascot::Mood;
use model::{first_problem, initial_values, preview, Config, Values};
use std::io::{stdin, stdout, IsTerminal};

#[derive(Parser, Debug)]
#[command(name = "gct", version, about = "Fast Git commit TUI")]
struct Args {
    /// Open the saved-accounts screen
    #[arg(long, conflicts_with = "clear_config")]
    account: bool,
    /// Delete the saved configuration file
    #[arg(long)]
    clear_config: bool,
    /// Print a usage guide for AI agents and exit
    #[arg(long)]
    agent: bool,
    /// Commit date YYYY-MM-DD (YYYY:MM:DD also works); default today. Any of -d/-t/-b/-m/-ml skips the TUI
    #[arg(short = 'd', long)]
    date: Option<String>,
    /// Commit time HH:MM:SS; default now
    #[arg(short = 't', long)]
    time: Option<String>,
    /// Branch to push to; default the current branch
    #[arg(short = 'b', long)]
    branch: Option<String>,
    /// One-line commit message (required without the TUI)
    #[arg(short = 'm', long)]
    message: Option<String>,
    /// Description as bullets: -ml "- first item - second item"
    #[arg(long = "ml", allow_hyphen_values = true)]
    ml: Option<String>,
    /// Also push to origin/<branch> after committing (default: local commit only)
    #[arg(long)]
    push: bool,
}

const AGENT_GUIDE: &str = r#"gct - commit with a custom date/time. Run inside a git repo.
It runs `git add .` (stages everything) and commits. With --push it also pushes to origin/<branch>.

Command format:
  gct -d YYYY-MM-DD -t HH:MM:SS -b <branch> -m "<type>: <one-line summary>" -ml "- <item> - <item>" [--push]

Flags:
  -d   commit date YYYY-MM-DD (default: today)
  -t   commit time HH:MM:SS, 24h (default: now)
  -b   branch to push to (default: current branch)
  -m   one-line commit message (required)
  -ml  optional description; each item becomes a bullet
  --push  push after committing (without it the commit stays local)

Example:
  gct -d 2026-05-05 -t 15:45:15 -b main -m "feat: add auth" -ml "- integrate login - integrate register"
Produces the commit message:
  feat: add auth

  - integrate login
  - integrate register

Rules: quote -m and -ml values. In -ml, separate items with " - " (space, dash, space).
Text inside an item may then contain flags like -d or --agent and words like log-in.
Short form "-one -two" also works, but splits at every " -", so avoid it when items mention flags.
Without any of these flags, gct opens its TUI."#;

/// What the menu does after a finalize attempt.
enum Step {
    /// Stay in the menu, optionally explaining why the commit did not run.
    Stay(Option<String>),
    /// Leave the TUI and print these lines to the normal terminal.
    Finish(Vec<String>),
}

fn run() -> Result<Vec<String>> {
    // clap short flags are one letter, so `-ml` would parse as `-m l`; rewrite it to `--ml`.
    let args =
        Args::parse_from(
            std::env::args().map(|arg| if arg == "-ml" { "--ml".into() } else { arg }),
        );
    if args.agent {
        return Ok(vec![AGENT_GUIDE.into()]);
    }
    if args.clear_config {
        config::clear()?;
        return Ok(vec!["Saved configuration cleared.".into()]);
    }
    if args.date.is_some()
        || args.time.is_some()
        || args.branch.is_some()
        || args.message.is_some()
        || args.ml.is_some()
        || args.push
    {
        return direct_commit(args);
    }
    if !stdin().is_terminal() || !stdout().is_terminal() {
        bail!("gct needs an interactive terminal; run it directly in a shell");
    }
    if args.account {
        let _terminal = screen::Guard::enter()?;
        let mut config = config::load()?;
        // Save before propagating any error, so a Ctrl+C after a switch keeps the snapshot.
        let result = input::accounts_screen(&mut config);
        config::save(&config)?;
        result?;
        return Ok(vec![]);
    }

    let branch = git::current_branch().map_err(|_| anyhow!("Not a git repository"))?;
    if branch == "HEAD" {
        bail!("HEAD is detached; check out a branch before committing");
    }
    let status = git::status_lines().map_err(|_| anyhow!("Not a git repository"))?;
    let mut config = config::load()?;
    let mut values = initial_values(config.clone(), &branch);
    let mut context = status_context(&branch, &status);
    let _terminal = screen::Guard::enter()?;

    let mut index = 0;
    let mut notice = None;
    loop {
        screen::set_mood(if notice.is_some() {
            Mood::Error
        } else {
            Mood::Idle
        });
        let active = config
            .active_account
            .clone()
            .unwrap_or_else(|| "git default".into());
        let choice = input::menu(
            &context,
            &values,
            notice.take().as_deref(),
            &mut index,
            &active,
        )?;
        match choice.as_str() {
            "account" => {
                match input::pick_account(&config.accounts, config.active_account.as_deref())? {
                    Some(input::Pick::Account(label)) => {
                        if let Err(error) = accounts::activate(&mut config, &label) {
                            notice = Some(error.to_string());
                        } else {
                            config::save(&config)?;
                        }
                    }
                    Some(input::Pick::GitDefault) => {
                        accounts::deactivate(&mut config)?;
                        config::save(&config)?;
                    }
                    None => {}
                }
            }
            "date" => values.date = input::masked("Commit date (YYYY-MM-DD):", &values.date, true)?,
            "time" => values.time = input::masked("Commit time (HH:MM:SS):", &values.time, false)?,
            "branch" => values.branch = input::read_line("Branch to push:", &values.branch, false)?,
            "message" => {
                values.message = input::read_line("Commit message:", &values.message, true)?
            }
            "editor" => match input::external_editor(&values.message) {
                Ok(message) => values.message = message,
                Err(error) if input::is_cancel(&error) => {}
                Err(error) => notice = Some(error.to_string()),
            },
            "options" => input::options(&mut values)?,
            "history" => input::history()?,
            "tree" => {
                input::working_tree()?;
                // Files may have changed since startup, so refresh the header too.
                if let Ok(status) = git::status_lines() {
                    context = status_context(&branch, &status);
                }
            }
            "finalize" => match finalize(&values, &config, active_account(&config))? {
                Step::Stay(message) => notice = message,
                Step::Finish(summary) => return Ok(summary),
            },
            _ => unreachable!(),
        }
    }
}

fn status_context(branch: &str, status: &[String]) -> Vec<String> {
    let mut lines = vec![format!(
        "{} {}",
        screen::muted("Branch"),
        screen::accent(branch)
    )];
    let identity = gitconfig::read();
    let who = match (identity.name.as_deref(), identity.email.as_deref()) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (Some(one), None) | (None, Some(one)) => one.to_string(),
        (None, None) => "not set".to_string(),
    };
    lines.push(format!("{} {}", screen::muted("Identity"), who));
    if status == ["(clean working tree)"] {
        lines.push(format!(
            "{} {}",
            screen::muted("Working tree"),
            screen::success("clean")
        ));
    } else {
        lines.push(format!(
            "{} {}",
            screen::warning("Working tree changes"),
            screen::muted("(click to view all)")
        ));
        lines.extend(status.iter().take(3).map(|line| format!("  {line}")));
        if status.len() > 3 {
            lines.push(screen::muted(format!("  ...and {} more", status.len() - 3)));
        }
    }
    lines
}

/// Shows a progress frame for a blocking git step.
fn working(message: impl AsRef<str>) -> Result<()> {
    screen::set_mood(Mood::Thinking);
    screen::draw(&[screen::accent(message)], "Please wait...", None)
}

fn active_account(config: &Config) -> Option<&accounts::Account> {
    config
        .active_account
        .as_deref()
        .and_then(|label| config.accounts.iter().find(|a| a.label == label))
}

/// With no active account, both stay None and git uses its own config as before.
fn identity_and_login(
    account: Option<&accounts::Account>,
) -> (Option<git::Identity>, Option<&str>) {
    let identity = account.map(|a| git::Identity {
        name: if a.name.trim().is_empty() {
            a.github_username.clone()
        } else {
            a.name.clone()
        },
        email: a.email.clone(),
    });
    let login = account
        .map(|a| a.github_username.as_str())
        .filter(|login| !login.is_empty());
    (identity, login)
}

/// Commits from flags without the TUI, so scripts and agents can run gct.
fn direct_commit(args: Args) -> Result<Vec<String>> {
    let branch = git::current_branch().map_err(|_| anyhow!("Not a git repository"))?;
    // A detached commit would not be on the branch that gets pushed, same rule as the TUI.
    if branch == "HEAD" {
        bail!("HEAD is detached; check out a branch before committing");
    }
    let now = chrono::Local::now();
    let values = Values {
        date: args.date.map_or_else(
            || now.format("%Y-%m-%d").to_string(),
            |d| d.replace(':', "-"),
        ),
        time: args
            .time
            .unwrap_or_else(|| now.format("%H:%M:%S").to_string()),
        branch: args.branch.unwrap_or(branch),
        message: match (args.message, args.ml) {
            (Some(m), Some(list)) => model::with_bullets(&m, &list),
            (Some(m), None) => m.trim().into(),
            (None, _) => bail!("-m <message> is required (run `gct --agent` for the format)"),
        },
        amend: false,
        dry_run: false,
    };
    if let Some(problem) = first_problem(&values) {
        bail!("{problem}");
    }
    let config = config::load()?;
    let account = active_account(&config);
    // Check the push login before committing, so a wrong token never leaves a commit behind.
    if let (Some(account), true) = (account, args.push) {
        accounts::verify_login(&account.github_username).context("Not committed")?;
    }
    let (identity, login) = identity_and_login(account);
    let date = format!("{} {}", values.date, values.time);

    git::run(&["add", "."]).context("Staging all files failed")?;
    git::commit(&values.message, &date, false, identity.as_ref())?;
    config::save(&Config {
        last_date: date.clone(),
        last_branch: values.branch.clone(),
        last_message: values.message.clone(),
        ..config.clone()
    })?;
    if !args.push {
        return Ok(vec![format!(
            "Done. commit ({date}) -> local only (add --push to push)"
        )]);
    }
    git::push(&values.branch, false, login).context("Committed locally, but push failed")?;
    Ok(vec![format!(
        "Done. commit ({date}) -> push origin/{}",
        values.branch
    )])
}

fn finalize(values: &Values, config: &Config, account: Option<&accounts::Account>) -> Result<Step> {
    if let Some(problem) = first_problem(values) {
        return Ok(Step::Stay(Some(problem.into())));
    }
    // Check the push login before committing, so a wrong token never leaves a commit behind.
    if let Some(account) = account {
        if let Err(error) = accounts::verify_login(&account.github_username) {
            return Ok(Step::Stay(Some(format!("Not committed: {error}"))));
        }
    }

    let (identity, login) = identity_and_login(account);

    let date = format!("{} {}", values.date, values.time);
    let mut summary = vec![
        screen::accent("Final summary"),
        format!("  Date:   {date}"),
        format!("  Branch: {}", values.branch),
        format!("  Msg:    {}", preview(&values.message, 60)),
    ];
    if values.amend {
        summary.push("  Mode:   amend last commit".into());
        if let Some(id) = &identity {
            summary.push(format!(
                "  Mode:   amend keeps the original author; committer is {}",
                id.name
            ));
        }
    }
    if values.dry_run {
        summary.push("  Mode:   dry run".into());
    }
    if identity.is_some() {
        let mut keys = vec!["user.name", "user.email"];
        if login.is_some() {
            keys.push("credential.https://github.com.username");
        }
        for key in keys {
            if git::has_local_config(key) {
                summary.push(screen::warning(format!(
                    "This repo sets its own {key}; gct overrides it for this commit."
                )));
            }
        }
    }
    if !input::confirm(&summary, "Proceed with these actions?", true)? {
        return Ok(Step::Stay(None));
    }

    if values.dry_run {
        let command = if values.amend {
            "commit --amend"
        } else {
            "commit"
        };
        return Ok(Step::Finish(vec![
            "Dry run - commands that would be executed:".into(),
            "  git add .".into(),
            format!(
                "  git {command} -m {:?} (author/committer date: {date})",
                values.message
            ),
            format!("  git push origin {}", values.branch),
        ]));
    }

    working("Staging all files...")?;
    git::run(&["add", "."]).context("Staging all files failed")?;
    working("Creating commit...")?;
    git::commit(&values.message, &date, values.amend, identity.as_ref())?;
    config::save(&Config {
        last_date: date.clone(),
        last_branch: values.branch.clone(),
        last_message: values.message.clone(),
        ..config.clone()
    })?;

    screen::set_mood(Mood::Happy);
    // Ctrl+C at the push question means "keep the local commit", not an error.
    let push = match input::confirm(
        &[screen::success("Commit created.")],
        "Push to GitHub?",
        true,
    ) {
        Ok(answer) => answer,
        Err(error) if input::is_cancel(&error) => false,
        Err(error) => return Err(error),
    };
    let outcome = if push {
        working(format!("Pushing to origin/{}...", values.branch))?;
        git::push(&values.branch, values.amend, login)?;
        format!("Done. commit -> push origin/{}", values.branch)
    } else {
        "Done. commit -> local only (no push)".to_string()
    };
    screen::set_mood(Mood::Happy);
    input::wait_key(&[screen::success(&outcome)], "Press any key to exit")?;
    Ok(Step::Finish(vec![outcome]))
}

fn main() {
    // Restore the terminal before the panic message prints, so it is not lost on the alternate screen.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        screen::restore();
        default_hook(info);
    }));

    match run() {
        Ok(summary) => {
            for line in summary {
                println!("{line}");
            }
        }
        Err(error) if input::is_cancel(&error) => {
            println!("Cancelled.");
            std::process::exit(130);
        }
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
    }
}
