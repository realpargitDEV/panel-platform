//! Emits TypeScript interfaces and Zod schemas from the JSON Schema that
//! `schemars` derives from this crate's types.
//!
//! Why the generator lives in Rust rather than a Node script: it can then be
//! unit tested with `cargo test` alongside the types it consumes, and CI needs
//! one toolchain to verify the contract rather than two.
//!
//! The supported JSON Schema subset is deliberately narrow — objects, string
//! enums, primitives, arrays, maps, `$ref`, `allOf` merges and nullable
//! `anyOf`. That is exactly what the types in this crate produce. Anything
//! outside it returns [`CodegenError`] instead of emitting a guess, because a
//! silently wrong contract is worse than a failed build.

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodegenError {
    #[error("schema root has no `$defs` object")]
    NoDefinitions,
    #[error("unsupported schema at {path}: {detail}")]
    Unsupported { path: String, detail: String },
    #[error("reference `{reference}` does not point into `$defs`")]
    BadReference { reference: String },
    #[error("types form a reference cycle and cannot be ordered: {types}")]
    CyclicReference { types: String },
    #[error("failed to write output: {0}")]
    Write(String),
}

const BANNER: &str = "\
// ---------------------------------------------------------------------------
// GENERATED FILE — DO NOT EDIT.
//
// Produced from crates/api-types by `cargo run -p project-host-api-types
// --bin emit-contracts`. Edit the Rust types and regenerate; CI fails if this
// file differs from what the generator produces.
// ---------------------------------------------------------------------------
";

/// The generic envelope helpers. Written by the generator rather than kept as a
/// hand-maintained file, so there is still exactly one place that defines them.
const TS_PREAMBLE: &str = "
/** Cursor-paginated result set. */
export interface Page<T> {
  items: T[];
  next_cursor?: string | null;
  has_more: boolean;
}

/** Metadata present on every successful response. */
export interface ResponseMetaEnvelope {
  request_id: string;
  server_time: string;
}

/** Success or failure, discriminated by `ok`. */
export type ApiResponse<T> =
  | { ok: true; data: T; meta: ResponseMetaEnvelope }
  | { ok: false; error: ApiError };
";

const ZOD_PREAMBLE: &str = "
/** Cursor-paginated result set for an arbitrary item schema. */
export const pageOf = <T extends z.ZodTypeAny>(item: T) =>
  z.object({
    items: z.array(item),
    next_cursor: z.string().nullish(),
    has_more: z.boolean(),
  });

export const responseMetaEnvelopeSchema = z.object({
  request_id: z.string(),
  server_time: z.string(),
});

/** Success or failure, discriminated by `ok`. */
export const apiResponseOf = <T extends z.ZodTypeAny>(data: T) =>
  z.union([
    z.object({ ok: z.literal(true), data, meta: responseMetaEnvelopeSchema }),
    z.object({ ok: z.literal(false), error: apiErrorSchema }),
  ]);
";

/// A single named type pulled out of `$defs`.
struct NamedSchema<'a> {
    name: &'a str,
    schema: &'a Value,
}

/// Generate both outputs from a schemars root schema.
pub fn generate(root: &Value) -> Result<(String, String), CodegenError> {
    let defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or(CodegenError::NoDefinitions)?;

    // Dependency order, alphabetical among independent types.
    //
    // TypeScript interfaces are hoisted and would tolerate any order, but Zod
    // schemas are `const` bindings: referencing one before its declaration is a
    // temporal dead zone error at import time. Both files use the same order so
    // there is one thing to reason about.
    let named: Vec<NamedSchema<'_>> = topological_order(defs)?
        .into_iter()
        .filter_map(|name| {
            defs.get_key_value(&name)
                .map(|(name, schema)| NamedSchema { name, schema })
        })
        .collect();

    Ok((emit_typescript(&named, defs)?, emit_zod(&named, defs)?))
}

