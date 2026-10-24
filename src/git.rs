use anyhow::{bail, Context, Result};
use std::process::Command;

pub fn run(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("could not start git {}", args.join(" ")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "{}",
            if detail.is_empty() {
                "git command failed".to_string()
            } else {
                detail
            }
        );
    }
    // trim_end only: `status --short` lines start with a meaningful space (" M file").
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

/// Works on a branch with no commits yet (symbolic-ref), and returns "HEAD" when detached.
pub fn current_branch() -> Result<String> {
    run(&["symbolic-ref", "--short", "-q", "HEAD"])
        .or_else(|_| run(&["rev-parse", "--abbrev-ref", "HEAD"]))
}

pub fn status_lines() -> Result<Vec<String>> {
    let status = run(&["status", "--short"])?;
    Ok(if status.is_empty() {
        vec!["(clean working tree)".into()]
    } else {
        status.lines().map(str::to_owned).collect()
    })
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

/// True when the repo's own config sets `key`. `git config --get` exits 1 when unset.
pub fn has_local_config(key: &str) -> bool {
    run(&["config", "--local", "--get", key]).is_ok_and(|value| !value.is_empty())
}

pub fn commit(message: &str, date: &str, amend: bool, identity: Option<&Identity>) -> Result<()> {
    let args = if amend {
        vec!["commit", "--amend", "-m", message]
    } else {
        vec!["commit", "-m", message]
    };
    let mut env = vec![("GIT_AUTHOR_DATE", date), ("GIT_COMMITTER_DATE", date)];
    // Explicit identity beats repo-local user.name/user.email.
    if let Some(id) = identity {
        // Amend keeps the original author, as git's own --amend does; only the committer changes.
        if !amend {
            env.extend([
                ("GIT_AUTHOR_NAME", id.name.as_str()),
                ("GIT_AUTHOR_EMAIL", id.email.as_str()),
            ]);
        }
        env.extend([
            ("GIT_COMMITTER_NAME", id.name.as_str()),
            ("GIT_COMMITTER_EMAIL", id.email.as_str()),
        ]);
    }
    let output = Command::new("git")
        .args(args)
        .envs(env)
        .output()
        .context("could not start git commit")?;
    if !output.status.success() {
        // Hook and "nothing to commit" messages go to stdout, so fall back to it.
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            stderr
        };
        bail!(
            "{}",
            if detail.is_empty() {
                "git commit failed".into()
            } else {
                detail
            }
        );
    }
    Ok(())
}

pub fn push(branch: &str, amend: bool, login: Option<&str>) -> Result<()> {
    // -c must come before the subcommand; it overrides a repo-local credential.username.
    let credential = login.map(|login| format!("credential.https://github.com.username={login}"));
    let mut args: Vec<&str> = Vec::new();
    if let Some(credential) = credential.as_deref() {
        args.extend(["-c", credential]);
    }
    args.push("push");
    if amend {
        args.push("--force-with-lease");
    }
    args.extend(["origin", branch]);
    run(&args)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub hash: String,
    pub date: String,
    pub subject: String,
    pub refs: String,
}

pub fn recent_commits(limit: usize) -> Result<Vec<Commit>> {
    let limit = limit.to_string();
    let log = match run(&[
        "log",
        "-n",
        &limit,
        "--no-color",
        "--date=short",
        "--format=%h%x1f%ad%x1f%s%x1f%D",
    ]) {
        Ok(log) => log,
        // Fresh repo: git exits non-zero, which is not an error for the caller.
        Err(e) if e.to_string().contains("does not have any commits") => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    Ok(log
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\u{1f}').collect();
            match fields[..] {
                [hash, date, subject, refs] => Some(Commit {
                    hash: hash.to_string(),
                    date: date.to_string(),
                    subject: subject.to_string(),
                    refs: refs.to_string(),
                }),
                _ => None,
            }
        })
        .collect())
}

pub fn remote_branches() -> Result<Vec<String>> {
    // strip=2 drops "refs/remotes/", leaving "origin/main".
    let out = run(&["branch", "-r", "--no-color", "--format=%(refname:strip=2)"])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|b| !b.is_empty() && !b.ends_with("/HEAD"))
        .map(str::to_owned)
        .collect())
}

pub fn upstream(branch: &str) -> Result<Option<String>> {
    let refname = format!("refs/heads/{branch}");
    let out = run(&["for-each-ref", "--format=%(upstream:short)", &refname])?;
    Ok(if out.is_empty() { None } else { Some(out) })
}
