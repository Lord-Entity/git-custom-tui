use chrono::Datelike;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub last_date: String,
    pub last_branch: String,
    pub last_message: String,
    #[serde(default)]
    pub accounts: Vec<crate::accounts::Account>,
    #[serde(default)]
    pub active_account: Option<String>,
    #[serde(default)]
    pub original: Option<crate::accounts::Original>,
}

#[derive(Clone, Debug)]
pub struct Values {
    pub date: String,
    pub time: String,
    pub branch: String,
    pub message: String,
    pub amend: bool,
    pub dry_run: bool,
}

pub fn initial_values(config: Config, branch: &str) -> Values {
    let (date, time) = config
        .last_date
        .split_once(' ')
        .map_or((String::new(), String::new()), |(date, time)| {
            (date.into(), time.into())
        });
    Values {
        date,
        time,
        branch: if config.last_branch.is_empty() {
            branch.into()
        } else {
            config.last_branch
        },
        message: config.last_message,
        amend: false,
        dry_run: false,
    }
}

pub fn valid_date(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
        return false;
    }
    let Ok(year) = parts[0].parse::<i32>() else {
        return false;
    };
    let Ok(month) = parts[1].parse::<u32>() else {
        return false;
    };
    let Ok(day) = parts[2].parse::<u32>() else {
        return false;
    };
    if !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    day <= days[(month - 1) as usize]
}

pub fn valid_time(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.len() != 2) {
        return false;
    }
    let Ok(hour) = parts[0].parse::<u32>() else {
        return false;
    };
    let Ok(minute) = parts[1].parse::<u32>() else {
        return false;
    };
    let Ok(second) = parts[2].parse::<u32>() else {
        return false;
    };
    hour < 24 && minute < 60 && second < 60
}

/// First problem that stops a commit, or None when the values are ready.
pub fn first_problem(values: &Values) -> Option<&'static str> {
    if !valid_date(&values.date) {
        Some("Please set a valid date")
    } else if !valid_time(&values.time) {
        Some("Please set a valid time (HH:MM:SS)")
    } else if values.branch.trim().is_empty() {
        Some("Please choose a branch")
    } else if values.message.trim().is_empty() {
        Some("Please write a commit message")
    } else {
        None
    }
}

/// Single-line, length-capped text for menus and summaries; newlines would break the frame.
pub fn preview(value: &str, max: usize) -> String {
    truncate(&value.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

pub fn current_year() -> String {
    chrono::Local::now().year().to_string()
}

pub fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() > max {
        format!("{}…", value.chars().take(max - 1).collect::<String>())
    } else {
        value.into()
    }
}

/// Appends `-ml` items as a bullet body. Items are marked, in order of preference, by
/// newlines, by spaced dashes (`- one - two`), or by a dash after a space (`-one -two`).
/// The spaced and per-line forms let an item mention flags like `-d`; `--` never splits.
pub fn with_bullets(message: &str, list: &str) -> String {
    let list = list.trim();
    let items: Vec<&str> = if list.contains('\n') {
        list.lines().collect()
    } else if list.starts_with("- ") || list.contains(" - ") {
        list.split(" - ").collect()
    } else {
        split_compact(list)
    };
    let bullets: Vec<String> = items
        .iter()
        .map(|item| item.trim())
        // Drop the item's own dash marker, but keep a leading flag like `--push` whole.
        .map(|item| {
            item.strip_prefix('-')
                .filter(|rest| !rest.starts_with('-'))
                .unwrap_or(item)
                .trim()
        })
        .filter(|item| !item.is_empty())
        .map(|item| format!("- {item}"))
        .collect();
    if bullets.is_empty() {
        message.trim().into()
    } else {
        format!("{}\n\n{}", message.trim(), bullets.join("\n"))
    }
}

/// Splits `-one -two` before each dash that starts the text or follows whitespace, unless it
/// begins `--`. Byte indexing is safe: `-` and ASCII whitespace never occur inside UTF-8 sequences.
fn split_compact(list: &str) -> Vec<&str> {
    let bytes = list.as_bytes();
    let mut starts: Vec<usize> = (0..bytes.len())
        .filter(|&i| {
            bytes[i] == b'-'
                && (i == 0 || bytes[i - 1].is_ascii_whitespace())
                && bytes.get(i + 1) != Some(&b'-')
        })
        .collect();
    if starts.first() != Some(&0) {
        starts.insert(0, 0);
    }
    starts.push(bytes.len());
    starts.windows(2).map(|w| &list[w[0]..w[1]]).collect()
}

#[cfg(test)]
mod tests {
    use super::with_bullets;

    #[test]
    fn bullets() {
        let b = |list| with_bullets("feat: x", list);
        assert_eq!(
            b("-integrate login -integrate log-in"),
            "feat: x\n\n- integrate login\n- integrate log-in"
        );
        assert_eq!(b("  "), "feat: x");
        // Spaced dashes: items may mention flags.
        assert_eq!(
            b("- commit with -d -t without the TUI - gct --agent prints it"),
            "feat: x\n\n- commit with -d -t without the TUI\n- gct --agent prints it"
        );
        // One item per line.
        assert_eq!(b("-a uses -d\n\t- b"), "feat: x\n\n- a uses -d\n- b");
        // Compact form never splits on `--`, and keeps unicode intact.
        assert_eq!(
            b("-run gct --agent -ünï"),
            "feat: x\n\n- run gct --agent\n- ünï"
        );
        assert_eq!(b("no dash -two"), "feat: x\n\n- no dash\n- two");
        // An item may start with a flag.
        assert_eq!(b("- a - --push pushes"), "feat: x\n\n- a\n- --push pushes");
        assert_eq!(b("--push first\n- b"), "feat: x\n\n- --push first\n- b");
    }
}
