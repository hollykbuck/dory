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
    #[serde(default)]
    commands: HashMap<String, CommandEntry>,
}

impl Config {
    fn normalize(&mut self) {
        let mut new_commands = HashMap::with_capacity(self.commands.len());
        for (name, entry) in self.commands.drain() {
            new_commands.insert(name.to_lowercase(), entry);
        }
        self.commands = new_commands;
    }
}

fn load_config() -> Option<Config> {
    let mut config_data: Option<String> = None;
    let mut config_path: Option<String> = None;

    // Try local config first
    let local_paths = ["dory.toml", ".dory.toml"];
    for path in local_paths {
        if let Ok(data) = std::fs::read_to_string(path) {
            config_data = Some(data);
            config_path = Some(path.to_string());
            break;
        }
    }

    if config_data.is_none() {
        if let Some(home) = dirs::home_dir() {
            // Try ~/.dory.toml
            let mut p1 = home.clone();
            p1.push(".dory.toml");
            if let Ok(data) = std::fs::read_to_string(&p1) {
                config_data = Some(data);
                config_path = Some(p1.display().to_string());
            }

            if config_data.is_none() {
                // Try $XDG_CONFIG_HOME/dory/config.toml or ~/.config/dory/config.toml
                let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        let mut p = home.clone();
                        p.push(".config");
                        p
                    });
                
                let mut p2 = xdg_config_home;
                p2.push("dory/config.toml");
                if let Ok(data) = std::fs::read_to_string(&p2) {
                    config_data = Some(data);
                    config_path = Some(p2.display().to_string());
                }
            }
        }
    }

    if config_data.is_none() {
        if let Some(mut path) = dirs::config_dir() {
            path.push("dory/config.toml");
            if let Ok(data) = std::fs::read_to_string(&path) {
                config_data = Some(data);
                config_path = Some(path.display().to_string());
            }
        }
    }

    if let (Some(data), Some(path)) = (config_data, config_path) {
        match toml::from_str(&data) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Error parsing config {}: {}", path, e);
                None
            }
        }
    } else {
        None
    }
}

fn main() {
    // argv[0] -> command name
    let argv0 = env::args().next().unwrap();
    let cmd_name_raw = std::path::Path::new(&argv0)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let mut config = load_config().unwrap_or_else(|| {
        eprintln!("Configuration not found or failed to load.");
        eprintln!("Please create a 'dory.toml' in the current directory or in your config folder.");
        exit(127);
    });

    config.normalize();

    let cmd_name = cmd_name_raw.to_lowercase();
    let entry = config.commands.get(&cmd_name).cloned().unwrap_or_else(|| {
        eprintln!("No command mapping for '{}' in configuration.", cmd_name_raw);
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
    fn test_deserialize_config_and_normalize() {
        let toml_str = r#"
            global_env = { "GLOBAL" = "1" }
            global_paths = ["/global/bin"]

            [commands]
            LS = "ls -la"
            Curl = { target = "curl", args = ["-k", "-s"], env = { "USER" = "dory" }, cwd = "/tmp" }
        "#;

        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();
        
        assert!(config.commands.contains_key("ls"));
        assert!(config.commands.contains_key("curl"));
        assert!(!config.commands.contains_key("LS"));
    }
}
