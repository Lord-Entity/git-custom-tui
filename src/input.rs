use anyhow::{anyhow, bail, Context, Result};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::{
    fs,
    process::{Command, Stdio},
};

use crate::{
    accounts::{self, Account},
    git, gitconfig,
    model::{current_year, preview, valid_date, valid_time, Config, Values},
    screen,
};

/// Error text for every user cancel, so `main` can tell a cancel from a failure.
pub const CANCELLED: &str = "Cancelled";

const MENU_HINT: &str =
    "Up/Down or W/S move  ·  Enter or click select  ·  Esc/Q quit  ·  Ctrl+C quit";
const TREE_HINT: &str =
    "Up/Down or W/S scroll  ·  PgUp/PgDn  ·  Home/End  ·  Esc/Q back  ·  Ctrl+C quit";
const CONFIRM_HINT: &str = "Y yes  ·  N no  ·  Enter default  ·  Ctrl+C quit";
const LINE_HINT: &str = "Enter save  ·  Esc cancel  ·  Ctrl+C quit";
const MESSAGE_HINT: &str = "Enter new line  ·  Ctrl+S save  ·  Esc cancel  ·  Ctrl+C quit";
const MASK_HINT: &str =
    "Digits type  ·  Left/Right move  ·  Backspace clear  ·  Enter save  ·  Esc cancel";
const HISTORY_HINT: &str =
    "Tab switch  ·  Up/Down or W/S scroll  ·  PgUp/PgDn  ·  Home/End  ·  Esc/Q back  ·  Ctrl+C quit";
const PICK_HINT: &str = "Up/Down or W/S move  ·  Enter select  ·  Esc/Q cancel  ·  Ctrl+C quit";
const ACCOUNTS_HINT: &str = "Up/Down or W/S move  ·  Enter/Space set active  ·  A add  ·  E edit  ·  D delete  ·  L sign in  ·  R refresh  ·  Esc/Q back  ·  Ctrl+C quit";

enum Input {
    Key(KeyEvent),
    /// Left click on this body line.
    Click(usize),
}

/// Next key press or body click, or None when the screen should redraw (resize, or the pointer moved the mascot).
fn read_input() -> Result<Option<Input>> {
    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => return Ok(Some(Input::Key(key))),
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(line) = screen::body_line(mouse.row) {
                    return Ok(Some(Input::Click(line)));
                }
            }
            Event::Mouse(mouse) if screen::track_pointer(mouse.column) => return Ok(None),
            Event::Resize(..) => return Ok(None),
            _ => {}
        }
    }
}

/// Like `read_input`, for screens that ignore clicks.
fn read_key() -> Result<Option<KeyEvent>> {
    loop {
        match read_input()? {
            Some(Input::Key(key)) => return Ok(Some(key)),
            Some(Input::Click(_)) => {}
            None => return Ok(None),
        }
    }
}

