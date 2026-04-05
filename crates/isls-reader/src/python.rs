// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Regex-based Python parser for ISLS I4.
//!
//! Extracts top-level function definitions, class definitions, and import
//! statements from `.py` files. Uses line-by-line regex matching — no full
//! Python AST is required since Resonites only need top-level declarations.

use regex::Regex;
use crate::{FunctionDef, StructDef};
use crate::normalize::normalize_name;

// ─── Standard library module list ────────────────────────────────────────────

const PYTHON_STDLIB: &[&str] = &[
    "os", "sys", "re", "json", "math", "datetime",
    "collections", "itertools", "functools", "typing",
    "pathlib", "io", "abc", "enum", "dataclasses",
    "asyncio", "logging", "unittest", "hashlib",
    "urllib", "http", "socket", "threading",
    "multiprocessing", "subprocess", "shutil",
    "copy", "pickle", "csv", "xml", "html",
    "argparse", "configparser", "contextlib",
    "time", "struct", "binascii", "base64",
    "string", "textwrap", "pprint", "types",
    "weakref", "gc", "inspect", "importlib",
    "warnings", "traceback", "linecache",
    "operator", "random", "statistics", "decimal",
    "fractions", "numbers", "cmath", "heapq",
    "bisect", "array", "queue", "deque",
    "shelve", "dbm", "sqlite3", "zipfile",
    "tarfile", "gzip", "bz2", "lzma",
    "tempfile", "glob", "fnmatch", "fileinput",
    "stat", "filecmp", "os.path", "platform",
    "signal", "mmap", "select", "selectors",
    "asynchat", "asyncore", "email", "mailbox",
    "html.parser", "xml.etree", "xml.dom", "xml.sax",
];

/// Returns `true` when the top-level module name is a Python stdlib module.
pub fn is_stdlib_python(module: &str) -> bool {
    let top = module.split('.').next().unwrap_or(module);
    PYTHON_STDLIB.contains(&top)
}

