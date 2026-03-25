// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Type-aware context collector for LLM enrichment (ISLS v2.2).
//!
//! After Pass 0 emits the file skeletons, `TypeContext::from_output_dir` reads
//! the generated model, error, pagination and auth files so that subsequent LLM
//! prompts receive exact field names, function signatures, and error constructors
//! instead of invented ones.

use std::collections::HashMap;
use std::path::Path;

// ─── TypeContext ───────────────────────────────────────────────────────────────

/// Exact type definitions extracted from a generated output directory.
///
/// Populated after Pass 0 (Structure) and injected into every LLM prompt that
/// touches the services layer or auth routes.
#[derive(Clone, Debug, Default)]
pub struct TypeContext {
    /// Per-entity struct definitions.
    pub models: Vec<ModelDef>,
    /// The `AppError` enum definition (full source, trimmed).
    pub error_enum: String,
    /// `PaginationParams` and `PaginatedResponse` struct definitions.
    pub pagination_types: String,
    /// `AuthUser` and `Claims` struct definitions.
    pub auth_types: String,
    /// Database query function signatures, keyed by entity name (lower-snake).
    pub query_signatures: HashMap<String, Vec<String>>,
}

impl TypeContext {
    /// Build a `TypeContext` by scanning `output_dir` for generated source files.
    ///
    /// Reads:
    /// - `backend/src/errors.rs` → `AppError` enum
    /// - `backend/src/pagination.rs` → `PaginationParams`, `PaginatedResponse`
    /// - `backend/src/auth.rs` → `AuthUser`, `Claims`
    /// - `backend/src/models/*.rs` → entity structs
    /// - `backend/src/database/*_queries.rs` → query signatures
    pub fn from_output_dir(output_dir: &Path) -> Self {
        let mut ctx = TypeContext::default();

        let backend = output_dir.join("backend").join("src");

        // ── Errors ──────────────────────────────────────────────────────────
        let errors_path = backend.join("errors.rs");
        if let Ok(content) = std::fs::read_to_string(&errors_path) {
            ctx.error_enum = extract_enum_definition(&content, "AppError");
        }

        // ── Pagination ───────────────────────────────────────────────────────
        let pagination_path = backend.join("pagination.rs");
        if let Ok(content) = std::fs::read_to_string(&pagination_path) {
            let params = extract_struct_full(&content, "PaginationParams");
            let resp   = extract_struct_full(&content, "PaginatedResponse");
            ctx.pagination_types = format!("{}\n\n{}", params, resp);
        }

        // ── Auth ─────────────────────────────────────────────────────────────
        let auth_path = backend.join("auth.rs");
        if let Ok(content) = std::fs::read_to_string(&auth_path) {
            let user   = extract_struct_full(&content, "AuthUser");
            let claims = extract_struct_full(&content, "Claims");
            ctx.auth_types = format!("{}\n\n{}", user, claims);
        }

        // ── Models ────────────────────────────────────────────────────────────
        let models_dir = backend.join("models");
        if let Ok(entries) = std::fs::read_dir(&models_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let stem = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if stem == "mod" {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // entity name is PascalCase of the stem
                    let entity = to_pascal_case(&stem);
                    let main_struct     = extract_struct_full(&content, &entity);
                    let create_payload  = extract_struct_full(&content, &format!("Create{}Payload", entity));
                    let update_payload  = extract_struct_full(&content, &format!("Update{}Payload", entity));
                    let validation_impl = extract_impl_validate(&content, &entity);
                    ctx.models.push(ModelDef {
                        entity_name: entity,
                        main_struct,
                        create_payload,
                        update_payload,
                        validation_impl,
                    });
                }
            }
        }

