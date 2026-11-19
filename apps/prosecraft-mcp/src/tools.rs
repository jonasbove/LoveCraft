//! The tools: their names, descriptions and input schemas as the TypeScript server lists them
//! (`mcp/src/index.ts`, upstream `8cdc694`; schemas as zod 4 writes them), the argument checks with zod's messages, and the
//! calls.

use prosecraft_core::js::{self, JsObject, JsString, JsValue, number_to_string};
use serde_json::{Map, Value, json};

use crate::env::Env;
use crate::read_tools;
use crate::safe_writes::{
    self, CommentInput, LabelTarget, WikiPageInput, WikiPageUpdate, WriteSceneInput,
};
use crate::search_tools::{self, SearchOptions, Space};
use crate::store::{self, ChapterPatch, ListFilter};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Str,
    Bool,
    Int { min: f64, max: Option<f64> },
    Enum(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug)]
struct Param {
    name: &'static str,
    kind: Kind,
    optional: bool,
    nullable: bool,
    description: Option<&'static str>,
}

const fn required(name: &'static str, kind: Kind, description: Option<&'static str>) -> Param {
    Param {
        name,
        kind,
        optional: false,
        nullable: false,
        description,
    }
}

const fn optional(name: &'static str, kind: Kind, description: Option<&'static str>) -> Param {
    Param {
        name,
        kind,
        optional: true,
        nullable: false,
        description,
    }
}

const STR: Kind = Kind::Str;
const BOOL: Kind = Kind::Bool;
const REVISION: Kind = Kind::Int {
    min: 0.0,
    max: None,
};
const PROJECT: Param = required("projectId", STR, None);

