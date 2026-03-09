use std::env;
use std::process::{Command, exit};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum CommandEntry {
    Simple(String),
    Detailed {
        target: String,
        #[serde(default)]
        env: HashMap<String, String>,
    },
}

impl CommandEntry {
    fn target(&self) -> &str {
        match self {
            CommandEntry::Simple(t) => t,
            CommandEntry::Detailed { target, .. } => target,
        }
    }

    fn env(&self) -> Option<&HashMap<String, String>> {
        match self {
            CommandEntry::Simple(_) => None,
            CommandEntry::Detailed { env, .. } => Some(env),
        }
    }
}

#[derive(Deserialize, Debug)]
struct Config {
    #[serde(default)]
    global_env: HashMap<String, String>,
    commands: HashMap<String, CommandEntry>,
}

fn load_config() -> Option<Config> {
    // Try local config first
    let local_paths = ["dory.toml", ".dory.toml"];
    for path in local_paths {
        if let Ok(data) = std::fs::read_to_string(path) {
            match toml::from_str(&data) {
                Ok(c) => return Some(c),
                Err(e) => {
                    eprintln!("Error parsing local config {}: {}", path, e);
                    return None;
                }
            }
        }
    }

    // Then try system config
    let mut path = dirs::config_dir()?;
    path.push("dory/config.toml");

    match std::fs::read_to_string(&path) {
        Ok(data) => match toml::from_str(&data) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Error parsing config {}: {}", path.display(), e);
                None
            }
        },
        Err(_) => None, // Silent fail if system config doesn't exist
    }
}

fn main() {
    // argv[0] -> command name
    let argv0 = env::args().next().unwrap();
    let cmd_name = std::path::Path::new(&argv0)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let config = load_config().unwrap_or_else(|| {
        eprintln!("Configuration not found or failed to load.");
        eprintln!("Please create a 'dory.toml' in the current directory or in your config folder.");
        exit(127);
    });

    let entry = config.commands.get(&cmd_name).cloned().unwrap_or_else(|| {
        eprintln!("No command mapping for '{}' in configuration.", cmd_name);
        exit(127);
    });

    let args: Vec<String> = env::args().skip(1).collect();

    let mut command = Command::new(entry.target());
    command.args(&args);

    // Apply global env
    for (key, value) in &config.global_env {
        command.env(key, value);
    }

    // Apply command-specific env
    if let Some(envs) = entry.env() {
        for (key, value) in envs {
            command.env(key, value);
        }
    }

    let child = command.spawn().expect("failed to start process");

    // Ctrl+C forwarding
    let child_arc = Arc::new(Mutex::new(Some(child)));

    let handler_child = child_arc.clone();

    ctrlc::set_handler(move || {
        if let Some(ref mut ch) = *handler_child.lock().unwrap() {
            let _ = ch.kill();
        }
    }).expect("failed to install ctrlc handler");

    let status = {
        let mut guard = child_arc.lock().unwrap();
        guard.take().unwrap().wait().unwrap()
    };

    // propagate exit code
    match status.code() {
        Some(code) => exit(code),
        None => exit(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_config() {
        let toml_str = r#"
            global_env = { "GLOBAL" = "1" }

            [commands]
            ls = "ls -la"
            git = { target = "git", env = { "GIT_AUTHOR_NAME" = "Dory" } }
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.global_env.get("GLOBAL").unwrap(), "1");
        
        let ls = config.commands.get("ls").unwrap();
        assert_eq!(ls.target(), "ls -la");
        assert!(ls.env().is_none());

        let git = config.commands.get("git").unwrap();
        assert_eq!(git.target(), "git");
        assert_eq!(git.env().unwrap().get("GIT_AUTHOR_NAME").unwrap(), "Dory");
    }
}
