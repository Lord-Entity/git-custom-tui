use crate::model::Config;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, ExitStatus, Stdio};

const USERNAME_KEY: &str = "credential.https://github.com.username";

#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    pub login: String,
    pub id: u64,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub label: String,
    pub name: String,
    pub email: String,
    pub github_username: String,
}

/// Global git values as they were before the first account was activated.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Original {
    pub name: Option<String>,
    pub email: Option<String>,
    pub username: Option<String>,
}

/// First problem with a candidate account, or None when it can be saved.
pub fn problem(candidate: &Account, others: &[Account]) -> Option<String> {
    if candidate.label.trim().is_empty() {
        return Some("label is required".into());
    }
    if others.iter().any(|a| a.label == candidate.label) {
        return Some(format!("label {} is already used", candidate.label));
    }
    let email = &candidate.email;
    if email.trim().is_empty() {
        return Some("email is required".into());
    }
    if email.matches('@').count() != 1 || email.chars().any(char::is_whitespace) {
        return Some("email must contain one '@' and no spaces".into());
    }
    if let Some(other) = others.iter().find(|a| a.email.eq_ignore_ascii_case(email)) {
        return Some(format!("email already used by {}", other.label));
    }
    let username = &candidate.github_username;
    if username.is_empty() {
        return Some("GitHub username is required".into());
    }
    if username.len() > 39
        || !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Some(
            "GitHub username may only use letters, digits or '-' (max 39 characters)".into(),
        );
    }
    if let Some(other) = others
        .iter()
        .find(|a| a.github_username.eq_ignore_ascii_case(username))
    {
        return Some(format!("GitHub username already used by {}", other.label));
    }
    None
}

/// Points the global git identity at the named account, remembering the old values first.
pub fn activate(config: &mut Config, label: &str) -> Result<()> {
    let account = config
        .accounts
        .iter()
        .find(|a| a.label == label)
        .cloned()
        .ok_or_else(|| anyhow!("no account named {label}"))?;
    let fresh = config.original.is_none();
    // Values in place right now: a failed switch puts exactly these back.
    let before = read_global();
    if fresh {
        config.original = Some(before.clone());
    }
    // A blank display name would leave git with no usable name, so fall back to the login.
    let name = if account.name.trim().is_empty() {
        account.github_username.as_str()
    } else {
        account.name.as_str()
    };
    let applied = git_set("user.name", name)
        .and_then(|_| git_set("user.email", &account.email))
        .and_then(|_| git_set(USERNAME_KEY, &account.github_username));
    if let Err(error) = applied {
        restore("user.name", before.name.as_deref()).ok();
        restore("user.email", before.email.as_deref()).ok();
        restore(USERNAME_KEY, before.username.as_deref()).ok();
        if fresh {
            config.original = None;
        }
        return Err(error);
    }
    config.active_account = Some(label.to_string());
    // Save now, so a crash or closed window after the switch still leaves the snapshot on disk.
    crate::config::save(config)
}

/// Restores the global git values saved by `activate`, if any.
pub fn deactivate(config: &mut Config) -> Result<()> {
    if let Some(original) = config.original.clone() {
        restore("user.name", original.name.as_deref())?;
        restore("user.email", original.email.as_deref())?;
        restore(USERNAME_KEY, original.username.as_deref())?;
        config.original = None;
    }
    config.active_account = None;
    Ok(())
}