fn is_interrupt(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub fn is_cancel(error: &anyhow::Error) -> bool {
    error.to_string() == CANCELLED
}

/// Shows a frame until a key is pressed; the frame redraws when the mascot moves.
pub fn wait_key(body: &[String], footer: &str) -> Result<()> {
    loop {
        screen::draw(body, footer, None)?;
        if read_key()?.is_some() {
            return Ok(());
        }
    }
}

pub fn menu(
    context: &[String],
    values: &Values,
    notice: Option<&str>,
    index: &mut usize,
    active: &str,
) -> Result<String> {
    let mode = match (values.amend, values.dry_run) {
        (true, true) => "amend, dry-run",
        (true, false) => "amend",
        (false, true) => "dry-run",
        _ => "normal",
    };
    let message = if values.message.is_empty() {
        "[not set]".into()
    } else {
        preview(&values.message, 40)
    };
    let choices = [
        format!(
            "Edit date     : {}",
            if values.date.is_empty() {
                "[not set]"
            } else {
                &values.date
            }
        ),
        format!(
            "Edit time     : {}",
            if values.time.is_empty() {
                "[not set]"
            } else {
                &values.time
            }
        ),
        format!(
            "Edit branch   : {}",
            if values.branch.is_empty() {
                "[not set]"
            } else {
                &values.branch
            }
        ),
        format!("Edit message  : {message}"),
        format!("Edit msg (editor): {message}"),
        format!("Edit options  : {mode}"),
        "View history".into(),
        "View working tree".into(),
        format!("Account: {active}"),
        "------------".into(),
        "Finalize & commit".into(),
    ];
    let ready = [
        !values.date.is_empty(),
        !values.time.is_empty(),
        !values.branch.is_empty(),
        !values.message.trim().is_empty(),
    ]
    .into_iter()
    .filter(|ready| *ready)
    .count();
    loop {
        let mut body = context.to_vec();
        body.push(String::new());
        body.push(screen::accent(format!(
            "MISSION CONTROL  [{ready}/4 FIELDS READY]"
        )));
        for (position, choice) in choices.iter().enumerate() {
            body.push(if position == SEPARATOR {
                screen::muted("  ------------")
            } else {
                styled_menu_line(position == *index, choice)
            });
        }
        if let Some(notice) = notice {
            body.push(String::new());
            body.push(screen::danger(notice));
        }
        screen::draw(&body, MENU_HINT, None)?;

        let key = match read_input()? {
            None => continue,
            Some(Input::Key(key)) => key,
            // Context lines 0-1 are branch and identity; the working-tree lines follow.
            Some(Input::Click(line)) if (2..context.len()).contains(&line) => {
                return Ok("tree".into())
            }
            Some(Input::Click(line)) => {
                // Choices start after the context, a blank line and the title.
                match line.checked_sub(context.len() + 2) {
                    Some(position) if position < choices.len() && position != SEPARATOR => {
                        *index = position;
                        return Ok(choice_name(position).into());
                    }
                    _ => continue,
                }
            }
        };
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('w' | 'W') => *index = previous_choice(*index),
            KeyCode::Down | KeyCode::Char('s' | 'S') => *index = next_choice(*index),
            KeyCode::Enter | KeyCode::Char(' ') => return Ok(choice_name(*index).into()),
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => bail!(CANCELLED),
            _ => {}
        }
    }
}

const SEPARATOR: usize = 9;

fn choice_name(index: usize) -> &'static str {
    match index {
        0 => "date",
        1 => "time",
        2 => "branch",
        3 => "message",
        4 => "editor",
        5 => "options",
        6 => "history",
        7 => "tree",
        8 => "account",
        _ => "finalize",
    }
}

fn styled_menu_line(active: bool, text: &str) -> String {
    if active {
        screen::selected(format!("  ▶ {text}"))
    } else {
        format!("  {}", screen::muted(format!("◇ {text}")))
    }
}

/// Menu rows 0-5 are the fields, 6 History, 7 Working tree, 8 Account, 9 a separator, 10 Finalize.
fn previous_choice(index: usize) -> usize {
    match index {
        0 => 10,
        9 | 10 => 8,
        _ => index - 1,
    }
}

fn next_choice(index: usize) -> usize {
    match index {
        8 => 10,
        10 => 0,
        _ => index + 1,
    }
}

/// Full `git status --short`, read fresh, scrollable. Load errors show as text.
pub fn working_tree() -> Result<()> {
    let rows = match git::status_lines() {
        Ok(lines) => lines,
        Err(error) => vec![screen::danger(format!("Could not read status: {error}"))],
    };
    let title = if rows == ["(clean working tree)"] {
        screen::success("Working tree clean")
    } else {
        screen::warning(format!(
            "Working tree: {} changes (all are staged by commit)",
            rows.len()
        ))
    };
    let mut offset = 0usize;
    loop {
        // The title sits above the list.
        let visible = screen::body_rows().saturating_sub(1);
        offset = offset.min(rows.len().saturating_sub(visible));
        let mut body = vec![title.clone()];
        body.extend(rows.iter().skip(offset).take(visible).cloned());
        screen::draw(&body, TREE_HINT, None)?;

        let Some(key) = read_key()? else { continue };
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('w' | 'W') => offset = offset.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('s' | 'S') => offset = offset.saturating_add(1),
            KeyCode::PageUp => offset = offset.saturating_sub(visible),
            KeyCode::PageDown => offset = offset.saturating_add(visible),
            KeyCode::Home => offset = 0,
            KeyCode::End => offset = usize::MAX,
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(()),
            _ => {}
        }
    }
}