struct Tool {
    name: &'static str,
    description: &'static str,
    params: &'static [Param],
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "list_projects",
        description: "List ProseCraft projects in the open sync folder.",
        params: &[],
    },
    Tool {
        name: "list_chapters",
        description: "List chapters in a project, optionally filtered by label or Trash state.",
        params: &[
            required("projectId", STR, Some("Project id from list_projects")),
            optional("labelId", STR, Some("Only chapters with this label")),
            optional(
                "trashed",
                BOOL,
                Some("true lists only trashed chapters; omit for live ones"),
            ),
        ],
    },
    Tool {
        name: "list_scenes",
        description: "List scenes (documents) and Wiki pages in a project, optionally filtered by chapter, label or Trash state.",
        params: &[
            PROJECT,
            optional("chapterId", STR, Some("Only scenes in this chapter")),
            optional("labelId", STR, Some("Only scenes with this label")),
            optional(
                "trashed",
                BOOL,
                Some("true lists only trashed scenes; omit for live ones"),
            ),
        ],
    },
    Tool {
        name: "search_documents",
        description: "Search scene and Wiki page titles and bodies with the app's matching rules: literal query, diacritics folded unless caseSensitive, optional wholeWord. Returns one result per document with a short plain-text snippet around the first match.",
        params: &[
            PROJECT,
            required("query", STR, Some("Literal text to find")),
            optional(
                "space",
                Kind::Enum(&["scene", "wiki"]),
                Some("Only scenes or only Wiki pages"),
            ),
            optional("chapterId", STR, Some("Only scenes in this chapter")),
            optional("folderId", STR, Some("Only Wiki pages in this folder")),
            optional("labelId", STR, Some("Only documents with this label")),
            optional("includeTrashed", BOOL, Some("Search the Trash too")),
            optional(
                "caseSensitive",
                BOOL,
                Some("Match case and diacritics exactly"),
            ),
            optional("wholeWord", BOOL, Some("Match whole words only")),
            optional(
                "limit",
                Kind::Int {
                    min: 1.0,
                    max: Some(search_tools::MAX_SEARCH_LIMIT),
                },
                Some("Maximum number of results (default 30)"),
            ),
        ],
    },
    Tool {
        name: "read_scene",
        description: "Read one scene as Markdown with metadata; fields carries every frontmatter field as stored. Trashed scenes fail unless includeTrashed is set.",
        params: &[
            PROJECT,
            required("sceneId", STR, None),
            optional(
                "includeTrashed",
                BOOL,
                Some("Read the scene even if it is in the Trash"),
            ),
        ],
    },
    Tool {
        name: "list_wiki_pages",
        description: "List Wiki pages with title, folder path and kind; no bodies.",
        params: &[
            PROJECT,
            optional(
                "folderId",
                STR,
                Some("Only pages directly in this Wiki folder"),
            ),
        ],
    },
    Tool {
        name: "read_wiki_page",
        description: "Read one Wiki page: Markdown body plus every frontmatter field as fields.",
        params: &[PROJECT, required("pageId", STR, None)],
    },
    Tool {
        name: "list_wiki_folders",
        description: "List the Wiki folder tree.",
        params: &[PROJECT],
    },
    Tool {
        name: "write_scene",
        description: "Create or update a scene. Omit sceneId to create with a generated id. Only title, chapterId and the body change; every other frontmatter field is preserved. An unknown sceneId fails unless create is true. Overwriting an existing scene first stores its previous text as a snapshot unless snapshot is false; the result carries the snapshot id (null when none was taken). Pass expectedRevision to fail instead of overwriting a newer edit; the result carries the new revision.",
        params: &[
            PROJECT,
            optional("sceneId", STR, None),
            optional("chapterId", STR, None),
            optional("title", STR, None),
            required("markdown", STR, Some("Scene body in Markdown")),
            optional(
                "create",
                BOOL,
                Some("Create a new scene when sceneId is unknown"),
            ),
            optional(
                "snapshot",
                BOOL,
                Some("Set false to skip the snapshot before overwriting"),
            ),
            optional(
                "expectedRevision",
                REVISION,
                Some("Fail unless the stored scene revision equals this"),
            ),
        ],
    },
    Tool {
        name: "trash_scene",
        description: "Move a scene to Trash (tombstone + remove file). Pass expectedRevision to fail instead of trashing a newer edit; the result carries the new revision.",
        params: &[
            PROJECT,
            required("sceneId", STR, None),
            optional(
                "expectedRevision",
                REVISION,
                Some("Fail unless the stored scene revision equals this"),
            ),
        ],
    },
    Tool {
        name: "create_chapter",
        description: "Create a chapter.",
        params: &[
            PROJECT,
            required("title", STR, None),
            optional("summary", STR, None),
        ],
    },
    Tool {
        name: "update_chapter",
        description: "Rename a chapter or update its summary. Pass expectedRevision to fail instead of overwriting a newer edit; the result carries the new revision.",
        params: &[
            PROJECT,
            required("chapterId", STR, None),
            optional("title", STR, None),
            optional("summary", STR, None),
            optional(
                "expectedRevision",
                REVISION,
                Some("Fail unless the stored chapter revision equals this"),
            ),
        ],
    },
    Tool {
        name: "create_wiki_page",
        description: "Create a Wiki page (space: \"Wiki\"). folderId must be an existing Wiki folder, labelId an existing label. The result carries the new revision.",
        params: &[
            PROJECT,
            required("title", STR, None),
            optional("folderId", STR, Some("Wiki folder from list_wiki_folders")),
            optional("content", STR, Some("Page body in Markdown")),
            optional("labelId", STR, Some("Label from list_labels")),
        ],
    },
    Tool {
        name: "update_wiki_page",
        description: "Update a Wiki page. Omitted fields keep their stored value and every other frontmatter field is preserved. The previous text is stored as a snapshot first; the result carries the snapshot id and new revision. Pass expectedRevision to fail instead of overwriting a newer edit.",
        params: &[
            PROJECT,
            required("pageId", STR, None),
            optional("title", STR, None),
            optional("content", STR, Some("New page body in Markdown")),
            optional("folderId", STR, Some("Move the page to this Wiki folder")),
            optional("labelId", STR, Some("Label from list_labels")),
            optional(
                "expectedRevision",
                REVISION,
                Some("Fail unless the stored page revision equals this"),
            ),
        ],
    },
    Tool {
        name: "set_label",
        description: "Set or clear the label of a document or a chapter. labelId null clears it; an unknown labelId fails without writing.",
        params: &[
            PROJECT,
            optional("documentId", STR, Some("Scene or Wiki page id")),
            optional(
                "chapterId",
                STR,
                Some("Chapter id; pass either this or documentId"),
            ),
            Param {
                name: "labelId",
                kind: STR,
                optional: false,
                nullable: true,
                description: Some("Label from list_labels, or null to clear"),
            },
        ],
    },
    Tool {
        name: "add_comment",
        description: "Add a comment anchored to a quote in a document. The quote must occur exactly once in the plain text; prefix and suffix context are stored like the app does.",
        params: &[
            PROJECT,
            required("documentId", STR, None),
            required(
                "quote",
                STR,
                Some("Exact passage the comment is anchored to"),
            ),
            required("text", STR, Some("Comment body")),
            optional("author", STR, Some("Defaults to Me")),
        ],
    },
    Tool {
        name: "resolve_comment",
        description: "Mark a comment as resolved.",
        params: &[PROJECT, required("commentId", STR, None)],
    },
    Tool {
        name: "list_labels",
        description: "List project labels with the number of live documents and chapters using each.",
        params: &[PROJECT],
    },
    Tool {
        name: "list_comments",
        description: "List comments, optionally for one document. Resolved comments appear only with includeResolved.",
        params: &[
            PROJECT,
            optional("documentId", STR, Some("Only comments on this document")),
            optional("includeResolved", BOOL, Some("Include resolved comments")),
        ],
    },
    Tool {
        name: "list_snapshots",
        description: "List snapshot metadata, newest first; no text. Optionally only for one document.",
        params: &[
            PROJECT,
            optional("documentId", STR, Some("Only snapshots of this document")),
        ],
    },
    Tool {
        name: "read_snapshot",
        description: "Read one snapshot, including its stored text.",
        params: &[PROJECT, required("snapshotId", STR, None)],
    },
];