/// GitHub usernames that Git Credential Manager has a saved login for. Empty if it cannot run.
pub fn stored_logins() -> Vec<String> {
    Command::new("git")
        .args(["credential-manager", "github", "list"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Opens the browser to sign `username` in to GitHub; Credential Manager stores the login.
/// Call only with the TUI suspended, because the browser flow prints to the terminal.
pub fn login(username: &str) -> Result<()> {
    if !browser_login(&["--username", username])?.success() {
        bail!("GitHub sign-in for {username} did not finish");
    }
    Ok(())
}

/// Signs in to GitHub as `username`, then fills the account from GitHub's profile.
/// Call only with the TUI suspended.
pub fn sign_in_new(label: &str, username: &str) -> Result<Account> {
    login(username)?;
    Ok(account_from(label, &fetch_profile(username)?))
}

/// Checks that the login Credential Manager returns for `username` really is that GitHub account.
/// fetch_profile bails when GitHub reports a different login, so a wrong token stops the push.
pub fn verify_login(username: &str) -> Result<()> {
    fetch_profile(username).map(|_| ())
}

/// Re-fetches name and email from GitHub, keeping the label.
pub fn refresh(account: &mut Account) -> Result<()> {
    let fresh = account_from(&account.label, &fetch_profile(&account.github_username)?);
    account.name = fresh.name;
    account.email = fresh.email;
    Ok(())
}

/// Reads the signed-in user's profile from the GitHub API. The token lives only for this request.
pub fn fetch_profile(username: &str) -> Result<Profile> {
    let token = token_for(username)?;
    ureq::get("https://api.github.com/user")
        .set("Authorization", &format!("Bearer {token}"))
        .set("User-Agent", "gct")
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .context("GitHub profile request failed")?
        .into_string()
        .context("GitHub profile could not be read")
        .and_then(|body| {
            let profile: Profile =
                serde_json::from_str(&body).context("GitHub returned an unexpected profile")?;
            if !profile.login.eq_ignore_ascii_case(username) {
                bail!(
                    "GitHub returned {} but {username} was requested; sign in again with the right account",
                    profile.login
                );
            }
            Ok(profile)
        })
}

fn account_from(label: &str, profile: &Profile) -> Account {
    let name = profile.name.clone().filter(|name| !name.trim().is_empty());
    let email = profile
        .email
        .clone()
        .filter(|email| !email.trim().is_empty());
    Account {
        label: label.into(),
        name: name.unwrap_or_else(|| profile.login.clone()),
        email: email.unwrap_or_else(|| {
            format!("{}+{}@users.noreply.github.com", profile.id, profile.login)
        }),
        github_username: profile.login.clone(),
    }
}

fn browser_login(extra: &[&str]) -> Result<ExitStatus> {
    Command::new("git")
        .args(["credential-manager", "github", "login"])
        .args(extra)
        .arg("--browser")
        .status()
        .context("could not start Git Credential Manager")
}

/// The password Credential Manager holds for `username`. Never put this in an error or a log.
fn token_for(username: &str) -> Result<String> {
    let mut child = Command::new("git")
        .args(["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start git credential")?;
    let request = format!("protocol=https\nhost=github.com\nusername={username}\n\n");
    // Dropping the handle after the write closes stdin, which tells git the request is complete.
    child
        .stdin
        .take()
        .context("could not reach git credential")?
        .write_all(request.as_bytes())
        .context("could not reach git credential")?;
    let output = child
        .wait_with_output()
        .context("could not read git credential")?;
    let missing = || anyhow!("no saved GitHub login for {username}");
    if !output.status.success() {
        return Err(missing());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("password="))
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .ok_or_else(missing)
}

fn read_global() -> Original {
    Original {
        name: git_get("user.name"),
        email: git_get("user.email"),
        username: git_get(USERNAME_KEY),
    }
}

// Missing key (exit 1), git not installed, or any other failure all map to None.
fn git_get(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn git_set(key: &str, value: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["config", "--global", key, value])
        .status()
        .with_context(|| format!("could not set {key}"))?;
    if !status.success() {
        bail!("could not set {key}");
    }
    Ok(())
}

fn restore(key: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => {
            // --replace-all so keys that hold several values restore cleanly.
            let status = Command::new("git")
                .args(["config", "--global", "--replace-all", key, value])
                .status()
                .with_context(|| format!("could not set {key}"))?;
            if !status.success() {
                bail!("could not set {key}");
            }
            Ok(())
        }
        None => {
            let status = Command::new("git")
                .args(["config", "--global", "--unset", key])
                .status()
                .with_context(|| format!("could not unset {key}"))?;
            // Exit code 5 means the key was already unset.
            if status.success() || status.code() == Some(5) {
                Ok(())
            } else {
                bail!("could not unset {key}")
            }
        }
    }
}