/// Read-only history: recent commits and remote branches, one tab each. Load errors show as text.
pub fn history() -> Result<()> {
    let head = match git::current_branch() {
        Ok(name) if name == "HEAD" => "detached HEAD".to_string(),
        Ok(name) => match git::upstream(&name) {
            Ok(Some(upstream)) => format!("{name} -> {upstream}"),
            _ => format!("{name} (no upstream)"),
        },
        Err(error) => format!("Branch unknown: {error}"),
    };
    let commits: Vec<String> = match git::recent_commits(200) {
        Ok(list) if list.is_empty() => vec![screen::muted("No commits yet")],
        Ok(list) => list
            .iter()
            .map(|commit| {
                let refs = if commit.refs.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", commit.refs)
                };
                format!("{}  {}{refs}  {}", commit.hash, commit.date, commit.subject)
            })
            .collect(),
        Err(error) => vec![screen::danger(format!("Could not read commits: {error}"))],
    };
    let branches: Vec<String> = match git::remote_branches() {
        Ok(list) if list.is_empty() => vec![screen::muted("No remote branches")],
        Ok(list) => list,
        Err(error) => vec![screen::danger(format!("Could not read branches: {error}"))],
    };
    let lists = [commits, branches];
    let mut tab = 0usize;
    let mut offset = 0usize;
    loop {
        // The branch line and the tab bar sit above the list.
        let visible = screen::body_rows().saturating_sub(2);
        let rows = &lists[tab];
        offset = offset.min(rows.len().saturating_sub(visible));
        let tabs: Vec<String> = ["Recent commits", "Remote branches"]
            .iter()
            .enumerate()
            .map(|(index, name)| {
                if index == tab {
                    screen::selected(format!("[{name}]"))
                } else {
                    screen::muted(name)
                }
            })
            .collect();
        let mut body = vec![screen::accent(&head), tabs.join("  ")];
        body.extend(rows.iter().skip(offset).take(visible).cloned());
        screen::draw(&body, HISTORY_HINT, None)?;

        let Some(key) = read_key()? else { continue };
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        match key.code {
            KeyCode::Tab => {
                tab = 1 - tab;
                offset = 0;
            }
            KeyCode::Up | KeyCode::Char('w' | 'W') => offset = offset.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('s' | 'S') => offset = offset.saturating_add(1),
            KeyCode::PageUp => offset = offset.saturating_sub(visible),
            KeyCode::PageDown => offset = offset.saturating_add(visible),
            KeyCode::Home => offset = 0,
            KeyCode::End => offset = usize::MAX,
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(()),
            _ => {}
        }
    }
}

pub fn read_line(prompt: &str, initial: &str, multiline: bool) -> Result<String> {
    text_input(prompt, initial, multiline, false)
}

/// `read_line` that can also be saved empty when `optional` is set.
fn text_input(prompt: &str, initial: &str, multiline: bool, optional: bool) -> Result<String> {
    let mut chars: Vec<char> = initial.chars().collect();
    let mut cursor = chars.len();
    let mut notice: Option<&str> = None;
    let hint = if multiline { MESSAGE_HINT } else { LINE_HINT };
    loop {
        let text: String = chars.iter().collect();
        let before: String = chars[..cursor].iter().collect();
        // Body row 0 is the prompt, so text starts on row 1.
        let row = (1 + before.matches('\n').count()) as u16;
        let column = before
            .rsplit('\n')
            .next()
            .map_or(0, |line| line.chars().count()) as u16;

        let mut body = vec![prompt.to_string()];
        body.extend(text.split('\n').map(str::to_owned));
        if let Some(message) = notice {
            body.push(String::new());
            body.push(screen::danger(message));
        }
        screen::draw(&body, hint, Some((row, column)))?;

        let Some(key) = read_key()? else { continue };
        notice = None;
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let submit = match key.code {
            KeyCode::Char('s') if ctrl => true,
            KeyCode::Enter => ctrl || !multiline,
            _ => false,
        };
        if submit {
            let value = text.trim();
            if value.is_empty() && !optional {
                notice = Some(if multiline {
                    "Message is required"
                } else {
                    "Value is required"
                });
                continue;
            }
            return Ok(value.to_string());
        }
        match key.code {
            KeyCode::Esc => return Ok(initial.into()),
            KeyCode::Enter => {
                chars.insert(cursor, '\n');
                cursor += 1;
            }
            KeyCode::Backspace if cursor > 0 => {
                cursor -= 1;
                chars.remove(cursor);
            }
            KeyCode::Delete if cursor < chars.len() => {
                chars.remove(cursor);
            }
            KeyCode::Left => cursor = cursor.saturating_sub(1),
            KeyCode::Right => cursor = (cursor + 1).min(chars.len()),
            KeyCode::Home => cursor = 0,
            KeyCode::End => cursor = chars.len(),
            KeyCode::Char(ch) if !ctrl => {
                chars.insert(cursor, ch);
                cursor += 1;
            }
            _ => {}
        }
    }
}

