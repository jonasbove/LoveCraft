//! Open folders written by the app's own writer (`create_stored_project`, `write_open_project`)
//! and read back with its reader, as the TypeScript server's tests make and check them.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use prosecraft_core::js::{self, JsValue};
use prosecraft_core::storage::stamp::create_stored_project;
use prosecraft_core::sync::memory::MemoryFs;
use prosecraft_core::sync::open_layout::{open_marker_text, read_open_project, write_open_project};
use prosecraft_mcp::env::SystemEnv;

pub const NOW: f64 = 1_700_000_000_000.0;

pub struct Root {
    _dir: tempfile::TempDir,
    pub path: String,
}

impl Root {
    pub fn doc_path(&self, id: &str) -> String {
        format!("{}/projects/project-1/documents/{id}.md", self.path)
    }
    pub fn project_path(&self) -> String {
        format!("{}/projects/project-1/project.json", self.path)
    }
    pub fn chapter_path(&self, id: &str) -> String {
        format!("{}/projects/project-1/chapters/{id}.json", self.path)
    }
    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(path).expect("read")
    }
    pub fn project_json(&self) -> JsValue {
        js::parse(&self.read(&self.project_path())).expect("project.json")
    }
    /// The `project` section of `project.json`, where snapshots and comments live.
    pub fn project_section(&self) -> JsValue {
        self.project_json()
            .get("project")
            .cloned()
            .expect("project")
    }
    /// The project as the app reads it back.
    pub fn reread(&self) -> JsValue {
        let mut memory = load(Path::new(&self.path));
        read_open_project(&mut memory, "project-1", NOW + 1_000.0)
            .expect("the app reads the folder")
    }
}

pub fn stored(project: &str) -> JsValue {
    create_stored_project(js::parse(project).expect("fixture"), "device-a", NOW)
}

/// Writes the project through the app's writer, then puts the files on disk.
pub fn make_root(stored: &JsValue) -> Root {
    let mut memory = MemoryFs::new();
    write_open_project(&mut memory, stored).expect("the app writes the folder");
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().to_string_lossy().into_owned();
    for (file, content) in memory.files() {
        let target = dir.path().join(file);
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::write(&target, content).expect("write");
    }
    fs::write(
        dir.path().join("prosecraft-folder.json"),
        open_marker_text(NOW),
    )
    .expect("marker");
    Root { _dir: dir, path }
}

/// Loads the folder back into memory so the app can decode it.
pub fn load(root: &Path) -> MemoryFs {
    let mut files = Vec::new();
    walk(root, root, &mut files);
    MemoryFs::with_files(files)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("relative")
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, fs::read_to_string(&path).expect("read")));
        }
    }
}

pub fn env() -> SystemEnv {
    SystemEnv
}

pub fn s(text: &str) -> prosecraft_core::js::JsString {
    prosecraft_core::js::JsString::from(text)
}

pub fn str_of(value: &JsValue, key: &str) -> String {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

pub fn num(value: &JsValue, key: &str) -> f64 {
    value.get(key).and_then(JsValue::as_number).expect("number")
}

pub fn ids(list: &JsValue) -> Vec<String> {
    list.as_array()
        .expect("list")
        .iter()
        .map(|v| str_of(v, "id"))
        .collect()
}

/// The document of `id` in a project as the app reads it.
pub fn document<'a>(project: &'a JsValue, id: &str) -> Option<&'a JsValue> {
    project
        .get("project")?
        .get("documents")?
        .as_array()?
        .iter()
        .find(|d| str_of(d, "id") == id)
}

pub fn meta<'a>(project: &'a JsValue, kind: &str, id: &str) -> &'a JsValue {
    project
        .get("entities")
        .and_then(|e| e.get(kind))
        .and_then(|m| m.get(id))
        .expect("meta")
}