fn kind_schema(param: &Param) -> Map<String, Value> {
    let mut out = Map::new();
    match param.kind {
        Kind::Str if param.nullable => {
            out.insert("type".into(), json!(["string", "null"]));
        }
        Kind::Str => {
            out.insert("type".into(), json!("string"));
        }
        Kind::Bool => {
            out.insert("type".into(), json!("boolean"));
        }
        Kind::Int { min, max } => {
            out.insert("type".into(), json!("integer"));
            out.insert("minimum".into(), json_number(min));
            out.insert(
                "maximum".into(),
                json_number(max.unwrap_or(MAX_SAFE_INTEGER)),
            );
        }
        Kind::Enum(values) => {
            out.insert("type".into(), json!("string"));
            out.insert("enum".into(), json!(values));
        }
    }
    out
}

/// A whole number as JSON writes it (`1`, not `1.0`).
fn json_number(value: f64) -> Value {
    serde_json::from_str(&number_to_string(value)).unwrap_or(Value::Null)
}

/// One property as zod 4 writes it: a wrapped (optional or nullable) type puts its description
/// first.
fn param_schema(param: &Param) -> Value {
    let inner = kind_schema(param);
    let mut out = Map::new();
    let wrapped = param.optional || param.nullable;
    if wrapped && let Some(description) = param.description {
        out.insert("description".into(), json!(description));
    }
    out.extend(inner);
    if !wrapped && let Some(description) = param.description {
        out.insert("description".into(), json!(description));
    }
    Value::Object(out)
}

