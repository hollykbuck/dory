# Dory

A command wrapper that maps executable names to target commands with environment variable, PATH, default arguments, and working directory support.

## Configuration

Dory only trusts the user config folder by default:
   - **Windows:** `%AppData%\dory\config.toml`
   - **Linux/macOS:** `$XDG_CONFIG_HOME/dory/config.toml` or `~/.config/dory/config.toml`

Local `dory.toml`, `.dory.toml`, and `~/.dory.toml` files are not loaded by default.

### Format

The configuration is in TOML format. You can define global environment variables, global paths, and command-specific mappings.

```toml
# Optional: Global environment variables for all commands
[global_env]
PROJECT_ROOT = "C:/projects"

# Optional: Add directories to the system PATH for all commands
global_paths = [
    "C:/tools/bin",
    "D:/scripts"
]

[commands]
# Simple mapping: executable_name = "target_command"
ls = "ls -la"

# Detailed mapping with working directory, arguments, and environment variables
curl = { target = "curl", args = ["-k", "-s"], env = { "USER" = "dory" }, cwd = "C:/temp" }

# Example: pass arguments as raw command-line fragments on Windows
code = { target = "C:/Windows/System32/cmd.exe", raw = true, args = ["/c", "\"C:/Program Files/Microsoft VS Code/bin/code.cmd\""] }

# Example: git with specific author and extra bin path
git = { target = "git", env = { "GIT_AUTHOR_NAME" = "Dory" }, paths = ["C:/git-extra/bin"] }

# Example: Run build in a specific directory
build = { target = "npm", args = ["run", "dev"], cwd = "./frontend" }
```

### How it works

1. Rename or symlink the `dory` executable to the name you want to wrap (e.g., `curl.exe` or `build.exe`).
2. When you run `curl.exe https://google.com`, Dory looks up `curl` in the `commands` section of the config.
3. It sets the working directory to `cwd` if specified.
4. It sets the specified environment variables and appends the `paths` to `PATH`.
5. It executes the target command `curl` with the default `args` followed by your command-line arguments.

On Windows, set `raw = true` when you want Dory to concatenate `args` and command-line arguments as raw command-line fragments without applying Rust's normal argument quoting.
