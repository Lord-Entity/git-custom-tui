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
    #[arg(long, conflicts_with = "clear_config")]
    account: bool,
    #[arg(long)]
    clear_config: bool,
}

/// What the menu does after a finalize attempt.
enum Step {
    /// Stay in the menu, optionally explaining why the commit did not run.
    Stay(Option<String>),
    /// Leave the TUI and print these lines to the normal terminal.
    Finish(Vec<String>),
}

fn run() -> Result<Vec<String>> {
    let args = Args::parse();
    if args.clear_config {
        config::clear()?;
        return Ok(vec!["Saved configuration cleared.".into()]);
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
    let context = status_context(&branch, &status);
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
            "finalize" => {
                let account = config
                    .active_account
                    .as_deref()
                    .and_then(|label| config.accounts.iter().find(|a| a.label == label));
                match finalize(&values, &config, account)? {
                    Step::Stay(message) => notice = message,
                    Step::Finish(summary) => return Ok(summary),
                }
            }
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
        lines.push(screen::warning("Working tree changes"));
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

    // With no active account, both stay None and git uses its own config as before.
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
