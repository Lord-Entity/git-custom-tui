use std::process::Command;

#[derive(Debug, Default)]
pub struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
}

pub fn read() -> Identity {
    Identity {
        name: get("user.name"),
        email: get("user.email"),
    }
}

// Missing key (exit 1), git not installed, or any other failure all map to None.
fn get(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}