// ---------------------------------------------------------------- TypeScript

fn emit_typescript(
    named: &[NamedSchema<'_>],
    defs: &Map<String, Value>,
) -> Result<String, CodegenError> {
    let mut out = String::from(BANNER);
    out.push_str(TS_PREAMBLE);

    for entry in named {
        out.push('\n');
        if let Some(doc) = description(entry.schema) {
            writeln!(out, "/** {doc} */").map_err(write_err)?;
        }

        if let Some(values) = string_enum_values(entry.schema) {
            let union = values
                .iter()
                .map(|value| format!("'{value}'"))
                .collect::<Vec<_>>()
                .join(" | ");
            writeln!(out, "export type {} = {union};", entry.name).map_err(write_err)?;
            continue;
        }

        let merged = merge_object(entry.schema, defs, entry.name)?;
        match merged {
            Some(object) => {
                writeln!(out, "export interface {} {{", entry.name).map_err(write_err)?;
                for property in &object.properties {
                    if let Some(doc) = &property.description {
                        writeln!(out, "  /** {doc} */").map_err(write_err)?;
                    }
                    let optional = if property.required { "" } else { "?" };
                    let ty = ts_type(&property.schema, &property.path)?;
                    writeln!(out, "  {}{}: {};", property.name, optional, ty).map_err(write_err)?;
                }
                writeln!(out, "}}").map_err(write_err)?;
            }
            None => {
                let ty = ts_type(entry.schema, entry.name)?;
                writeln!(out, "export type {} = {ty};", entry.name).map_err(write_err)?;
            }
        }
    }

    Ok(out)
}

fn ts_type(schema: &Value, path: &str) -> Result<String, CodegenError> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return Ok(ref_name(reference)?.to_string());
    }

    if let Some(constant) = schema.get("const") {
        return Ok(match constant {
            Value::Bool(value) => value.to_string(),
            Value::String(value) => format!("'{value}'"),
            other => other.to_string(),
        });
    }

    if let Some(values) = string_enum_values(schema) {
        return Ok(values
            .iter()
            .map(|value| format!("'{value}'"))
            .collect::<Vec<_>>()
            .join(" | "));
    }

    if let Some(variants) = union_variants(schema) {
        let (non_null, nullable) = split_null(variants);
        let mut rendered = Vec::new();
        for (index, variant) in non_null.iter().enumerate() {
            rendered.push(ts_type(variant, &format!("{path}[{index}]"))?);
        }
        if nullable {
            rendered.push("null".to_string());
        }
        if rendered.is_empty() {
            return Err(unsupported(path, "empty union"));
        }
        return Ok(rendered.join(" | "));
    }

    match type_of(schema) {
        Some("string") => Ok("string".to_string()),
        Some("integer") | Some("number") => Ok("number".to_string()),
        Some("boolean") => Ok("boolean".to_string()),
        Some("null") => Ok("null".to_string()),
        Some("array") => {
            let items = schema
                .get("items")
                .ok_or_else(|| unsupported(path, "array without `items`"))?;
            let inner = ts_type(items, &format!("{path}[]"))?;
            // Parenthesise unions so `A | B[]` cannot be misread.
            if inner.contains('|') {
                Ok(format!("({inner})[]"))
            } else {
                Ok(format!("{inner}[]"))
            }
        }
        Some("object") => {
            if let Some(additional) = schema.get("additionalProperties") {
                if additional.is_object() {
                    let inner = ts_type(additional, &format!("{path}{{}}"))?;
                    return Ok(format!("Record<string, {inner}>"));
                }
            }
            if schema.get("properties").is_some() {
                return Err(unsupported(
                    path,
                    "inline object; give it a named Rust type",
                ));
            }
            Ok("Record<string, unknown>".to_string())
        }
        _ => Err(unsupported(path, "no recognisable `type`")),
    }
}

// ---------------------------------------------------------------- Zod