/// Returns `true` when the import path refers to an external (non-stdlib) package.
pub fn is_external_python(module: &str) -> bool {
    !is_stdlib_python(module)
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Parse Python source and return (functions, structs/classes, imports).
///
/// Import strings have the form `"module.name"` for `from module import name`,
/// or `"module"` for `import module`. External imports are prefixed with `ext:`.
pub fn parse_python(source: &str) -> (Vec<FunctionDef>, Vec<StructDef>, Vec<String>) {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    let fn_re = Regex::new(r"^(\s*)(async\s+)?def\s+(\w+)\s*\(([^)]*)\)").unwrap();
    let class_re = Regex::new(r"^(\s*)class\s+(\w+)(?:\(([^)]*)\))?:").unwrap();
    let import_from_re = Regex::new(r"^\s*from\s+([\w.]+)\s+import\s+(.+)").unwrap();
    let import_re = Regex::new(r"^\s*import\s+([\w., ]+)").unwrap();
    let decorator_re = Regex::new(r"^\s*@(\w[\w.]*)").unwrap();
    let self_assign_re = Regex::new(r"^\s+self\.(\w+)\s*=").unwrap();
    let dc_field_re = Regex::new(r"^    (\w+)\s*[=:]").unwrap();

    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    let mut pending_decorators: Vec<String> = Vec::new();
    let mut is_dataclass = false;

    while i < lines.len() {
        let line = lines[i];

        // Decorator
        if let Some(cap) = decorator_re.captures(line) {
            let dec = cap[1].to_string();
            if dec == "dataclass" {
                is_dataclass = true;
            }
            pending_decorators.push(dec);
            i += 1;
            continue;
        }

        // Function definition
        if let Some(cap) = fn_re.captures(line) {
            let indent = cap[1].len();
            let is_async = cap.get(2).is_some();
            let raw_name = cap[3].to_string();
            let params_raw = cap[4].trim().to_string();

            let params: Vec<String> = if params_raw.is_empty() {
                vec![]
            } else {
                params_raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };

            // Exclude `self` and `cls` from arity count
            let arity_params: Vec<String> = params
                .iter()
                .filter(|p| {
                    let base = p.split(':').next().unwrap_or(p).trim();
                    base != "self" && base != "cls"
                })
                .cloned()
                .collect();

            // Top-level functions (indent==0) that don't start with '_' are public
            let is_public = indent == 0 && !raw_name.starts_with('_');
            let name = normalize_name(&raw_name);

            functions.push(FunctionDef {
                name,
                is_public,
                is_async,
                params: arity_params,
                return_type: None,
                line: i + 1,
            });
            pending_decorators.clear();
        }
        // Class definition
        else if let Some(cap) = class_re.captures(line) {
            let raw_name = cap[2].to_string();
            let name = raw_name.clone(); // class names stay PascalCase per spec

            let is_dc = is_dataclass;
            is_dataclass = false;

            // Determine field count by scanning ahead
            let mut field_count = 0usize;
            if is_dc {
                // dataclass: count annotated attributes at indent+4
                for k in (i + 1)..lines.len() {
                    let l = lines[k];
                    if l.trim().is_empty() { continue; }
                    let ind = l.len() - l.trim_start().len();
                    if ind == 0 { break; }
                    if dc_field_re.is_match(l) && !l.trim().starts_with("def ") {
                        field_count += 1;
                    }
                }
            } else {
                // regular class: find __init__ and count self.X = assignments
                let mut in_init = false;
                let mut init_indent = 0usize;
                for k in (i + 1)..lines.len() {
                    let l = lines[k];
                    if l.trim().is_empty() { continue; }
                    let ind = l.len() - l.trim_start().len();
                    if ind == 0 { break; } // back to top level
                    let trimmed = l.trim();
                    if !in_init {
                        if trimmed.starts_with("def __init__") {
                            in_init = true;
                            init_indent = ind;
                            continue;
                        }
                    } else {
                        // Leave __init__ when we see another def at same indent
                        if (trimmed.starts_with("def ") || trimmed.starts_with("async def "))
                            && ind <= init_indent
                        {
                            break;
                        }
                        if self_assign_re.is_match(l) {
                            field_count += 1;
                        }
                    }
                }
            }

            let derives: Vec<String> = pending_decorators.drain(..).collect();

            structs.push(StructDef {
                name,
                is_public: true,
                fields: (0..field_count).map(|j| format!("field_{}", j)).collect(),
                derives,
                line: i + 1,
            });
        }
        // from X import Y, Z
        else if let Some(cap) = import_from_re.captures(line) {
            let module = cap[1].to_string();
            let names_raw = cap[2].trim().to_string();
            let ext = is_external_python(&module);

            for name in names_raw.split(',') {
                let name = name.trim().trim_matches(|c: char| c == '(' || c == ')');
                let name = name.split(" as ").next().unwrap_or(name).trim();
                if !name.is_empty() && name != "*" {
                    let path = format!("{}.{}", module, name);
                    imports.push(if ext { format!("ext:{}", path) } else { path });
                }
            }
            pending_decorators.clear();
        }
        // import X
        else if let Some(cap) = import_re.captures(line) {
            let modules_raw = cap[1].trim().to_string();
            for module in modules_raw.split(',') {
                let module = module.trim().split(" as ").next().unwrap_or("").trim();
                if !module.is_empty() {
                    let ext = is_external_python(module);
                    imports.push(if ext {
                        format!("ext:{}", module)
                    } else {
                        module.to_string()
                    });
                }
            }
            pending_decorators.clear();
        } else {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                pending_decorators.clear();
            }
        }

        i += 1;
    }

    (functions, structs, imports)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
import os
import requests
from typing import Optional
from sqlalchemy.orm import Session

class User:
    def __init__(self, name: str, email: str, age: int):
        self.name = name
        self.email = email
        self.age = age

    def greet(self):
        return f"Hello, {self.name}"

async def get_user(session: Session, user_id: int) -> Optional[User]:
    return session.query(User).filter_by(id=user_id).first()

def create_user(session: Session, name: str, email: str) -> User:
    user = User(name=name, email=email, age=0)
    session.add(user)
    return user

def _internal_helper(x):
    return x
"#;

    #[test]
    fn py_functions() {
        let (fns, _, _) = parse_python(SAMPLE);
        let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"get_user"), "get_user not found: {:?}", names);
        assert!(names.contains(&"create_user"), "create_user not found: {:?}", names);
    }

    #[test]
    fn py_classes_field_count() {
        let (_, structs, _) = parse_python(SAMPLE);
        let user = structs.iter().find(|s| s.name == "User").expect("User class not found");
        assert_eq!(user.fields.len(), 3, "User should have 3 fields (name/email/age)");
    }

    #[test]
    fn py_imports_external() {
        let (_, _, imports) = parse_python(SAMPLE);
        assert!(imports.iter().any(|i| i.starts_with("ext:") && i.contains("requests")),
            "requests should be external: {:?}", imports);
        assert!(imports.iter().any(|i| !i.starts_with("ext:") && i.starts_with("os")),
            "os should be stdlib: {:?}", imports);
    }

    #[test]
    fn py_async_detection() {
        let (fns, _, _) = parse_python(SAMPLE);
        let get_user = fns.iter().find(|f| f.name == "get_user").expect("get_user not found");
        assert!(get_user.is_async, "get_user should be async");
        let create_user = fns.iter().find(|f| f.name == "create_user").expect("create_user not found");
        assert!(!create_user.is_async, "create_user should not be async");
    }

    #[test]
    fn py_visibility() {
        let (fns, _, _) = parse_python(SAMPLE);
        let public_fn = fns.iter().find(|f| f.name == "get_user").expect("get_user not found");
        assert!(public_fn.is_public, "get_user should be public");
        let internal = fns.iter().find(|f| f.name == "_internal_helper"
            || f.name == "internal_helper");
        // _internal_helper starts with _ → not public
        if let Some(f) = internal {
            assert!(!f.is_public, "_internal_helper should not be public");
        }
    }

    #[test]
    fn py_dataclass_fields() {
        let src = r#"
from dataclasses import dataclass

@dataclass
class Product:
    name: str
    price: float
    quantity: int
"#;
        let (_, structs, _) = parse_python(src);
        let p = structs.iter().find(|s| s.name == "Product").expect("Product not found");
        assert_eq!(p.fields.len(), 3, "Product should have 3 fields: {:?}", p.fields);
        assert!(p.derives.contains(&"dataclass".to_string()), "should track @dataclass");
    }
}