fn tool_definition(tool: &Tool) -> Value {
    let mut properties = Map::new();
    for param in tool.params {
        properties.insert(param.name.into(), param_schema(param));
    }
    let mut schema = Map::new();
    schema.insert(
        "$schema".into(),
        json!("http://json-schema.org/draft-07/schema#"),
    );
    schema.insert("type".into(), json!("object"));
    schema.insert("properties".into(), Value::Object(properties));
    let required: Vec<&str> = tool
        .params
        .iter()
        .filter(|p| !p.optional)
        .map(|p| p.name)
        .collect();
    if !required.is_empty() {
        schema.insert("required".into(), json!(required));
    }
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": Value::Object(schema),
        "execution": {"taskSupport": "forbidden"},
    })
}

/// Every tool's definition, as `tools/list` serves it.
pub fn definitions() -> Vec<Value> {
    TOOLS.iter().map(tool_definition).collect()
}

/// zod's name for the type of a value it did not expect.
fn received(value: Option<&JsValue>) -> &'static str {
    match value {
        None => "undefined",
        Some(JsValue::Null) => "null",
        Some(JsValue::Bool(_)) => "boolean",
        Some(JsValue::Number(n)) if n.is_nan() => "NaN",
        Some(JsValue::Number(_)) => "number",
        Some(JsValue::String(_)) => "string",
        Some(JsValue::Array(_)) => "array",
        Some(JsValue::Object(_)) => "object",
    }
}

/// The zod issues for one argument, in the order zod reports them.
fn check_param(param: &Param, value: Option<&JsValue>) -> Vec<String> {
    let invalid = |expected: &str| {
        vec![format!(
            "Invalid input: expected {expected}, received {}",
            received(value)
        )]
    };
    match value {
        None if param.optional => return Vec::new(),
        Some(JsValue::Null) if param.nullable => return Vec::new(),
        _ => {}
    }
    match param.kind {
        Kind::Str if matches!(value, Some(JsValue::String(_))) => Vec::new(),
        Kind::Str => invalid("string"),
        Kind::Bool if matches!(value, Some(JsValue::Bool(_))) => Vec::new(),
        Kind::Bool => invalid("boolean"),
        Kind::Enum(values) => {
            if jsx_str(value).is_some_and(|v| values.iter().any(|e| v.eq_str(e))) {
                Vec::new()
            } else {
                let listed: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
                vec![format!(
                    "Invalid option: expected one of {}",
                    listed.join("|")
                )]
            }
        }
        Kind::Int { min, max } => {
            let Some(JsValue::Number(n)) = value else {
                return invalid("number");
            };
            let n = *n;
            if !n.is_finite() || n.fract() != 0.0 {
                return invalid("int");
            }
            let mut issues = Vec::new();
            if n > MAX_SAFE_INTEGER {
                issues.push(format!(
                    "Too big: expected int to be <={}",
                    number_to_string(MAX_SAFE_INTEGER)
                ));
            }
            if n < -MAX_SAFE_INTEGER {
                issues.push(format!(
                    "Too small: expected int to be >={}",
                    number_to_string(-MAX_SAFE_INTEGER)
                ));
            }
            if n < min {
                issues.push(format!(
                    "Too small: expected number to be >={}",
                    number_to_string(min)
                ));
            }
            if let Some(max) = max
                && n > max
            {
                issues.push(format!(
                    "Too big: expected number to be <={}",
                    number_to_string(max)
                ));
            }
            issues
        }
    }
}

fn jsx_str(value: Option<&JsValue>) -> Option<&JsString> {
    value.and_then(JsValue::as_js_string)
}

/// A tool result: the value as `JSON.stringify(value, null, 2)`, or an error message.
fn result(outcome: Result<JsValue, String>) -> Value {
    match outcome {
        Ok(value) => craft_mcp_server::text_result(js::stringify_pretty(&value), false),
        Err(message) => craft_mcp_server::text_result(message, true),
    }
}

/// The arguments of one call, read as the tool's schema allows them.
struct Args<'a>(&'a JsObject);