fn emit_zod(named: &[NamedSchema<'_>], defs: &Map<String, Value>) -> Result<String, CodegenError> {
    let mut out = String::from(BANNER);
    out.push_str("\nimport { z } from 'zod';\n");

    for entry in named {
        out.push('\n');
        let const_name = schema_const_name(entry.name);

        if let Some(values) = string_enum_values(entry.schema) {
            let list = values
                .iter()
                .map(|value| format!("'{value}'"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "export const {const_name} = z.enum([{list}]);").map_err(write_err)?;
            continue;
        }

        let merged = merge_object(entry.schema, defs, entry.name)?;
        match merged {
            Some(object) => {
                writeln!(out, "export const {const_name} = z.object({{").map_err(write_err)?;
                for property in &object.properties {
                    let inner = zod_type(&property.schema, &property.path)?;
                    let suffix = if property.required { "" } else { ".optional()" };
                    writeln!(out, "  {}: {}{},", property.name, inner, suffix)
                        .map_err(write_err)?;
                }
                writeln!(out, "}});").map_err(write_err)?;
            }
            None => {
                let inner = zod_type(entry.schema, entry.name)?;
                writeln!(out, "export const {const_name} = {inner};").map_err(write_err)?;
            }
        }
    }

    out.push_str(ZOD_PREAMBLE);
    Ok(out)
}

fn zod_type(schema: &Value, path: &str) -> Result<String, CodegenError> {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return Ok(schema_const_name(ref_name(reference)?));
    }

    if let Some(constant) = schema.get("const") {
        return Ok(match constant {
            Value::Bool(value) => format!("z.literal({value})"),
            Value::String(value) => format!("z.literal('{value}')"),
            other => format!("z.literal({other})"),
        });
    }

    if let Some(values) = string_enum_values(schema) {
        let list = values
            .iter()
            .map(|value| format!("'{value}'"))
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(format!("z.enum([{list}])"));
    }

    if let Some(variants) = union_variants(schema) {
        let (non_null, nullable) = split_null(variants);
        let mut rendered = Vec::new();
        for (index, variant) in non_null.iter().enumerate() {
            rendered.push(zod_type(variant, &format!("{path}[{index}]"))?);
        }
        let base = match rendered.len() {
            0 => return Err(unsupported(path, "empty union")),
            1 => rendered.remove(0),
            _ => format!("z.union([{}])", rendered.join(", ")),
        };
        return Ok(if nullable {
            format!("{base}.nullable()")
        } else {
            base
        });
    }

    match type_of(schema) {
        Some("string") => Ok("z.string()".to_string()),
        Some("integer") => Ok("z.number().int()".to_string()),
        Some("number") => Ok("z.number()".to_string()),
        Some("boolean") => Ok("z.boolean()".to_string()),
        Some("null") => Ok("z.null()".to_string()),
        Some("array") => {
            let items = schema
                .get("items")
                .ok_or_else(|| unsupported(path, "array without `items`"))?;
            Ok(format!(
                "z.array({})",
                zod_type(items, &format!("{path}[]"))?
            ))
        }
        Some("object") => {
            if let Some(additional) = schema.get("additionalProperties") {
                if additional.is_object() {
                    let inner = zod_type(additional, &format!("{path}{{}}"))?;
                    return Ok(format!("z.record(z.string(), {inner})"));
                }
            }
            if schema.get("properties").is_some() {
                return Err(unsupported(
                    path,
                    "inline object; give it a named Rust type",
                ));
            }
            Ok("z.record(z.string(), z.unknown())".to_string())
        }
        _ => Err(unsupported(path, "no recognisable `type`")),
    }
}

// ---------------------------------------------------------------- shared

struct ObjectShape {
    properties: Vec<Property>,
}

struct Property {
    name: String,
    schema: Value,
    required: bool,
    description: Option<String>,
    path: String,
}

/// Flatten `allOf` into a single object shape.
///
/// `#[serde(flatten)]` makes schemars emit the parent's own properties plus an
/// `allOf` entry per flattened field. Merging them is what keeps
/// `ProjectDetail` a single flat interface on the TypeScript side, matching
/// what the wire format actually looks like.
fn merge_object(
    schema: &Value,
    defs: &Map<String, Value>,
    path: &str,
) -> Result<Option<ObjectShape>, CodegenError> {
    let mut properties: Vec<Property> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut found_object = false;

    let mut collect = |node: &Value, path: &str| -> Result<(), CodegenError> {
        let Some(props) = node.get("properties").and_then(Value::as_object) else {
            return Ok(());
        };
        found_object = true;
        let required: Vec<&str> = node
            .get("required")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        for (name, property_schema) in props {
            let property = Property {
                name: name.clone(),
                schema: property_schema.clone(),
                required: required.contains(&name.as_str()),
                description: description(property_schema),
                path: format!("{path}.{name}"),
            };
            match seen.get(name) {
                // A flattened field re-declaring a property: last write wins,
                // matching serde's own behaviour.
                Some(index) => {
                    if let Some(slot) = properties.get_mut(*index) {
                        *slot = property;
                    }
                }
                None => {
                    seen.insert(name.clone(), properties.len());
                    properties.push(property);
                }
            }
        }
        Ok(())
    };

    collect(schema, path)?;

    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        for (index, member) in all_of.iter().enumerate() {
            let resolved = resolve(member, defs)?;
            collect(&resolved, &format!("{path}.allOf[{index}]"))?;
        }
    }

    if !found_object {
        return Ok(None);
    }
    Ok(Some(ObjectShape { properties }))
}