        // ── Query signatures ─────────────────────────────────────────────────
        let database_dir = backend.join("database");
        if let Ok(entries) = std::fs::read_dir(&database_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !name.ends_with("_queries.rs") {
                    continue;
                }
                let entity = name.trim_end_matches("_queries.rs").to_string();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    ctx.query_signatures.insert(entity, extract_fn_signatures(&content));
                }
            }
        }

        ctx
    }

    /// Generate a type-context section for injection into an LLM prompt for
    /// the given entity.
    ///
    /// The returned string should be prepended to the prompt so the model sees
    /// exact types instead of guessing.
    pub fn prompt_for_entity(&self, entity_name: &str) -> String {
        let mut parts = Vec::new();

        // Find matching model def (case-insensitive)
        let model = self.models.iter().find(|m| {
            m.entity_name.eq_ignore_ascii_case(entity_name)
        });

        parts.push("// === TYPE CONTEXT (do not modify — injected by ISLS) ===".to_string());

        if let Some(m) = model {
            if !m.main_struct.is_empty() {
                parts.push(format!("// --- {} struct ---\n{}", m.entity_name, m.main_struct));
            }
            if !m.create_payload.is_empty() {
                parts.push(format!("// --- Create{}Payload ---\n{}", m.entity_name, m.create_payload));
            }
            if !m.update_payload.is_empty() {
                parts.push(format!("// --- Update{}Payload ---\n{}", m.entity_name, m.update_payload));
            }
            if !m.validation_impl.is_empty() {
                parts.push(format!("// --- Validation ---\n{}", m.validation_impl));
            }
        }

        if !self.error_enum.is_empty() {
            parts.push(format!("// --- AppError enum ---\n{}", self.error_enum));
        }
        if !self.pagination_types.is_empty() {
            parts.push(format!("// --- Pagination ---\n{}", self.pagination_types));
        }
        if !self.auth_types.is_empty() {
            parts.push(format!("// --- Auth types ---\n{}", self.auth_types));
        }

        // Query sigs for this entity
        let entity_key = entity_name.to_lowercase();
        if let Some(sigs) = self.query_signatures.get(&entity_key) {
            if !sigs.is_empty() {
                parts.push(format!("// --- {}_queries.rs signatures ---", entity_key));
                for sig in sigs {
                    parts.push(format!("// {}", sig));
                }
            }
        }

        parts.push("// === RULES ===".to_string());
        parts.push("// - Use tracing::info!/warn!/error! — NEVER use log::".to_string());
        parts.push("// - AppError::ValidationError takes Vec<String>".to_string());
        parts.push("// - Use exact field names from the structs above".to_string());
        parts.push("// - Compare Option<T> fields with .is_some()/.is_none(), not as T".to_string());
        parts.push("// === END TYPE CONTEXT ===".to_string());

        parts.join("\n")
    }
}

// ─── ModelDef ─────────────────────────────────────────────────────────────────

/// Per-entity type definitions extracted from generated model files.
#[derive(Clone, Debug, Default)]
pub struct ModelDef {
    /// Entity name in PascalCase (e.g. `"Product"`).
    pub entity_name: String,
    /// Full `struct <Entity> { … }` definition with derives.
    pub main_struct: String,
    /// Full `struct Create<Entity>Payload { … }` definition.
    pub create_payload: String,
    /// Full `struct Update<Entity>Payload { … }` definition.
    pub update_payload: String,
    /// `impl <Entity> { fn validate … }` block.
    pub validation_impl: String,
}

// ─── Enrichment gating ────────────────────────────────────────────────────────

/// Returns `true` when `path` should be sent to the LLM for enrichment.
///
/// Only service files (excluding `mod.rs`) and the auth-routes file are
/// enriched; all other paths are protected (template output is authoritative).
pub fn should_enrich(path: &str) -> bool {
    // Services layer (but not mod.rs)
    if path.contains("services/") && !path.ends_with("mod.rs") {
        return true;
    }
    // Auth routes
    if path.contains("auth_routes") || path.ends_with("auth.rs") && path.contains("api/") {
        return true;
    }
    false
}

// ─── Enrichment validation ────────────────────────────────────────────────────

/// Validates an LLM-enriched function before it is written back to a file.
///
/// Returns `true` only when all checks pass; callers should keep the original
/// on failure.
pub fn validate_enriched_function(code: &str, ctx: &TypeContext, entity_name: &str) -> bool {
    // 1. Must not be empty
    if code.trim().is_empty() {
        return false;
    }
    // 2. No markdown fences
    if code.contains("```") || code.contains("~~~") {
        return false;
    }
    // 3. Balanced braces
    let open  = code.matches('{').count();
    let close = code.matches('}').count();
    if open != close {
        return false;
    }
    // 4. Must contain `fn `
    if !code.contains("fn ") {
        return false;
    }
    // 5. Must not use log:: crate
    if code.contains("use log::") || code.contains("log::info!") || code.contains("log::warn!") {
        return false;
    }
    // 6. No hallucinated field names — check against known entity fields
    if let Some(model) = ctx.models.iter().find(|m| {
        m.entity_name.eq_ignore_ascii_case(entity_name)
    }) {
        let known = extract_field_names(&model.main_struct);
        // Skip validation if we couldn't parse any fields (empty context)
        if !known.is_empty() {
            // Check each `.field_name` access in code
            for access in extract_field_accesses(code) {
                // Skip common non-entity fields
                let skip = matches!(
                    access.as_str(),
                    "pool" | "params" | "user" | "id" | "inner" | "extensions" | "data"
                        | "query" | "path" | "state" | "body" | "headers" | "status"
                        | "message" | "errors" | "items" | "total" | "page" | "per_page"
                        | "offset" | "limit" | "email" | "password" | "role" | "sub"
                        | "exp" | "iat" | "name" | "is_active"
                );
                if !skip && !known.contains(&access) {
                    tracing::debug!(field = %access, entity = %entity_name, "hallucinated field rejected");
                    return false;
                }
            }
        }
    }
    true
}

