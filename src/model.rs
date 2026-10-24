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