impl Args<'_> {
    fn str(&self, key: &str) -> Option<JsString> {
        jsx_str(self.0.get(key)).cloned()
    }
    fn string(&self, key: &str) -> String {
        self.str(key)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    }
    fn lossy(&self, key: &str) -> Option<String> {
        self.str(key).map(|s| s.to_string_lossy())
    }
    fn bool(&self, key: &str) -> Option<bool> {
        match self.0.get(key) {
            Some(JsValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }
    fn number(&self, key: &str) -> Option<f64> {
        self.0.get(key).and_then(JsValue::as_number)
    }
}

/// The open folder and the clock and ids the tools write with.
pub struct ProseCraftTools {
    root: String,
    env: Box<dyn Env>,
}

impl ProseCraftTools {
    pub fn new(root: String, env: Box<dyn Env>) -> Self {
        Self { root, env }
    }

    /// Runs tool `name` with `args` (an object, or nothing): the argument check first, failing
    /// with zod's messages, then the tool.
    pub fn call_tool(&mut self, name: &str, args: Option<&JsObject>) -> Value {
        let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
            return craft_mcp_server::text_result(
                format!("MCP error -32602: Tool {name} not found"),
                true,
            );
        };
        let empty = JsObject::new();
        let args = args.unwrap_or(&empty);
        let issues: Vec<String> = tool
            .params
            .iter()
            .flat_map(|param| {
                check_param(param, args.get(param.name))
                    .into_iter()
                    .map(|issue| format!("{issue} at {}", param.name))
            })
            .collect();
        if !issues.is_empty() {
            return craft_mcp_server::text_result(
                format!(
                    "MCP error -32602: Input validation error: Invalid arguments for tool {name}: {}",
                    issues.join("\n")
                ),
                true,
            );
        }
        result(self.run(name, &Args(args)))
    }

    fn run(&mut self, name: &str, args: &Args<'_>) -> Result<JsValue, String> {
        let root = self.root.as_str();
        let env = self.env.as_mut();
        let project = args.string("projectId");
        let project = project.as_str();
        match name {
            "list_projects" => Ok(JsValue::Array(
                store::list_projects(root)?
                    .iter()
                    .map(store::ProjectEntry::to_js)
                    .collect(),
            )),
            "list_chapters" => {
                let filter = ListFilter {
                    label_id: args.lossy("labelId"),
                    trashed: args.bool("trashed"),
                    chapter_id: None,
                };
                Ok(JsValue::Array(store::list_chapters(
                    root, project, &filter,
                )?))
            }
            "list_scenes" => list_scenes(root, project, args),
            "search_documents" => {
                let options = SearchOptions {
                    space: match args.lossy("space").as_deref() {
                        Some("scene") => Some(Space::Scene),
                        Some("wiki") => Some(Space::Wiki),
                        _ => None,
                    },
                    chapter_id: args.lossy("chapterId"),
                    folder_id: args.lossy("folderId"),
                    label_id: args.lossy("labelId"),
                    include_trashed: args.bool("includeTrashed") == Some(true),
                    case_sensitive: args.bool("caseSensitive") == Some(true),
                    whole_word: args.bool("wholeWord") == Some(true),
                    limit: args.number("limit"),
                };
                let query = args.str("query").unwrap_or_default();
                search_tools::search_documents(root, project, &query, &options)
            }
            "read_scene" => store::read_scene(
                root,
                project,
                &args.string("sceneId"),
                args.bool("includeTrashed") == Some(true),
            ),
            "list_wiki_pages" => {
                read_tools::list_wiki_pages(root, project, args.lossy("folderId").as_deref())
            }
            "read_wiki_page" => read_tools::read_wiki_page(root, project, &args.string("pageId")),
            "list_wiki_folders" => read_tools::list_wiki_folders(root, project),
            "write_scene" => {
                let input = WriteSceneInput {
                    scene_id: args.str("sceneId"),
                    chapter_id: args.str("chapterId"),
                    title: args.str("title"),
                    markdown: args.str("markdown").unwrap_or_default(),
                    create: args.bool("create"),
                    snapshot: args.bool("snapshot"),
                    expected_revision: args.number("expectedRevision"),
                };
                safe_writes::write_scene(root, env, project, &input)
            }
            "trash_scene" => store::trash_scene(
                root,
                env,
                project,
                &args.string("sceneId"),
                args.number("expectedRevision"),
            ),
            "create_chapter" => store::create_chapter(
                root,
                env,
                project,
                &args.str("title").unwrap_or_default(),
                args.str("summary").as_ref(),
            ),
            "update_chapter" => {
                let patch = ChapterPatch {
                    title: args.str("title"),
                    summary: args.str("summary"),
                    expected_revision: args.number("expectedRevision"),
                };
                store::update_chapter(root, env, project, &args.string("chapterId"), &patch)
            }
            "create_wiki_page" => {
                let input = WikiPageInput {
                    title: args.str("title").unwrap_or_default(),
                    folder_id: args.str("folderId"),
                    content: args.str("content"),
                    label_id: args.str("labelId"),
                };
                safe_writes::create_wiki_page(root, env, project, &input)
            }
            "update_wiki_page" => {
                let input = WikiPageUpdate {
                    page_id: args.str("pageId").unwrap_or_default(),
                    title: args.str("title"),
                    content: args.str("content"),
                    folder_id: args.str("folderId"),
                    label_id: args.str("labelId"),
                    expected_revision: args.number("expectedRevision"),
                };
                safe_writes::update_wiki_page(root, env, project, &input)
            }
            "set_label" => {
                let target = match (args.str("documentId"), args.str("chapterId")) {
                    (Some(document), None) => LabelTarget::Document(document),
                    (None, Some(chapter)) => LabelTarget::Chapter(chapter),
                    _ => return Err("Pass exactly one of documentId or chapterId.".into()),
                };
                safe_writes::set_label(root, env, project, &target, args.str("labelId").as_ref())
            }
            "add_comment" => {
                let input = CommentInput {
                    document_id: args.str("documentId").unwrap_or_default(),
                    quote: args.str("quote").unwrap_or_default(),
                    text: args.str("text").unwrap_or_default(),
                    author: args.str("author"),
                };
                safe_writes::add_comment(root, env, project, &input)
            }
            "resolve_comment" => {
                safe_writes::resolve_comment(root, env, project, &args.string("commentId"))
            }
            "list_labels" => read_tools::list_labels(root, project),
            "list_comments" => read_tools::list_comments(
                root,
                project,
                args.lossy("documentId").as_deref(),
                args.bool("includeResolved") == Some(true),
            ),
            "list_snapshots" => {
                read_tools::list_snapshots(root, project, args.lossy("documentId").as_deref())
            }
            "read_snapshot" => read_tools::read_snapshot(root, project, &args.string("snapshotId")),
            other => Err(format!("MCP error -32602: Tool {other} not found")),
        }
    }
}

