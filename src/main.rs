use std::env;
use std::process::{Command, exit};
use std::sync::{Arc, Mutex};

use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    commands: std::collections::HashMap<String, String>,
}

fn load_config() -> Option<Config> {
    let mut path = dirs::config_dir()?;
    path.push("dory/config.toml");

    let data = std::fs::read_to_string(path).ok()?;
    toml::from_str(&data).ok()
}

fn main() {
    // argv[0] -> command name
    let argv0 = env::args().next().unwrap();
    let cmd_name = std::path::Path::new(&argv0)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let config = load_config();

    let target = config
        .as_ref()
        .and_then(|c| c.commands.get(&cmd_name))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("No command mapping for {}", cmd_name);
            exit(127);
        });

    let args: Vec<String> = env::args().skip(1).collect();

    let child = Command::new(target)
        .args(&args)
        .spawn()
        .expect("failed to start process");

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