fn resolve(node: &Value, defs: &Map<String, Value>) -> Result<Value, CodegenError> {
    match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let name = ref_name(reference)?;
            defs.get(name)
                .cloned()
                .ok_or_else(|| CodegenError::BadReference {
                    reference: reference.to_string(),
                })
        }
        None => Ok(node.clone()),
    }
}

/// Names every `$defs` entry reachable from this node, at any depth.
fn collect_refs(node: &Value, out: &mut std::collections::BTreeSet<String>) {
    match node {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "$ref" {
                    if let Some(name) = value.as_str().and_then(|r| r.strip_prefix("#/$defs/")) {
                        out.insert(name.to_string());
                    }
                } else {
                    collect_refs(value, out);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

/// Depth-first topological sort. Ties break alphabetically so the output is
/// byte-identical between runs, which is what makes `--check` meaningful.
fn topological_order(defs: &Map<String, Value>) -> Result<Vec<String>, CodegenError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        InProgress,
        Done,
    }

    let dependencies: BTreeMap<String, std::collections::BTreeSet<String>> = defs
        .iter()
        .map(|(name, schema)| {
            let mut refs = std::collections::BTreeSet::new();
            collect_refs(schema, &mut refs);
            // A self-reference needs no ordering and must not look like a cycle.
            refs.remove(name);
            // Ignore references to types that are not defined here; `ref_name`
            // reports those separately with a better message.
            refs.retain(|dependency| defs.contains_key(dependency));
            (name.clone(), refs)
        })
        .collect();

    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut ordered: Vec<String> = Vec::with_capacity(defs.len());
    // Explicit stack rather than recursion: a deeply nested contract should not
    // be able to overflow the generator.
    let mut stack: Vec<(&str, bool)> = Vec::new();

    for root in dependencies.keys() {
        if marks.get(root.as_str()) == Some(&Mark::Done) {
            continue;
        }
        stack.push((root.as_str(), false));

        while let Some((name, children_done)) = stack.pop() {
            if children_done {
                marks.insert(name, Mark::Done);
                ordered.push(name.to_string());
                continue;
            }
            match marks.get(name) {
                Some(Mark::Done) => continue,
                Some(Mark::InProgress) => {
                    return Err(CodegenError::CyclicReference {
                        types: name.to_string(),
                    });
                }
                None => {}
            }
            marks.insert(name, Mark::InProgress);
            stack.push((name, true));

            if let Some(children) = dependencies.get(name) {
                // Reversed so the alphabetically first child is popped first.
                for child in children.iter().rev() {
                    if marks.get(child.as_str()) != Some(&Mark::Done) {
                        if let Some((key, _)) = dependencies.get_key_value(child) {
                            stack.push((key.as_str(), false));
                        }
                    }
                }
            }
        }
    }

    Ok(ordered)
}

fn ref_name(reference: &str) -> Result<&str, CodegenError> {
    reference
        .strip_prefix("#/$defs/")
        .ok_or_else(|| CodegenError::BadReference {
            reference: reference.to_string(),
        })
}

/// `type` may be a bare string or an array such as `["string","null"]`.
fn type_of(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(value)) => Some(value.as_str()),
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .find(|value| *value != "null"),
        _ => None,
    }
}

