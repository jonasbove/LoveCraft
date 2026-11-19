//! prosecraft-mcp: a local MCP server over a ProseCraft open sync folder, on stdio.
//!
//! A port of ProseCraft's TypeScript server (`mcp/`, upstream `8cdc694`): the same tools, argument names,
//! schemas, results, error messages and on-disk effects, so either server can serve the same
//! folder and each excludes the other while it writes. `craft-mcp-server` speaks the protocol;
//! calls to tools go through [`serve`] so their arguments are checked as the TypeScript server's
//! zod schemas check them.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod env;
pub mod fsx;
pub mod jsx;
pub mod read_tools;
pub mod safe_writes;
pub mod search_tools;
pub mod store;
pub mod tools;

use std::io::{BufRead, Write};

use craft_mcp_server::{Incoming, Info, Server, read_message, response};
use prosecraft_core::js::{self, JsValue};
use serde_json::{Value, json};

pub use tools::ProseCraftTools;

/// Who the server says it is, as the TypeScript server does ("prosecraft" 0.1.0).
pub const INFO: Info = Info {
    name: "prosecraft",
    title: "prosecraft-rs",
    version: "0.1.0",
    // A write carries a whole scene; this leaves room for a long novel chapter many times over.
    max_line: 64 * 1024 * 1024,
};

/// The open folder from `PROSECRAFT_OPEN_DIR`, trimmed (`resolveRoot`).
pub fn resolve_root(value: Option<&str>) -> Result<String, String> {
    let root = value.map(jsx::trim).unwrap_or_default();
    if root.is_empty() {
        return Err("Set PROSECRAFT_OPEN_DIR to your open-folder sync directory.".into());
    }
    Ok(root.to_owned())
}

/// A `tools/call` request handled here rather than by the shared server, whose argument check
/// refuses `null` for a required argument (`set_label` clears with `labelId: null`) and unknown
/// arguments (zod strips them), and answers a bad argument with a protocol error rather than the
/// tool error the TypeScript server returns.
fn tool_call(server: &mut Server<ProseCraftTools>, message: &JsValue) -> Option<Value> {
    let object = message.as_object()?;
    if !object
        .get("method")
        .is_some_and(|m| m.as_js_string().is_some_and(|m| m.eq_str("tools/call")))
    {
        return None;
    }
    let id = object.get("id")?;
    let id: Value = serde_json::from_str(&js::stringify(id)).unwrap_or(Value::Null);
    let params = object.get("params");
    let error = |message: String| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": message}});
    let Some(name) = params
        .and_then(|p| p.get("name"))
        .and_then(JsValue::as_js_string)
        .map(|n| n.to_string_lossy())
    else {
        return Some(error("missing tool `name`".into()));
    };
    let args = match params.and_then(|p| p.get("arguments")) {
        None | Some(JsValue::Null) => None,
        Some(JsValue::Object(args)) => Some(args),
        Some(_) => return Some(error(format!("{name}: arguments must be an object"))),
    };
    Some(response(id.clone(), server.tools().call_tool(&name, args)))
}

/// One line in, at most one line out.
pub fn handle_line(server: &mut Server<ProseCraftTools>, line: &str) -> Option<String> {
    if let Ok(message) = js::parse(line.trim())
        && let Some(reply) = tool_call(server, &message)
    {
        return Some(reply.to_string());
    }
    server.handle_line(line)
}

/// Serves newline-delimited JSON-RPC from `input` until it closes.
pub fn serve(
    tools: ProseCraftTools,
    mut input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let mut server = Server::new(INFO, tools);
    while let Some(incoming) = read_message(&mut input, INFO.max_line)? {
        let reply = match incoming {
            Incoming::Line(line) => handle_line(&mut server, &line),
            Incoming::Refused(reply) => Some(reply),
        };
        if let Some(reply) = reply {
            output.write_all(reply.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
