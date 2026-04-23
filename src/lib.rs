use std::collections::BTreeMap;

use zed_extension_api::{
    self as zed, settings::ContextServerSettings, ContextServerId, Project, Result,
};

const CONTEXT_SERVER_ID: &str = "jira-mcp";
const MCP_PACKAGE: &str = "mcp-atlassian";

struct JiraMcpExtension;

impl zed::Extension for JiraMcpExtension {
    fn new() -> Self {
        Self
    }

    fn context_server_command(
        &mut self,
        context_server_id: &ContextServerId,
        project: &Project,
    ) -> Result<zed::Command> {
        if context_server_id.as_ref() != CONTEXT_SERVER_ID {
            return Err(format!("Unknown context server: {context_server_id}"));
        }

        let settings = ContextServerSettings::for_project(CONTEXT_SERVER_ID, project)
            .map_err(|err| format!("Failed to read `{CONTEXT_SERVER_ID}` settings: {err}"))?;

        let env = merged_env_from_settings(&settings);

        if let Some(custom_command) = resolve_custom_command(&settings, &env)? {
            return Ok(custom_command);
        }

        resolve_runtime_command(&env)
    }

    fn context_server_configuration(
        &mut self,
        context_server_id: &ContextServerId,
        _project: &Project,
    ) -> Result<Option<zed::ContextServerConfiguration>> {
        if context_server_id.as_ref() != CONTEXT_SERVER_ID {
            return Ok(None);
        }

        let installation_instructions = r#"## Jira MCP Server setup

This extension launches `mcp-atlassian` and expects Jira/Confluence credentials from Zed settings.

### 0. Ensure Rust dev-extension toolchain is available

Zed compiles Rust dev extensions with `rustup`.
If Rust was installed via Homebrew/system package, dev extension build may fail.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustup target add wasm32-wasip1
```

### 1. Install one runtime option

Preferred:

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

Fallback:

```bash
python3 -m pip install --user mcp-atlassian
```

### 2. Configure credentials in Zed settings

```json
{
  "context_servers": {
    "jira-mcp": {
      "settings": {
        "env": {
          "JIRA_URL": "https://your-company.atlassian.net",
          "JIRA_USERNAME": "your.email@company.com",
          "JIRA_API_TOKEN": "your_api_token",
          "CONFLUENCE_URL": "https://your-company.atlassian.net/wiki",
          "CONFLUENCE_USERNAME": "your.email@company.com",
          "CONFLUENCE_API_TOKEN": "your_api_token"
        }
      }
    }
  }
}
```

For Jira Server/Data Center, prefer `JIRA_PERSONAL_TOKEN` instead of
`JIRA_USERNAME` + `JIRA_API_TOKEN`.
"#
        .to_string();

        let default_settings = r#"{
  "env": {
    "JIRA_URL": "https://your-company.atlassian.net",
    "JIRA_USERNAME": "your.email@company.com",
    "JIRA_API_TOKEN": "your_api_token",
    "CONFLUENCE_URL": "https://your-company.atlassian.net/wiki",
    "CONFLUENCE_USERNAME": "your.email@company.com",
    "CONFLUENCE_API_TOKEN": "your_api_token"
  }
}"#
        .to_string();

        let settings_schema = r#"{
  "type": "object",
  "properties": {
    "env": {
      "type": "object",
      "description": "Environment variables forwarded to mcp-atlassian.",
      "additionalProperties": {
        "type": "string"
      },
      "properties": {
        "JIRA_URL": { "type": "string" },
        "JIRA_USERNAME": { "type": "string" },
        "JIRA_API_TOKEN": { "type": "string" },
        "JIRA_PERSONAL_TOKEN": { "type": "string" },
        "CONFLUENCE_URL": { "type": "string" },
        "CONFLUENCE_USERNAME": { "type": "string" },
        "CONFLUENCE_API_TOKEN": { "type": "string" }
      }
    }
  },
  "additionalProperties": false
}"#
        .to_string();

        Ok(Some(zed::ContextServerConfiguration {
            installation_instructions,
            settings_schema,
            default_settings,
        }))
    }
}

fn resolve_custom_command(settings: &ContextServerSettings, env: &[(String, String)]) -> Result<Option<zed::Command>> {
    let Some(command_settings) = settings.command.as_ref() else {
        return Ok(None);
    };

    let Some(path) = command_settings.path.as_ref() else {
        return Ok(None);
    };

    let args = if let Some(arguments) = command_settings.arguments.as_ref() {
        arguments.clone()
    } else if let Some(inferred) = inferred_launch_args(path) {
        inferred
    } else {
        return Err(format!(
            "Configured `command.path` is `{path}` but no `command.arguments` are set. \
Add explicit arguments in `context_servers.{CONTEXT_SERVER_ID}.command.arguments`."
        ));
    };

    Ok(Some(zed::Command {
        command: path.clone(),
        args,
        env: env.to_vec(),
    }))
}

fn resolve_runtime_command(env: &[(String, String)]) -> Result<zed::Command> {
    let mut failures = Vec::new();

    for runtime in runtime_candidates() {
        match probe_runtime(&runtime, env) {
            Ok(()) => {
                return Ok(zed::Command {
                    command: runtime.program.to_string(),
                    args: runtime.launch_args.iter().map(|arg| (*arg).to_string()).collect(),
                    env: env.to_vec(),
                });
            }
            Err(err) => failures.push(err),
        }
    }

    Err(format!(
        "Could not start `{MCP_PACKAGE}`. Tried runtime fallbacks in order:\n- {}",
        failures.join("\n- ")
    ))
}

fn probe_runtime(runtime: &RuntimeCandidate, env: &[(String, String)]) -> Result<()> {
    let mut command = zed::Command::new(runtime.program)
        .args(runtime.probe_args.iter().copied())
        .envs(env.iter().cloned());

    let output = command
        .output()
        .map_err(|err| format!("{}: spawn failed ({err})", runtime.label))?;

    if output.status == Some(0) {
        return Ok(());
    }

    let status = output
        .status
        .map_or_else(|| "terminated by signal".to_string(), |code| code.to_string());
    let stderr = utf8_trimmed_or_default(&output.stderr);
    let stderr = if stderr.is_empty() {
        "no stderr output".to_string()
    } else {
        truncate_for_error(stderr, 500)
    };

    Err(format!("{}: exited with status {status}; stderr: {stderr}", runtime.label))
}

fn merged_env_from_settings(settings: &ContextServerSettings) -> Vec<(String, String)> {
    let mut merged = BTreeMap::<String, String>::new();

    if let Some(command_env) = settings.command.as_ref().and_then(|command| command.env.as_ref()) {
        for (key, value) in command_env {
            merged.insert(key.clone(), value.clone());
        }
    }

    if let Some(settings_env) = settings
        .settings
        .as_ref()
        .and_then(|value| value.as_object())
        .and_then(|object| object.get("env"))
        .and_then(|value| value.as_object())
    {
        for (key, value) in settings_env {
            if let Some(value) = value.as_str() {
                merged.insert(key.clone(), value.to_string());
            }
        }
    }

    merged.into_iter().collect()
}

fn inferred_launch_args(program: &str) -> Option<Vec<String>> {
    let executable = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();

    if executable == "uvx" || executable == "uvx.exe" {
        return Some(vec![MCP_PACKAGE.to_string()]);
    }

    if executable == "uv" || executable == "uv.exe" {
        return Some(vec![
            "tool".to_string(),
            "run".to_string(),
            MCP_PACKAGE.to_string(),
        ]);
    }

    if executable == "python" || executable == "python.exe" || executable == "python3" || executable == "python3.exe" {
        return Some(vec![
            "-m".to_string(),
            "mcp_atlassian".to_string(),
        ]);
    }

    None
}

fn runtime_candidates() -> Vec<RuntimeCandidate> {
    let (os, _) = zed::current_platform();

    match os {
        zed::Os::Windows => vec![
            RuntimeCandidate {
                label: "uvx.exe mcp-atlassian",
                program: "uvx.exe",
                launch_args: &[MCP_PACKAGE],
                probe_args: &[MCP_PACKAGE, "--help"],
            },
            RuntimeCandidate {
                label: "uv.exe tool run mcp-atlassian",
                program: "uv.exe",
                launch_args: &["tool", "run", MCP_PACKAGE],
                probe_args: &["tool", "run", MCP_PACKAGE, "--help"],
            },
            RuntimeCandidate {
                label: "python.exe -m mcp_atlassian",
                program: "python.exe",
                launch_args: &["-m", "mcp_atlassian"],
                probe_args: &["-m", "mcp_atlassian", "--help"],
            },
            RuntimeCandidate {
                label: "python -m mcp_atlassian",
                program: "python",
                launch_args: &["-m", "mcp_atlassian"],
                probe_args: &["-m", "mcp_atlassian", "--help"],
            },
        ],
        _ => vec![
            RuntimeCandidate {
                label: "uvx mcp-atlassian",
                program: "uvx",
                launch_args: &[MCP_PACKAGE],
                probe_args: &[MCP_PACKAGE, "--help"],
            },
            RuntimeCandidate {
                label: "uv tool run mcp-atlassian",
                program: "uv",
                launch_args: &["tool", "run", MCP_PACKAGE],
                probe_args: &["tool", "run", MCP_PACKAGE, "--help"],
            },
            RuntimeCandidate {
                label: "python3 -m mcp_atlassian",
                program: "python3",
                launch_args: &["-m", "mcp_atlassian"],
                probe_args: &["-m", "mcp_atlassian", "--help"],
            },
            RuntimeCandidate {
                label: "python -m mcp_atlassian",
                program: "python",
                launch_args: &["-m", "mcp_atlassian"],
                probe_args: &["-m", "mcp_atlassian", "--help"],
            },
        ],
    }
}

fn utf8_trimmed_or_default(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn truncate_for_error(message: String, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        return message;
    }

    let mut truncated = message.chars().take(max_chars).collect::<String>();
    truncated.push('…');
    truncated
}

struct RuntimeCandidate {
    label: &'static str,
    program: &'static str,
    launch_args: &'static [&'static str],
    probe_args: &'static [&'static str],
}

zed::register_extension!(JiraMcpExtension);