/// `["string","null"]` is schemars' other way of spelling an optional field.
fn type_array_is_nullable(schema: &Value) -> bool {
    matches!(schema.get("type"), Some(Value::Array(values))
        if values.iter().any(|value| value == "null"))
}

fn string_enum_values(schema: &Value) -> Option<Vec<String>> {
    let values = schema.get("enum")?.as_array()?;
    let strings: Vec<String> = values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    (strings.len() == values.len() && !strings.is_empty()).then_some(strings)
}

fn union_variants(schema: &Value) -> Option<Vec<Value>> {
    for keyword in ["anyOf", "oneOf"] {
        if let Some(list) = schema.get(keyword).and_then(Value::as_array) {
            return Some(list.clone());
        }
    }
    // A nullable primitive spelled as a type array behaves like a union.
    if type_array_is_nullable(schema) {
        if let Some(inner) = type_of(schema) {
            return Some(vec![
                serde_json::json!({ "type": inner }),
                serde_json::json!({ "type": "null" }),
            ]);
        }
    }
    None
}

fn split_null(variants: Vec<Value>) -> (Vec<Value>, bool) {
    let mut nullable = false;
    let mut kept = Vec::new();
    for variant in variants {
        if type_of(&variant) == Some("null") || variant.get("type") == Some(&Value::from("null")) {
            nullable = true;
        } else {
            kept.push(variant);
        }
    }
    (kept, nullable)
}

fn description(schema: &Value) -> Option<String> {
    schema
        .get("description")
        .and_then(Value::as_str)
        .map(|text| text.replace('\n', " ").trim().to_string())
        .filter(|text| !text.is_empty())
}

/// `ProjectSummary` → `projectSummarySchema`.
fn schema_const_name(name: &str) -> String {
    let mut chars = name.chars();
    let lowered = match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    };
    format!("{lowered}Schema")
}

fn unsupported(path: &str, detail: &str) -> CodegenError {
    CodegenError::Unsupported {
        path: path.to_string(),
        detail: detail.to_string(),
    }
}