/// `list_scenes`, slimmed to what the listing shows.
fn list_scenes(root: &str, project: &str, args: &Args<'_>) -> Result<JsValue, String> {
    let filter = ListFilter {
        label_id: args.lossy("labelId"),
        trashed: args.bool("trashed"),
        chapter_id: args.lossy("chapterId"),
    };
    let scenes = store::list_scenes(root, project, &filter)?;
    Ok(JsValue::Array(
        scenes
            .into_iter()
            .map(|scene| {
                let mut out = JsObject::new();
                out.insert("id", JsValue::str(&scene.id));
                out.insert("title", scene.title);
                out.insert(
                    "space",
                    JsValue::str(if scene.wiki { "wiki" } else { "scene" }),
                );
                if let Some(chapter) = scene.chapter_id {
                    out.insert("chapterId", JsValue::String(chapter));
                }
                out.insert("folderId", scene.folder_id);
                out.insert("labelId", scene.label_id);
                out.insert("trashed", JsValue::Bool(scene.trashed));
                for key in ["updatedAt", "revision"] {
                    if let Some(value) = crate::jsx::get(&scene.meta, key) {
                        out.insert(key, value.clone());
                    }
                }
                let chars = scene.markdown.encode_utf16().count();
                out.insert("chars", JsValue::Number(chars as f64));
                JsValue::Object(out)
            })
            .collect(),
    ))
}