// ─── Function-level replacement ───────────────────────────────────────────────

/// Replace a single named function in `file_content` with `new_function`.
///
/// Finds the function by scanning for `fn <name>`, counts curly braces to
/// locate the closing `}`, walks back over any preceding doc comments, and
/// splices the new function in.  Returns the original content unchanged if the
/// function is not found.
pub fn replace_function_in_file(file_content: &str, function_name: &str, new_function: &str) -> String {
    let search = format!("fn {}(", function_name);
    let fn_pos = match file_content.find(&search) {
        Some(p) => p,
        None => return file_content.to_string(),
    };

    // Walk back to the start of the line (to include visibility modifier / pub)
    let start_of_line = file_content[..fn_pos]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);

    // Walk back further over doc comments (lines starting with `///` or `//`)
    let block_start = walk_back_over_comments(file_content, start_of_line);

    // Find the end of the function body by counting braces
    let body_start = match file_content[fn_pos..].find('{') {
        Some(p) => fn_pos + p,
        None => return file_content.to_string(),
    };

    let mut depth = 0usize;
    let mut end = body_start;
    for (i, ch) in file_content[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    format!(
        "{}{}{}",
        &file_content[..block_start],
        new_function,
        &file_content[end..]
    )
}

// ─── String-based helpers ─────────────────────────────────────────────────────

/// Extract a complete `struct <name> { … }` block (including derives) from Rust source.
///
/// Includes attribute lines (`#[…]`) immediately before the struct keyword.
pub fn extract_struct_full(content: &str, name: &str) -> String {
    let search = format!("struct {}", name);
    let struct_pos = match content.find(&search) {
        Some(p) => p,
        None => return String::new(),
    };

    // Walk back to collect attribute/derive lines
    let pre = &content[..struct_pos];
    let block_start = walk_back_over_attrs(content, struct_pos, pre);

    // Find opening brace
    let brace_start = match content[struct_pos..].find('{') {
        Some(p) => struct_pos + p,
        None => return String::new(),
    };

    // Count braces to find end
    let mut depth = 0usize;
    let mut end = brace_start;
    for (i, ch) in content[brace_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    content[block_start..end].trim().to_string()
}

/// Extract a complete `enum <name> { … }` block from Rust source.
pub fn extract_enum_definition(content: &str, name: &str) -> String {
    let search = format!("enum {}", name);
    let enum_pos = match content.find(&search) {
        Some(p) => p,
        None => return String::new(),
    };

    let block_start = walk_back_over_attrs(content, enum_pos, &content[..enum_pos]);

    let brace_start = match content[enum_pos..].find('{') {
        Some(p) => enum_pos + p,
        None => return String::new(),
    };

    let mut depth = 0usize;
    let mut end = brace_start;
    for (i, ch) in content[brace_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    content[block_start..end].trim().to_string()
}

/// Extract an `impl <Entity> { fn validate … }` block from Rust source.
fn extract_impl_validate(content: &str, entity: &str) -> String {
    let search = format!("impl {}", entity);
    let impl_pos = match content.find(&search) {
        Some(p) => p,
        None => return String::new(),
    };

    let brace_start = match content[impl_pos..].find('{') {
        Some(p) => impl_pos + p,
        None => return String::new(),
    };

    let mut depth = 0usize;
    let mut end = brace_start;
    for (i, ch) in content[brace_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = brace_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    content[impl_pos..end].trim().to_string()
}

/// Extract `pub fn …` signatures (up to and including the parameter list `)`).
pub fn extract_fn_signatures(content: &str) -> Vec<String> {
    let mut sigs = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if (t.starts_with("pub fn ") || t.starts_with("pub async fn ")) && t.contains('(') {
            // Take up to the `) ->` or `) {` to get the full signature line
            let sig = t.split('{').next().unwrap_or(t).trim().to_string();
            sigs.push(sig);
        }
    }
    sigs
}

/// Parse field names out of a struct definition string.
///
/// Returns lowercase field names only (ignores `pub`, types, etc.).
pub fn extract_field_names(struct_def: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;
    let mut depth = 0usize;
    for line in struct_def.lines() {
        let t = line.trim();
        if t.contains('{') { depth += 1; inside = true; }
        if t.contains('}') { if depth > 0 { depth -= 1; } }
        if !inside || depth == 0 { continue; }
        // Field lines look like: `pub name: Type,`
        let stripped = t.trim_start_matches("pub").trim();
        if let Some(colon_pos) = stripped.find(':') {
            let name = stripped[..colon_pos].trim().to_string();
            // Skip attribute lines, tuple struct fields, etc.
            if !name.is_empty()
                && !name.starts_with('#')
                && !name.starts_with("//")
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                names.push(name);
            }
        }
    }
    names
}

/// Find all `.field_name` accesses in `code`.
pub fn extract_field_accesses(code: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let bytes = code.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'.' {
            // Collect the identifier that follows
            let start = i + 1;
            let mut end = start;
            while end < len && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start {
                let field = &code[start..end];
                // Skip method calls (followed by `(`)
                if end >= len || bytes[end] != b'(' {
                    fields.push(field.to_string());
                }
            }
        }
        i += 1;
    }
    fields
}