pub fn masked(prompt: &str, initial: &str, date: bool) -> Result<String> {
    let template = if date { "YYYY-MM-DD" } else { "hh:mm:ss" };
    let fallback = if date {
        format!("{}-__-__", current_year())
    } else {
        "00:__:__".into()
    };
    let initial_is_valid = (date && valid_date(initial)) || (!date && valid_time(initial));
    let mut chars: Vec<char> = if initial_is_valid {
        initial.chars().collect()
    } else {
        fallback.chars().collect()
    };
    let slots: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            (*value == '_'
                || template.chars().nth(index).is_some_and(|token| {
                    token == 'Y'
                        || token == 'M'
                        || token == 'D'
                        || token == 'h'
                        || token == 'm'
                        || token == 's'
                }))
            .then_some(index)
        })
        .collect();
    let mut slot = 0usize;
    let mut notice: Option<&str> = None;
    loop {
        let rendered: String = chars
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if slots[slot] == index {
                    format!("\x1b[7;36m{value}\x1b[0m")
                } else {
                    value.to_string()
                }
            })
            .collect();
        let column = prompt.chars().count() as u16 + 1 + slots[slot] as u16;
        let mut body = vec![format!("{prompt} {rendered}")];
        if let Some(message) = notice {
            body.push(String::new());
            body.push(screen::danger(message));
        }
        screen::draw(&body, MASK_HINT, Some((0, column)))?;

        let Some(key) = read_key()? else { continue };
        notice = None;
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        match key.code {
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                chars[slots[slot]] = ch;
                if slot + 1 < slots.len() {
                    slot += 1;
                }
            }
            KeyCode::Left => slot = slot.saturating_sub(1),
            KeyCode::Right => slot = (slot + 1).min(slots.len() - 1),
            KeyCode::Home => slot = 0,
            KeyCode::End => slot = slots.len() - 1,
            KeyCode::Backspace => {
                chars[slots[slot]] = '_';
                slot = slot.saturating_sub(1);
            }
            KeyCode::Delete => chars[slots[slot]] = '_',
            KeyCode::Esc => return Ok(initial.into()),
            KeyCode::Enter => {
                let result: String = chars.iter().collect();
                if result.contains('_') {
                    notice = Some("Complete all fields before continuing");
                } else if (date && !valid_date(&result)) || (!date && !valid_time(&result)) {
                    notice = Some(if date {
                        "Not a valid calendar date"
                    } else {
                        "Time out of range (00:00:00 - 23:59:59)"
                    });
                } else {
                    return Ok(result);
                }
            }
            _ => {}
        }
    }
}

pub fn external_editor(initial: &str) -> Result<String> {
    let path = std::env::temp_dir().join(format!("git-cc-{}.txt", std::process::id()));
    fs::write(&path, initial)?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "vi".into()
            }
        });
    let mut parts = editor.split_whitespace();
    let program = parts.next().context("editor command is empty")?;
    // The editor needs the normal terminal, so leave the TUI while it runs.
    screen::suspend();
    let status = Command::new(program)
        .args(parts)
        .arg(&path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    let resumed = screen::resume();
    let result = match status {
        Ok(status) if status.success() => {
            fs::read_to_string(&path).context("could not read editor output")
        }
        Ok(status) => Err(anyhow!("editor exited with status {status}")),
        Err(error) => Err(error.into()),
    };
    let _ = fs::remove_file(&path);
    resumed?;
    let message = result?;
    if message.trim().is_empty() {
        bail!("Commit message is required");
    }
    Ok(message.trim().into())
}

