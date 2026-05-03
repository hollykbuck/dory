use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::{Command, exit};
use std::sync::{Arc, Mutex};

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
            new_commands.insert(normalize_command_name(&name), entry);
        }
        self.commands = new_commands;
    }
}

fn normalize_command_name(name: &str) -> String {
    let normalized = name.to_lowercase();

    #[cfg(windows)]
    {
        normalized
            .strip_suffix(".exe")
            .unwrap_or(&normalized)
            .to_string()
    }

    #[cfg(not(windows))]
    {
        normalized
    }
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "-h" | "--help")
}

fn is_version_arg(arg: &str) -> bool {
    matches!(arg, "-v" | "--version")
}

fn print_version() {
    println!("dory v{}", env!("CARGO_PKG_VERSION"));
    println!("Commit: {}", env!("VERGEN_GIT_SHA"));
    println!("Build Date: {}", env!("VERGEN_BUILD_TIMESTAMP"));
}

fn print_help() {
    println!(
        r#"dory v{} - command wrapper (Commit: {})

Usage:
  dory -h|--help
  dory -v|--version
  <wrapped-command> [args...]

Configuration:
  Dory maps the executable name used to invoke it to an entry under [commands].
  On Windows, a trailing .exe suffix is ignored for command lookup.

Config search order:
  1. user config directory: dory/config.toml

Local dory.toml, .dory.toml, and ~/.dory.toml files are not trusted by default."#,
        env!("CARGO_PKG_VERSION"),
        env!("VERGEN_GIT_SHA")
    );
}

fn trusted_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(mut config_dir) = dirs::config_dir() {
        config_dir.push("dory/config.toml");
        paths.push(config_dir);
    }

    paths
}

fn load_config() -> Option<Config> {
    let mut config_data: Option<String> = None;
    let mut config_path: Option<String> = None;

    for path in trusted_config_paths() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            config_data = Some(data);
            config_path = Some(path.display().to_string());
            break;
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
    let cmd_name = normalize_command_name(&cmd_name_raw);
    let args: Vec<String> = env::args().skip(1).collect();

    if cmd_name == "dory" {
        if args.iter().any(|arg| is_help_arg(arg)) {
            print_help();
            exit(0);
        }
        if args.iter().any(|arg| is_version_arg(arg)) {
            print_version();
            exit(0);
        }
    }

    let mut config = load_config().unwrap_or_else(|| {
        eprintln!("Configuration not found or failed to load.");
        eprintln!("Please create 'dory/config.toml' in your user config folder.");
        exit(127);
    });

    config.normalize();

    let entry = config.commands.get(&cmd_name).cloned().unwrap_or_else(|| {
        eprintln!(
            "No command mapping for '{}' in configuration.",
            cmd_name_raw
        );
        exit(127);
    });

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
        } else if let Ok(new_path) = env::join_paths(extra_paths) {
            command.env("PATH", new_path);
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
    })
    .expect("failed to install ctrlc handler");

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

    #[test]
    #[cfg(windows)]
    fn test_normalize_command_name_ignores_windows_exe_suffix() {
        assert_eq!(normalize_command_name("vim"), "vim");
        assert_eq!(normalize_command_name("vim.exe"), "vim");
        assert_eq!(normalize_command_name("vim.EXE"), "vim");
        assert_eq!(normalize_command_name("vim.exe.exe"), "vim.exe");
    }

    #[test]
    #[cfg(not(windows))]
    fn test_normalize_command_name_keeps_exe_suffix_on_non_windows() {
        assert_eq!(normalize_command_name("vim"), "vim");
        assert_eq!(normalize_command_name("vim.exe"), "vim.exe");
        assert_eq!(normalize_command_name("vim.EXE"), "vim.exe");
    }

    #[test]
    fn test_is_help_arg() {
        assert!(is_help_arg("-h"));
        assert!(is_help_arg("--help"));
        assert!(!is_help_arg("-help"));
        assert!(!is_help_arg("help"));
    }

    #[test]
    fn test_trusted_config_paths_do_not_include_local_configs() {
        let paths = trusted_config_paths();

        assert!(!paths.contains(&PathBuf::from("dory.toml")));
        assert!(!paths.contains(&PathBuf::from(".dory.toml")));
        assert!(!paths.contains(&PathBuf::from("~/.dory.toml")));
        assert!(!paths.iter().any(|path| path.ends_with(".dory.toml")));
        assert!(paths.iter().all(|path| path.is_absolute()));
    }
}