// ─── Internal utilities ───────────────────────────────────────────────────────

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(first) => first.to_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Walk back from `start_of_line` in `content` over consecutive doc-comment
/// and attribute lines, returning the byte position of the first such line.
fn walk_back_over_comments(content: &str, start_of_line: usize) -> usize {
    let before = &content[..start_of_line];
    let lines: Vec<&str> = before.lines().collect();
    let mut keep = lines.len();
    for line in lines.iter().rev() {
        let t = line.trim();
        if t.starts_with("///") || t.starts_with("//!") || t.starts_with("//") || t.starts_with('#') {
            keep -= 1;
        } else {
            break;
        }
    }
    if keep == lines.len() {
        return start_of_line;
    }
    // Byte offset of line `keep`
    let mut offset = 0usize;
    for (i, line) in before.lines().enumerate() {
        if i == keep { return offset; }
        offset += line.len() + 1; // +1 for '\n'
    }
    start_of_line
}

/// Walk back from `keyword_pos` in `content` over `#[…]` attribute lines.
fn walk_back_over_attrs(content: &str, keyword_pos: usize, _pre: &str) -> usize {
    let before = &content[..keyword_pos];
    let lines: Vec<&str> = before.lines().collect();
    let mut keep = lines.len();
    for line in lines.iter().rev() {
        let t = line.trim();
        if t.starts_with("#[") || t.starts_with("///") || t.starts_with("//!") || t.starts_with("//") {
            keep -= 1;
        } else {
            break;
        }
    }
    if keep == lines.len() {
        return keyword_pos;
    }
    let mut offset = 0usize;
    for (i, line) in before.lines().enumerate() {
        if i == keep { return offset; }
        offset += line.len() + 1;
    }
    keyword_pos
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MODEL: &str = r#"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub unit_price_cents: i64,
    pub quantity_on_hand: i32,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateProductPayload {
    pub sku: String,
    pub name: String,
    pub unit_price_cents: i64,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProductPayload {
    pub name: Option<String>,
    pub unit_price_cents: Option<i64>,
}

impl Product {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.sku.trim().is_empty() { errors.push("SKU must not be empty".into()); }
        errors
    }
}
"#;

    const SAMPLE_QUERIES: &str = r#"
pub async fn get_product(pool: &PgPool, id: i64) -> Result<Product, AppError> {
    todo!()
}
pub async fn list_products(pool: &PgPool, params: PaginationParams) -> Result<PaginatedResponse<Product>, AppError> {
    todo!()
}
pub async fn create_product(pool: &PgPool, payload: CreateProductPayload) -> Result<Product, AppError> {
    todo!()
}
"#;

    const SAMPLE_ERRORS: &str = r#"
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound(String),
    #[error("validation error")]
    ValidationError(Vec<String>),
    #[error("unauthorized")]
    Unauthorized,
}
"#;

    // Test 1: extract_struct_full returns the struct definition.
    #[test]
    fn test_extract_struct_full() {
        let result = extract_struct_full(SAMPLE_MODEL, "Product");
        assert!(result.contains("pub sku: String"), "should contain sku field");
        assert!(result.contains("pub unit_price_cents: i64"), "should contain unit_price_cents");
        assert!(!result.contains("CreateProductPayload"), "should not contain other structs");
    }

    // Test 2: extract_fn_signatures returns signatures from queries file.
    #[test]
    fn test_extract_fn_signatures() {
        let sigs = extract_fn_signatures(SAMPLE_QUERIES);
        assert_eq!(sigs.len(), 3, "should find 3 function signatures");
        assert!(sigs[0].contains("get_product"), "first sig should be get_product");
        assert!(sigs[2].contains("create_product"), "third sig should be create_product");
    }

    // Test 3: extract_enum_definition returns AppError enum.
    #[test]
    fn test_extract_enum_definition() {
        let result = extract_enum_definition(SAMPLE_ERRORS, "AppError");
        assert!(result.contains("NotFound"), "should contain NotFound variant");
        assert!(result.contains("ValidationError(Vec<String>)"), "should contain correct ValidationError");
        assert!(result.contains("Unauthorized"), "should contain Unauthorized variant");
    }

    // Test 4: prompt_for_entity returns non-empty string with all sections.
    #[test]
    fn test_prompt_for_entity_nonempty() {
        let mut ctx = TypeContext::default();
        ctx.error_enum = extract_enum_definition(SAMPLE_ERRORS, "AppError");
        ctx.pagination_types = "struct PaginationParams {}".to_string();
        ctx.auth_types = "struct AuthUser {}".to_string();
        let sigs = extract_fn_signatures(SAMPLE_QUERIES);
        ctx.query_signatures.insert("product".to_string(), sigs);
        ctx.models.push(ModelDef {
            entity_name: "Product".to_string(),
            main_struct: extract_struct_full(SAMPLE_MODEL, "Product"),
            create_payload: extract_struct_full(SAMPLE_MODEL, "CreateProductPayload"),
            update_payload: extract_struct_full(SAMPLE_MODEL, "UpdateProductPayload"),
            validation_impl: String::new(),
        });
        let prompt = ctx.prompt_for_entity("Product");
        assert!(!prompt.is_empty());
        assert!(prompt.contains("AppError"));
        assert!(prompt.contains("PaginationParams"));
        assert!(prompt.contains("AuthUser"));
        assert!(prompt.contains("get_product"));
        assert!(prompt.contains("tracing"));
    }

    // Test 5: validate_enriched_function passes on good code.
    #[test]
    fn test_validate_good_code() {
        let ctx = TypeContext::default();
        let code = r#"pub async fn create_product(pool: &PgPool, payload: CreateProductPayload) -> Result<Product, AppError> {
    tracing::info!("creating product");
    Ok(Product { id: 1, sku: payload.sku.clone(), name: payload.name.clone(), unit_price_cents: payload.unit_price_cents, quantity_on_hand: 0, is_active: true })
}"#;
        assert!(validate_enriched_function(code, &ctx, "Product"));
    }

    // Test 6: hallucinated fields are rejected.
    #[test]
    fn test_validate_hallucinated_field_rejected() {
        let mut ctx = TypeContext::default();
        ctx.models.push(ModelDef {
            entity_name: "Product".to_string(),
            main_struct: extract_struct_full(SAMPLE_MODEL, "Product"),
            ..Default::default()
        });
        // "price" is hallucinated — the real field is "unit_price_cents"
        let code = r#"pub fn f() { let x = product.price; }"#;
        assert!(!validate_enriched_function(code, &ctx, "Product"));
    }

    // Test 7: markdown fences are rejected.
    #[test]
    fn test_validate_markdown_rejected() {
        let ctx = TypeContext::default();
        let code = "```rust\npub fn f() { }\n```";
        assert!(!validate_enriched_function(code, &ctx, "Product"));
    }

    // Test 8: `use log::` is rejected.
    #[test]
    fn test_validate_log_crate_rejected() {
        let ctx = TypeContext::default();
        let code = "use log::info;\npub fn f() { info!(\"x\"); }";
        assert!(!validate_enriched_function(code, &ctx, "Product"));
    }

    // Test 9: replace_function_in_file targets the correct function.
    #[test]
    fn test_replace_function_targets_correct() {
        let original = r#"pub fn foo() {
    let x = 1;
}

pub fn bar() {
    let y = 2;
}
"#;
        let new_fn = "pub fn foo() {\n    let x = 42;\n}";
        let result = replace_function_in_file(original, "foo", new_fn);
        assert!(result.contains("let x = 42"), "foo should be replaced");
        assert!(result.contains("let y = 2"), "bar should be unchanged");
    }

    // Test 10: other functions in file remain intact after replacement.
    #[test]
    fn test_replace_function_others_intact() {
        let original = r#"fn alpha() { let a = 1; }
fn beta() { let b = 2; }
fn gamma() { let c = 3; }
"#;
        let new_fn = "fn beta() { let b = 99; }";
        let result = replace_function_in_file(original, "beta", new_fn);
        assert!(result.contains("let a = 1"), "alpha unchanged");
        assert!(result.contains("let b = 99"), "beta replaced");
        assert!(result.contains("let c = 3"), "gamma unchanged");
    }
}