/// A JSON-RPC value as a JavaScript one (through its text, so nothing is lost).
pub fn to_js(value: &Value) -> Option<JsValue> {
    js::parse(&value.to_string()).ok()
}

impl craft_mcp_server::Tools for ProseCraftTools {
    fn instructions(&self) -> String {
        "Reads and edits the ProseCraft open sync folder named by PROSECRAFT_OPEN_DIR: projects, chapters, scenes, Wiki pages, labels, comments and snapshots.".into()
    }

    fn list(&mut self) -> Vec<Value> {
        definitions()
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        match to_js(args) {
            Some(JsValue::Object(args)) => self.call_tool(name, Some(&args)),
            _ => self.call_tool(name, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues(tool: &str, args: &str) -> String {
        let mut tools =
            ProseCraftTools::new("/nonexistent".into(), Box::new(crate::env::SystemEnv));
        let args = js::parse(args).expect("json");
        let out = tools.call_tool(tool, args.as_object());
        out["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn argument_errors_read_like_zod() {
        assert_eq!(
            issues("list_chapters", r#"{"projectId":5}"#),
            "MCP error -32602: Input validation error: Invalid arguments for tool list_chapters: Invalid input: expected string, received number at projectId"
        );
        assert_eq!(
            issues(
                "search_documents",
                r#"{"projectId":"x","query":"a","limit":1000}"#
            ),
            "MCP error -32602: Input validation error: Invalid arguments for tool search_documents: Too big: expected number to be <=100 at limit"
        );
        assert_eq!(
            issues(
                "search_documents",
                r#"{"projectId":"x","query":"a","limit":1e300,"space":null}"#
            ),
            "MCP error -32602: Input validation error: Invalid arguments for tool search_documents: Invalid option: expected one of \"scene\"|\"wiki\" at space\nToo big: expected int to be <=9007199254740991 at limit\nToo big: expected number to be <=100 at limit"
        );
        assert_eq!(
            issues("set_label", r#"{"projectId":"x","documentId":"d"}"#),
            "MCP error -32602: Input validation error: Invalid arguments for tool set_label: Invalid input: expected string, received undefined at labelId"
        );
        assert_eq!(
            issues(
                "write_scene",
                r#"{"projectId":"x","markdown":"m","expectedRevision":1.5}"#
            ),
            "MCP error -32602: Input validation error: Invalid arguments for tool write_scene: Invalid input: expected int, received number at expectedRevision"
        );
        assert_eq!(
            issues(
                "write_scene",
                r#"{"projectId":"x","markdown":"m","expectedRevision":-1}"#
            ),
            "MCP error -32602: Input validation error: Invalid arguments for tool write_scene: Too small: expected number to be >=0 at expectedRevision"
        );
        assert_eq!(
            issues("nope", "{}"),
            "MCP error -32602: Tool nope not found"
        );
    }

    #[test]
    fn null_clears_a_label_and_unknown_arguments_are_ignored() {
        // Both pass the check and reach the tool, which fails on the missing folder (a write
        // takes the lock first, as in the TypeScript server).
        assert_eq!(
            issues(
                "set_label",
                r#"{"projectId":"x","documentId":"d","labelId":null}"#
            ),
            "ENOENT: no such file or directory, open '/nonexistent/.prosecraft-mcp.lock'"
        );
        assert_eq!(
            issues("list_chapters", r#"{"projectId":"x","extra":1}"#),
            "Not a ProseCraft open folder: /nonexistent"
        );
        assert_eq!(
            issues("set_label", r#"{"projectId":"x","labelId":null}"#),
            "Pass exactly one of documentId or chapterId."
        );
    }
}