fn write_err(error: std::fmt::Error) -> CodegenError {
    CodegenError::Write(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn generate_from(defs: Value) -> (String, String) {
        let root = json!({ "$defs": defs });
        match generate(&root) {
            Ok(pair) => pair,
            Err(error) => panic!("generation failed: {error}"),
        }
    }

    #[test]
    fn emits_an_interface_with_optional_fields() {
        let (ts, zod) = generate_from(json!({
            "Thing": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "count": { "type": "integer" },
                    "note": { "type": "string" }
                },
                "required": ["id", "count"]
            }
        }));

        assert!(ts.contains("export interface Thing {"), "{ts}");
        assert!(ts.contains("id: string;"), "{ts}");
        assert!(ts.contains("count: number;"), "{ts}");
        assert!(ts.contains("note?: string;"), "{ts}");

        assert!(
            zod.contains("export const thingSchema = z.object({"),
            "{zod}"
        );
        assert!(zod.contains("count: z.number().int(),"), "{zod}");
        assert!(zod.contains("note: z.string().optional(),"), "{zod}");
    }

    #[test]
    fn string_enums_become_unions_and_z_enum() {
        let (ts, zod) = generate_from(json!({
            "Status": { "type": "string", "enum": ["RUNNING", "STOPPED"] }
        }));
        assert!(
            ts.contains("export type Status = 'RUNNING' | 'STOPPED';"),
            "{ts}"
        );
        assert!(
            zod.contains("export const statusSchema = z.enum(['RUNNING', 'STOPPED']);"),
            "{zod}"
        );
    }

    #[test]
    fn references_become_named_types() {
        let (ts, zod) = generate_from(json!({
            "Inner": { "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"] },
            "Outer": {
                "type": "object",
                "properties": { "inner": { "$ref": "#/$defs/Inner" } },
                "required": ["inner"]
            }
        }));
        assert!(ts.contains("inner: Inner;"), "{ts}");
        assert!(zod.contains("inner: innerSchema,"), "{zod}");
    }

    #[test]
    fn nullable_any_of_collapses_to_a_nullable_type() {
        let (ts, zod) = generate_from(json!({
            "Thing": {
                "type": "object",
                "properties": {
                    "maybe": { "anyOf": [{ "type": "string" }, { "type": "null" }] }
                },
                "required": ["maybe"]
            }
        }));
        assert!(ts.contains("maybe: string | null;"), "{ts}");
        assert!(zod.contains("maybe: z.string().nullable(),"), "{zod}");
    }

    #[test]
    fn nullable_type_arrays_are_treated_as_unions() {
        let (ts, _) = generate_from(json!({
            "Thing": {
                "type": "object",
                "properties": { "maybe": { "type": ["string", "null"] } },
                "required": ["maybe"]
            }
        }));
        assert!(ts.contains("maybe: string | null;"), "{ts}");
    }

    #[test]
    fn arrays_and_maps_are_supported() {
        let (ts, zod) = generate_from(json!({
            "Thing": {
                "type": "object",
                "properties": {
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "meta": { "type": "object", "additionalProperties": { "type": "string" } }
                },
                "required": ["tags", "meta"]
            }
        }));
        assert!(ts.contains("tags: string[];"), "{ts}");
        assert!(ts.contains("meta: Record<string, string>;"), "{ts}");
        assert!(zod.contains("tags: z.array(z.string()),"), "{zod}");
        assert!(
            zod.contains("meta: z.record(z.string(), z.string()),"),
            "{zod}"
        );
    }

    #[test]
    fn all_of_is_flattened_into_one_interface() {
        // This is the `#[serde(flatten)]` case: the wire format is one flat
        // object, so the generated interface must be flat too.
        let (ts, _) = generate_from(json!({
            "Base": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            },
            "Extended": {
                "type": "object",
                "properties": { "extra": { "type": "string" } },
                "required": ["extra"],
                "allOf": [{ "$ref": "#/$defs/Base" }]
            }
        }));
        assert!(ts.contains("export interface Extended {"), "{ts}");
        assert!(ts.contains("id: string;"), "flattened field missing: {ts}");
        assert!(ts.contains("extra: string;"), "{ts}");
        assert!(
            !ts.contains("Base;"),
            "flatten must not emit a nested field: {ts}"
        );
    }

    #[test]
    fn const_booleans_become_literals() {
        let (ts, zod) = generate_from(json!({
            "Marker": { "type": "boolean", "const": true }
        }));
        assert!(ts.contains("export type Marker = true;"), "{ts}");
        assert!(
            zod.contains("export const markerSchema = z.literal(true);"),
            "{zod}"
        );
    }

    #[test]
    fn a_referenced_type_is_declared_before_its_user() {
        // Zod schemas are `const` bindings, so this is not cosmetic: emitting
        // Alpha (which references Zebra) first would throw at import time.
        let (_, zod) = generate_from(json!({
            "Alpha": {
                "type": "object",
                "properties": { "z": { "$ref": "#/$defs/Zebra" } },
                "required": ["z"]
            },
            "Zebra": { "type": "string", "enum": ["A"] }
        }));
        let zebra = zod.find("export const zebraSchema").unwrap_or(usize::MAX);
        let alpha = zod.find("export const alphaSchema").unwrap_or(0);
        assert!(zebra < alpha, "dependency emitted after its user:\n{zod}");
    }

    #[test]
    fn transitive_dependencies_are_ordered_too() {
        let (_, zod) = generate_from(json!({
            "A": { "type": "object", "properties": { "b": { "$ref": "#/$defs/B" } }, "required": ["b"] },
            "B": { "type": "object", "properties": { "c": { "$ref": "#/$defs/C" } }, "required": ["c"] },
            "C": { "type": "string", "enum": ["X"] }
        }));
        let c = zod.find("export const cSchema").unwrap_or(usize::MAX);
        let b = zod.find("export const bSchema").unwrap_or(usize::MAX);
        let a = zod.find("export const aSchema").unwrap_or(0);
        assert!(c < b && b < a, "wrong order:\n{zod}");
    }

    #[test]
    fn a_reference_cycle_is_reported_rather_than_emitted_broken() {
        let root = json!({ "$defs": {
            "A": { "type": "object", "properties": { "b": { "$ref": "#/$defs/B" } }, "required": ["b"] },
            "B": { "type": "object", "properties": { "a": { "$ref": "#/$defs/A" } }, "required": ["a"] }
        }});
        assert!(matches!(
            generate(&root).unwrap_err(),
            CodegenError::CyclicReference { .. }
        ));
    }

    #[test]
    fn a_self_reference_is_not_mistaken_for_a_cycle() {
        let (_, zod) = generate_from(json!({
            "Node": {
                "type": "object",
                "properties": { "child": { "$ref": "#/$defs/Node" } },
                "required": []
            }
        }));
        assert!(zod.contains("export const nodeSchema"), "{zod}");
    }

    #[test]
    fn output_order_is_stable_across_runs() {
        let defs = json!({
            "Zebra": { "type": "string", "enum": ["A"] },
            "Apple": { "type": "string", "enum": ["B"] }
        });
        let (first, _) = generate_from(defs.clone());
        let (second, _) = generate_from(defs);
        assert_eq!(first, second);
        let apple = first.find("Apple").unwrap_or(usize::MAX);
        let zebra = first.find("Zebra").unwrap_or(0);
        assert!(apple < zebra, "definitions must be emitted in sorted order");
    }

    #[test]
    fn an_unsupported_schema_fails_instead_of_guessing() {
        let root = json!({ "$defs": { "Weird": { "type": "object", "properties": {
            "nested": { "type": "object", "properties": { "a": { "type": "string" } } }
        }, "required": ["nested"] } } });
        let error = generate(&root).unwrap_err();
        assert!(
            matches!(error, CodegenError::Unsupported { .. }),
            "expected Unsupported, got {error:?}"
        );
    }

    #[test]
    fn a_missing_defs_block_is_an_error() {
        assert_eq!(
            generate(&json!({})).unwrap_err(),
            CodegenError::NoDefinitions
        );
    }

    #[test]
    fn a_dangling_reference_is_reported() {
        let root = json!({ "$defs": { "Thing": {
            "type": "object",
            "properties": { "x": { "type": "string" } },
            "required": ["x"],
            "allOf": [{ "$ref": "#/$defs/Missing" }]
        } } });
        assert!(matches!(
            generate(&root).unwrap_err(),
            CodegenError::BadReference { .. }
        ));
    }
}
