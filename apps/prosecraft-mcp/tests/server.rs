//! The server over newline-delimited JSON-RPC: the tool list the TypeScript server serves, tool
//! calls with zod's argument rules, and results as `JSON.stringify(value, null, 2)`.

mod common;

use common::{make_root, stored};
use prosecraft_mcp::env::SystemEnv;
use prosecraft_mcp::{ProseCraftTools, serve};
use serde_json::{Value, json};

fn run(root: &str, requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let mut output = Vec::new();
    let tools = ProseCraftTools::new(root.to_owned(), Box::new(SystemEnv));
    serve(tools, input.as_bytes(), &mut output).expect("serve");
    String::from_utf8(output)
        .expect("utf-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect()
}

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

const PROJECT: &str = r#"{"id":"project-1","name":"Novel","documents":[{"id":"doc-1","title":"One","content":"<p>Hi.</p>","chapterId":"chapter-1","labelId":"label-1"}],
"chapters":[{"id":"chapter-1","title":"Chapter One","expanded":true}],"labels":[{"id":"label-1","name":"Plot","color":"red"}]}"#;

#[test]
fn lists_the_tools_exactly_as_the_typescript_server() {
    let golden: Value = serde_json::from_str(include_str!("fixtures/tools-list.json"))
        .expect("fixture from the TypeScript server");
    let replies = run(
        "/nonexistent",
        &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})],
    );
    assert_eq!(replies[0]["result"], golden);
}

#[test]
fn serves_tool_calls_with_the_typescript_results() {
    let root = make_root(&stored(PROJECT));
    let replies = run(
        &root.path,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            call(2, "list_projects", json!({})),
            call(
                3,
                "set_label",
                json!({"projectId": "project-1", "documentId": "doc-1", "labelId": null, "extra": true}),
            ),
            call(4, "list_chapters", json!({"projectId": 5})),
            call(5, "nope", json!({})),
            call(
                6,
                "read_scene",
                json!({"projectId": "project-1", "sceneId": "missing"}),
            ),
        ],
    );
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "prosecraft");
    assert_eq!(
        replies[1]["result"]["content"][0]["text"],
        "[\n  {\n    \"id\": \"project-1\",\n    \"name\": \"Novel\",\n    \"dirName\": \"project-1\"\n  }\n]"
    );
    assert!(replies[1]["result"].get("isError").is_none());
    assert_eq!(
        replies[2]["result"]["content"][0]["text"],
        "{\n  \"documentId\": \"doc-1\",\n  \"labelId\": null,\n  \"revision\": 2\n}"
    );
    assert_eq!(
        replies[3]["result"],
        json!({"content": [{"type": "text", "text": "MCP error -32602: Input validation error: Invalid arguments for tool list_chapters: Invalid input: expected string, received number at projectId"}], "isError": true})
    );
    assert_eq!(
        replies[4]["result"]["content"][0]["text"],
        "MCP error -32602: Tool nope not found"
    );
    assert_eq!(
        replies[5]["result"],
        json!({"content": [{"type": "text", "text": "Scene not found: missing"}], "isError": true})
    );
    assert!(!root.read(&root.doc_path("doc-1")).contains("labelId"));
}
