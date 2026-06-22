use std::io::{BufRead, Write};

use anyhow::{Context, Result};

use crate::protocol::Response;

/// Read one newline-delimited JSON message from `reader`.
///
/// Per the MCP stdio transport spec (2025-11-25), each message is a single UTF-8
/// JSON object on its own line with no embedded newlines. Blank lines are
/// skipped. Returns `Ok(None)` on EOF, `Ok(Some(line))` on success.
pub fn read_message(reader: &mut impl BufRead) -> Result<Option<String>> {
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .context("reading message line")?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']).to_owned();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed));
        }
    }
}

/// Serialise `resp` as a compact single-line JSON object followed by `\n`.
///
/// Per the MCP stdio transport spec, each message must be a single
/// newline-terminated JSON object with no embedded newlines.
pub fn write_response(resp: &Response, writer: &mut impl Write) -> Result<()> {
    let body = serde_json::to_string(resp).context("serialising response")?;
    writeln!(writer, "{body}").context("writing response")?;
    writer.flush().context("flushing response")
}