pub fn options(values: &mut Values) -> Result<()> {
    values.amend = confirm(
        &[],
        "Amend the last commit instead of creating a new one?",
        false,
    )?;
    values.dry_run = confirm(&[], "Preview only (dry run, do not commit/push)?", false)?;
    Ok(())
}

/// Shows `context` above the question. Ctrl+C cancels with `CANCELLED`.
pub fn confirm(context: &[String], prompt: &str, default: bool) -> Result<bool> {
    let mut body = context.to_vec();
    if !body.is_empty() {
        body.push(String::new());
    }
    body.push(format!(
        "{prompt} ({})",
        if default { "Y/n" } else { "y/N" }
    ));
    loop {
        screen::draw(&body, CONFIRM_HINT, None)?;
        let Some(key) = read_key()? else { continue };
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        match key.code {
            KeyCode::Enter => return Ok(default),
            KeyCode::Char('y' | 'Y') => return Ok(true),
            KeyCode::Char('n' | 'N') => return Ok(false),
            _ => {}
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Pick {
    Account(String),
    GitDefault,
}

/// Account chooser: "git default" first, then one row per account. Esc/Q gives None.
pub fn pick_account(accounts: &[Account], active: Option<&str>) -> Result<Option<Pick>> {
    let mut index = 0usize;
    let logins = accounts::stored_logins();
    loop {
        let mut body = vec![screen::accent("SELECT ACCOUNT"), String::new()];
        body.extend(account_rows(accounts, active, &logins, index));
        if accounts.is_empty() {
            body.push(String::new());
            body.push(screen::muted(
                "No accounts yet. Use gct --account to add one.",
            ));
        }
        screen::draw(&body, PICK_HINT, None)?;

        let Some(key) = read_key()? else { continue };
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        let count = accounts.len() + 1;
        match key.code {
            KeyCode::Up | KeyCode::Char('w' | 'W') => index = (index + count - 1) % count,
            KeyCode::Down | KeyCode::Char('s' | 'S') => index = (index + 1) % count,
            KeyCode::Enter | KeyCode::Char(' ') => {
                return Ok(Some(if index == 0 {
                    Pick::GitDefault
                } else {
                    Pick::Account(accounts[index - 1].label.clone())
                }))
            }
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(None),
            _ => {}
        }
    }
}

/// Account manager: set active, add, edit, refresh, sign in, delete. Failures show in the body instead of exiting.
pub fn accounts_screen(config: &mut Config) -> Result<()> {
    let mut index = 0usize;
    let mut notice: Option<String> = None;
    let mut logins = accounts::stored_logins();
    loop {
        // Deleting the last account can leave the index past the end.
        index = index.min(config.accounts.len());
        let mut body = vec![
            screen::accent("ACCOUNTS"),
            format!("Active in git: {}", identity_text(&gitconfig::read())),
            screen::muted("Press A to add a GitHub account; a browser window opens to sign in."),
            String::new(),
        ];
        if let Some(message) = &notice {
            body.push(screen::danger(message));
            body.push(String::new());
        }
        body.extend(account_rows(
            &config.accounts,
            config.active_account.as_deref(),
            &logins,
            index,
        ));
        if config.accounts.is_empty() {
            body.push(String::new());
            body.push(screen::muted(
                "No accounts yet. Press A, give it a label, and sign in with GitHub.",
            ));
        }
        screen::draw(&body, ACCOUNTS_HINT, None)?;

        let Some(key) = read_key()? else { continue };
        notice = None;
        if is_interrupt(&key) {
            bail!(CANCELLED);
        }
        let count = config.accounts.len() + 1;
        match key.code {
            KeyCode::Up | KeyCode::Char('w' | 'W') => index = (index + count - 1) % count,
            KeyCode::Down | KeyCode::Char('s' | 'S') => index = (index + 1) % count,
            KeyCode::Enter | KeyCode::Char(' ') => {
                let result = if index == 0 {
                    accounts::deactivate(config)
                } else {
                    let label = config.accounts[index - 1].label.clone();
                    accounts::activate(config, &label)
                };
                if let Err(error) = result {
                    notice = Some(error.to_string());
                }
            }
            KeyCode::Char('a' | 'A') => {
                notice = Some(match add_form(&config.accounts)? {
                    Some(label) => {
                        let mut username = String::new();
                        if ask("GitHub username:", &mut username)? {
                            match suspended(|| accounts::sign_in_new(&label, &username))? {
                                Ok(account) => {
                                    let message = format!(
                                        "Added {label}, signed in to GitHub as @{}",
                                        account.github_username
                                    );
                                    config.accounts.push(account);
                                    index = config.accounts.len();
                                    message
                                }
                                Err(error) => error.to_string(),
                            }
                        } else {
                            "Cancelled, no account saved".into()
                        }
                    }
                    None => "Cancelled, no account saved".into(),
                });
                logins = accounts::stored_logins();
            }
            KeyCode::Char('l' | 'L') if index > 0 => {
                notice = Some(
                    update_account(config, index - 1, true)
                        .unwrap_or_else(|error| error.to_string()),
                );
                logins = accounts::stored_logins();
            }
            KeyCode::Char('r' | 'R') if index > 0 => {
                notice = Some(
                    update_account(config, index - 1, false)
                        .unwrap_or_else(|error| error.to_string()),
                );
                logins = accounts::stored_logins();
            }
            KeyCode::Char('e' | 'E') if index > 0 => {
                notice = Some(match edit_form(&config.accounts, index - 1)? {
                    Some(edited) => {
                        let label = edited.label.clone();
                        let old = config.accounts[index - 1].clone();
                        let old_active = config.active_account.clone();
                        match replace_account(config, index - 1, edited) {
                            Ok(()) => format!("Saved {label}"),
                            Err(error) => {
                                // Activation failed, so the edit must not stay saved.
                                config.accounts[index - 1] = old;
                                config.active_account = old_active;
                                error.to_string()
                            }
                        }
                    }
                    None => "Cancelled, account unchanged".into(),
                });
            }
            KeyCode::Char('d' | 'D') if index > 0 => {
                let label = config.accounts[index - 1].label.clone();
                let prompt = format!(
                    "Delete account '{label}'? Its GitHub sign-in stays saved in Git Credential Manager."
                );
                if confirm(&[], &prompt, false)? {
                    let was_active = config.active_account.as_deref() == Some(label.as_str());
                    let deactivated = if was_active {
                        accounts::deactivate(config)
                    } else {
                        Ok(())
                    };
                    match deactivated {
                        Ok(()) => {
                            config.accounts.remove(index - 1);
                        }
                        Err(error) => notice = Some(error.to_string()),
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return Ok(()),
            _ => {}
        }
    }
}

/// Runs `work` with the TUI suspended, so the browser or Credential Manager can use the terminal.
/// The outer error is a failed resume; the inner one is `work`'s own result.
fn suspended<T>(work: impl FnOnce() -> Result<T>) -> Result<Result<T>> {
    screen::suspend();
    let result = work();
    screen::resume()?;
    Ok(result)
}

/// Re-fetches name and email from GitHub for the account at `position`, signing in first when asked.
fn update_account(config: &mut Config, position: usize, sign_in: bool) -> Result<String> {
    let account = config.accounts[position].clone();
    let username = account.github_username.clone();
    let fresh = suspended(|| {
        if sign_in {
            accounts::login(&username)?;
        }
        let mut fresh = account;
        accounts::refresh(&mut fresh)?;
        Ok(fresh)
    })??;
    let label = fresh.label.clone();
    replace_account(config, position, fresh)?;
    Ok(if sign_in {
        format!("Signed in to GitHub as @{username}, {label} updated")
    } else {
        format!("Refreshed {label} from GitHub")
    })
}

/// Stores `account` at `position`. When it is the active one, git is re-pointed at the new values.
fn replace_account(config: &mut Config, position: usize, account: Account) -> Result<()> {
    let was_active =
        config.active_account.as_deref() == Some(config.accounts[position].label.as_str());
    let label = account.label.clone();
    let old_label = config.accounts[position].label.clone();
    let old = std::mem::replace(&mut config.accounts[position], account);
    if was_active {
        config.active_account = Some(label.clone());
        if let Err(error) = accounts::activate(config, &label) {
            // The switch failed, so keep the old values rather than the unapplied new ones.
            config.accounts[position] = old;
            config.active_account = Some(old_label);
            return Err(error);
        }
    }
    Ok(())
}

/// "Name <email>", the email alone without a name, or "not set" when git has neither.
fn identity_text(identity: &gitconfig::Identity) -> String {
    match (identity.name.as_deref(), identity.email.as_deref()) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (Some(name), None) => name.into(),
        (None, Some(email)) => email.into(),
        (None, None) => "not set".into(),
    }
}

/// "git default" row then the accounts, with the active one marked and its sign-in state shown.
fn account_rows(
    accounts: &[Account],
    active: Option<&str>,
    logins: &[String],
    index: usize,
) -> Vec<String> {
    let mut rows = vec![styled_menu_line(
        index == 0,
        &marked("git default", active.is_none()),
    )];
    for (position, account) in accounts.iter().enumerate() {
        let is_active = active == Some(account.label.as_str());
        let signed_in = logins
            .iter()
            .any(|login| login.eq_ignore_ascii_case(&account.github_username));
        rows.push(styled_menu_line(
            index == position + 1,
            &account_line(account, is_active, signed_in),
        ));
    }
    rows
}

/// One account: label, display name (the email when no name is saved), email, @username, sign-in state.
fn account_line(account: &Account, active: bool, signed_in: bool) -> String {
    let mut parts = vec![format!("{:<10}", account.label)];
    if account.name.trim().is_empty() {
        parts.push(account.email.clone());
    } else {
        parts.push(account.name.clone());
        parts.push(format!("<{}>", account.email));
    }
    parts.push(format!("@{}", account.github_username));
    parts.push(
        if signed_in {
            "[signed in]"
        } else {
            "[not signed in]"
        }
        .into(),
    );
    if active {
        parts.push("(active)".into());
    }
    parts.join("  ")
}

fn marked(text: &str, active: bool) -> String {
    if active {
        format!("{text}  (active)")
    } else {
        text.into()
    }
}

/// Asks one value. Returns false when it came back empty, which only happens when Esc is pressed on an empty prompt.
fn ask(prompt: &str, value: &mut String) -> Result<bool> {
    let answer = read_line(prompt, value.as_str(), false)?;
    let filled = !answer.is_empty();
    *value = answer;
    Ok(filled)
}

/// Label for a new account, pre-filled with the next free "account N". None on cancel.
fn add_form(accounts: &[Account]) -> Result<Option<String>> {
    let mut label = (1..)
        .map(|number| format!("account {number}"))
        .find(|label| !accounts.iter().any(|account| account.label == *label))
        .unwrap_or_default();
    loop {
        if !ask("Label (short name, e.g. work):", &mut label)? {
            return Ok(None);
        }
        if accounts.iter().any(|account| account.label == label) {
            wait_key(
                &[screen::danger(format!("label {label} is already used"))],
                "Press any key to fix the label",
            )?;
            continue;
        }
        return Ok(Some(label));
    }
}

/// Label and display name for the account at `position`. Name and email come from GitHub, so only these two change.
fn edit_form(accounts: &[Account], position: usize) -> Result<Option<Account>> {
    let mut candidate = accounts[position].clone();
    loop {
        if !ask("Label (short name, e.g. work):", &mut candidate.label)? {
            return Ok(None);
        }
        candidate.name = text_input(
            "Display name (blank shows the email):",
            &candidate.name,
            false,
            true,
        )?;
        let others: Vec<Account> = accounts
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != position)
            .map(|(_, account)| account.clone())
            .collect();
        match accounts::problem(&candidate, &others) {
            None => return Ok(Some(candidate)),
            Some(message) => wait_key(
                &[screen::danger(message)],
                "Press any key to fix the values",
            )?,
        }
    }
}
