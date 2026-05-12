use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::{Command, exit};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum CommandEntry {
    Simple(String),
    Detailed {
        target: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        raw: bool,
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

    fn raw(&self) -> bool {
        match self {
            CommandEntry::Simple(_) => false,
            CommandEntry::Detailed { raw, .. } => *raw,
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

#[cfg(windows)]
fn is_cmd_script(target: &str) -> bool {
    std::path::Path::new(target)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "cmd" | "bat"))
}

#[cfg(windows)]
fn quote_for_cmd(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(|c| c.is_whitespace()) {
        format!("\"{}\"", arg.replace('"', "\"\""))
    } else {
        arg.to_string()
    }
}

#[cfg(windows)]
fn windows_cmd_call(entry: &CommandEntry, cli_args: &[String]) -> String {
    let mut parts = Vec::with_capacity(entry.args().len() + cli_args.len() + 2);
    parts.push("call".to_string());
    parts.push(quote_for_cmd(entry.target()));
    parts.extend(entry.args().iter().map(|arg| quote_for_cmd(arg)));
    parts.extend(cli_args.iter().map(|arg| quote_for_cmd(arg)));
    parts.join(" ")
}

#[cfg(windows)]
fn raw_command_tail(entry: &CommandEntry, cli_args: &[String]) -> String {
    let mut parts = Vec::with_capacity(entry.args().len() + cli_args.len());
    parts.extend(entry.args().iter().cloned());
    parts.extend(cli_args.iter().cloned());
    parts.join(" ")
}

fn build_command(entry: &CommandEntry, cli_args: &[String]) -> Command {
    #[cfg(windows)]
    if entry.raw() {
        let mut command = Command::new(entry.target());
        let tail = raw_command_tail(entry, cli_args);
        if !tail.is_empty() {
            command.raw_arg(format!(" {}", tail));
        }
        return command;
    }

    #[cfg(windows)]
    if is_cmd_script(entry.target()) {
        let mut command = Command::new("C:\\Windows\\System32\\cmd.exe");
        command.arg("/d");
        command.arg("/c");
        command.raw_arg(format!(" {}", windows_cmd_call(entry, cli_args)));
        return command;
    }

    let mut command = Command::new(entry.target());
    command.args(entry.args());
    command.args(cli_args);
    command
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

    let mut command = build_command(&entry, &args);

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
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_deserialize_config_and_normalize() {
        let toml_str = r#"
            global_env = { "GLOBAL" = "1" }
            global_paths = ["/global/bin"]

            [commands]
            LS = "ls -la"
            Curl = { target = "curl", args = ["-k", "-s"], raw = true, env = { "USER" = "dory" }, cwd = "/tmp" }
        "#;

        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();

        assert!(config.commands.contains_key("ls"));
        assert!(config.commands.contains_key("curl"));
        assert!(!config.commands.contains_key("LS"));
        assert!(config.commands["curl"].raw());
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
    #[cfg(windows)]
    fn test_is_cmd_script() {
        assert!(is_cmd_script("code.cmd"));
        assert!(is_cmd_script("C:/tools/build.BAT"));
        assert!(!is_cmd_script("code.exe"));
    }

    #[test]
    #[cfg(windows)]
    fn test_windows_cmd_call_wraps_cmd_scripts_with_call() {
        let entry = CommandEntry::Detailed {
            target: "C:/Users/dingle/AppData/Local/Programs/Microsoft VS Code/bin/code.cmd"
                .to_string(),
            args: vec!["--reuse-window".to_string()],
            raw: false,
            env: HashMap::new(),
            paths: Vec::new(),
            cwd: None,
        };
        let cli_args = vec!["--version".to_string()];
        assert_eq!(
            windows_cmd_call(&entry, &cli_args),
            "call \"C:/Users/dingle/AppData/Local/Programs/Microsoft VS Code/bin/code.cmd\" --reuse-window --version"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_cmd_c_needs_cmd_level_quoting_for_batch_paths_with_spaces() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = env::temp_dir().join(format!("dory cmd spacing {nonce}"));
        std::fs::create_dir_all(&base).unwrap();

        let script = base.join("echo argv.cmd");
        std::fs::write(
            &script,
            "@echo off\r\necho SCRIPT:%~f0\r\necho ARG1:%1\r\n",
        )
        .unwrap();

        let split_args = Command::new("C:\\Windows\\System32\\cmd.exe")
            .arg("/d")
            .arg("/c")
            .arg(script.as_os_str())
            .arg("hello")
            .output()
            .unwrap();

        assert!(split_args.status.success());
        let split_stdout = String::from_utf8_lossy(&split_args.stdout);
        assert!(split_stdout.contains("SCRIPT:"));
        assert!(split_stdout.contains("ARG1:hello"));

        let quoted_as_normal_arg = Command::new("C:\\Windows\\System32\\cmd.exe")
            .arg("/d")
            .arg("/c")
            .arg(format!("\"{}\" hello", script.display()))
            .output()
            .unwrap();

        assert!(!quoted_as_normal_arg.status.success());
        let quoted_as_normal_arg_stderr = String::from_utf8_lossy(&quoted_as_normal_arg.stderr);
        assert!(quoted_as_normal_arg_stderr.contains("is not recognized"));
        assert!(quoted_as_normal_arg_stderr.contains("\\\""));

        let quoted_as_raw_arg = Command::new("C:\\Windows\\System32\\cmd.exe")
            .arg("/d")
            .arg("/c")
            .raw_arg(format!(" \"{}\" hello", script.display()))
            .output()
            .unwrap();

        assert!(quoted_as_raw_arg.status.success());
        let quoted_as_raw_arg_stdout = String::from_utf8_lossy(&quoted_as_raw_arg.stdout);
        assert!(quoted_as_raw_arg_stdout.contains("SCRIPT:"));
        assert!(quoted_as_raw_arg_stdout.contains("ARG1:hello"));

        let quoted_with_call = Command::new("C:\\Windows\\System32\\cmd.exe")
            .arg("/d")
            .arg("/c")
            .raw_arg(format!(" call \"{}\" hello", script.display()))
            .output()
            .unwrap();

        assert!(quoted_with_call.status.success());
        let stdout = String::from_utf8_lossy(&quoted_with_call.stdout);
        assert!(stdout.contains("SCRIPT:"));
        assert!(stdout.contains("ARG1:hello"));

        let _ = std::fs::remove_file(script);
        let _ = std::fs::remove_dir(base);
    }

    #[test]
    #[cfg(windows)]
    fn test_raw_command_tail_preserves_user_supplied_fragments() {
        let entry = CommandEntry::Detailed {
            target: "C:/Windows/System32/cmd.exe".to_string(),
            args: vec![
                "/d".to_string(),
                "/c".to_string(),
                "\"C:\\Program Files\\Tool\\tool.cmd\"".to_string(),
            ],
            raw: true,
            env: HashMap::new(),
            paths: Vec::new(),
            cwd: None,
        };
        let cli_args = vec!["--flag".to_string(), "\"value with spaces\"".to_string()];

        assert_eq!(
            raw_command_tail(&entry, &cli_args),
            "/d /c \"C:\\Program Files\\Tool\\tool.cmd\" --flag \"value with spaces\""
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_raw_mode_executes_cmd_with_user_supplied_quoting() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = env::temp_dir().join(format!("dory raw mode {nonce}"));
        std::fs::create_dir_all(&base).unwrap();

        let script = base.join("print args.cmd");
        std::fs::write(&script, "@echo off\r\necho ARG1:%1\r\n").unwrap();

        let entry = CommandEntry::Detailed {
            target: "C:\\Windows\\System32\\cmd.exe".to_string(),
            args: vec![
                "/d".to_string(),
                "/c".to_string(),
                format!("\"{}\"", script.display()),
            ],
            raw: true,
            env: HashMap::new(),
            paths: Vec::new(),
            cwd: None,
        };
        let cli_args = vec!["hello".to_string()];
        let output = build_command(&entry, &cli_args).output().unwrap();

        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("ARG1:hello"));

        let _ = std::fs::remove_file(script);
        let _ = std::fs::remove_dir(base);
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
