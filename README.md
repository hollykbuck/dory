# Dory

A command wrapper that maps executable names to target commands with environment variable support.

## Configuration

Dory searches for a configuration file in the following order:
1. `dory.toml` (in the current directory)
2. `.dory.toml` (in the current directory)
3. System config folder:
   - **Windows:** `%AppData%\dory\config.toml`
   - **Linux/macOS:** `$XDG_CONFIG_HOME/dory/config.toml` or `~/.config/dory/config.toml`

### Format

The configuration is in TOML format. You can define global environment variables and command-specific mappings.

```toml
# Optional: Global environment variables for all commands
[global_env]
PROJECT_ROOT = "C:/projects"

[commands]
# Simple mapping: executable_name = "target_command"
ls = "ls -la"

# Detailed mapping with environment variables
git = { target = "git", env = { "GIT_AUTHOR_NAME" = "Dory", "GIT_AUTHOR_EMAIL" = "dory@example.com" } }

# Another example
build = { target = "npm", env = { "NODE_ENV" = "production" } }
```

### How it works

1. Rename or symlink the `dory` executable to the name you want to wrap (e.g., `git.exe` or `build.exe`).
2. When you run `build.exe`, Dory looks up `build` in the `commands` section of the config.
3. It sets the specified environment variables and executes the target command with any additional arguments you provided.
