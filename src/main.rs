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
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default)]
        paths: Vec<String>,
        cwd: Option<String>,
    },
}

impl CommandEntry {
    fn target(&self) -> &str {
        match self {
            CommandEntry::Simple(t) => t,
            CommandEntry::Detailed { target, .. } => target,
        }
    }

    fn args(&self) -> &[String] {
        match self {
            CommandEntry::Simple(_) => &[],
            CommandEntry::Detailed { args, .. } => args,
        }
    }

    fn env(&self) -> Option<&HashMap<String, String>> {
        match self {
            CommandEntry::Simple(_) => None,
            CommandEntry::Detailed { env, .. } => Some(env),
        }
    }

    fn paths(&self) -> &[String] {
        match self {
            CommandEntry::Simple(_) => &[],
            CommandEntry::Detailed { paths, .. } => paths,
        }
    }

    fn cwd(&self) -> Option<&str> {
        match self {
            CommandEntry::Simple(_) => None,
            CommandEntry::Detailed { cwd, .. } => cwd.as_deref(),
        }
    }
}

#[derive(Deserialize, Debug)]
struct Config {
    #[serde(default)]
    global_env: HashMap<String, String>,
    #[serde(default)]
    global_paths: Vec<String>,
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
    
    // Apply pre-defined args from config
    command.args(entry.args());
    
    // Apply args from command line
    command.args(&args);

    // Set working directory if specified
    if let Some(cwd) = entry.cwd() {
        command.current_dir(cwd);
    }

    // Handle PATH specially
    let mut extra_paths = config.global_paths.clone();
    extra_paths.extend(entry.paths().to_vec());

    if !extra_paths.is_empty() {
        if let Some(existing_path) = env::var_os("PATH") {
            let mut paths = env::split_paths(&existing_path).collect::<Vec<_>>();
            for p in extra_paths {
                paths.push(std::path::PathBuf::from(p));
            }
            if let Ok(new_path) = env::join_paths(paths) {
                command.env("PATH", new_path);
            }
        } else {
            if let Ok(new_path) = env::join_paths(extra_paths) {
                command.env("PATH", new_path);
            }
        }
    }

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
            global_paths = ["/global/bin"]

            [commands]
            ls = "ls -la"
            curl = { target = "curl", args = ["-k", "-s"], env = { "USER" = "dory" }, cwd = "/tmp" }
            git = { target = "git", env = { "GIT_AUTHOR_NAME" = "Dory" }, paths = ["/git/bin"] }
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.global_env.get("GLOBAL").unwrap(), "1");
        assert_eq!(config.global_paths, vec!["/global/bin"]);
        
        let ls = config.commands.get("ls").unwrap();
        assert_eq!(ls.target(), "ls -la");
        assert!(ls.args().is_empty());

        let curl = config.commands.get("curl").unwrap();
        assert_eq!(curl.target(), "curl");
        assert_eq!(curl.args(), vec!["-k", "-s"]);
        assert_eq!(curl.env().unwrap().get("USER").unwrap(), "dory");
        assert_eq!(curl.cwd().unwrap(), "/tmp");

        let git = config.commands.get("git").unwrap();
        assert_eq!(git.target(), "git");
        assert_eq!(git.env().unwrap().get("GIT_AUTHOR_NAME").unwrap(), "Dory");
        assert_eq!(git.paths(), vec!["/git/bin"]);
        assert!(git.cwd().is_none());
    }
}
