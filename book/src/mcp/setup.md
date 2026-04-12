# MCP Tooling Setup

`papertowel` provides a Model Context Protocol (MCP) server that allows AI assistants (like Claude Desktop) to interact directly with your codebase's AI fingerprints.

## The `papertowel-mcp` Server

The MCP server exposes the core functionality of the Scrubber as a set of tools that the AI can call. This allows the AI to "self-diagnose" its own fingerprints and suggest fixes.

### Available Tools

| Tool | Description |
| :--- | :--- |
| `papertowel_scan` | Scans a directory for AI fingerprints and returns a structured report of findings. |
| `papertowel_scrub` | Applies fixes to the detected fingerprints in a target directory. |

## Installation

### 1. Build the Server

First, build the MCP server binary:

```bash
cargo build --release -p papertowel-mcp
```

### 2. Configure Claude Desktop

Add the server to your `claude_desktop_config.json` (usually located at `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

```json
{
  "mcpServers": {
    "papertowel": {
      "type": "stdio",
      "command": "papertowel-mcp",
      "args": [],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

## Usage in Chat

Once configured, you can simply ask Claude to clean up your code:

- *"Scan my current directory for AI fingerprints and tell me what you find."*
- *"Run the papertowel scrubber on the `src/` directory to remove any obvious slop."*

The AI will call the `papertowel_scan` and `papertowel_scrub` tools, receive the results, and report back to you.
