# Jira MCP Server for Zed

Rust-based Zed extension that launches the upstream [`mcp-atlassian`](https://github.com/sooperset/mcp-atlassian) server for Jira/Confluence.

## What this extension does

- Registers a Zed context server with ID `jira-mcp`
- Resolves runtime in this order:
  1. `uvx mcp-atlassian`
  2. `uv tool run mcp-atlassian`
  3. `python3 -m mcp_atlassian` (or `python`)
- Reads credentials from Zed settings and injects them into command env
- Returns descriptive errors with captured `stderr` when launch preflight fails

## Install as a dev extension

Before installing, ensure Rust is installed via **rustup** and WASM target exists:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable
rustup target add wasm32-wasip1
```

1. Open Zed.
2. Run `zed: install dev extension`.
3. Select this directory: `jira-mcp-server-zed-ide`.

## Runtime prerequisites

Preferred:

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

Fallback:

```bash
python3 -m pip install --user mcp-atlassian
```

## Zed configuration

Set credentials in Zed settings:

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

For Jira Server/Data Center, prefer `JIRA_PERSONAL_TOKEN`.

## Optional explicit command override

You can provide a custom command path/args via Zed settings:

```json
{
  "context_servers": {
    "jira-mcp": {
      "command": {
        "path": "uvx",
        "arguments": ["mcp-atlassian"],
        "env": {
          "JIRA_URL": "https://your-company.atlassian.net"
        }
      }
    }
  }
}
```

If `command.path` is set without `arguments`, the extension auto-inferrs args for `uvx`, `uv`, and `python/python3`.
