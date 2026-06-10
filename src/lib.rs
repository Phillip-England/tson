use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, HashMap};
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Debug, Error)]
pub enum TsonError {
    #[error("failed to load TypeScript parser")]
    ParserLanguage,
    #[error("failed to parse TypeScript source")]
    ParseFailed,
    #[error("TypeScript parse error found in input")]
    ParseError,
    #[error("no TypeScript classes or interfaces were found")]
    NoTypesFound,
    #[error("type `{0}` was not found")]
    TypeNotFound(String),
    #[error("symbol `{0}` is not a class, interface, or type alias")]
    UnsupportedRoot(String),
    #[error("invalid UTF-8 in parsed source")]
    Utf8,
}

#[derive(Clone, Copy)]
enum Symbol<'tree> {
    Class(Node<'tree>),
    Interface(Node<'tree>),
    TypeAlias(Node<'tree>),
}

pub struct SchemaOptions {
    pub root_type: Option<String>,
    pub additional_properties: bool,
}

pub struct ExampleOptions {
    pub count: usize,
    pub valid: bool,
}

pub fn schema_from_source(source: &str, options: SchemaOptions) -> Result<Value, TsonError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .map_err(|_| TsonError::ParserLanguage)?;

    let tree = parser.parse(source, None).ok_or(TsonError::ParseFailed)?;
    let root = tree.root_node();
    if root.has_error() {
        return Err(TsonError::ParseError);
    }

    let symbols = collect_symbols(root, source.as_bytes())?;
    if symbols.is_empty() {
        return Err(TsonError::NoTypesFound);
    }

    let root_name = match options.root_type {
        Some(name) => name,
        None => first_object_type(root, source.as_bytes())?.ok_or(TsonError::NoTypesFound)?,
    };

    let mut converter = Converter {
        source: source.as_bytes(),
        symbols,
        emitted_defs: BTreeSet::new(),
        additional_properties: options.additional_properties,
    };

    let root_symbol = *converter
        .symbols
        .get(&root_name)
        .ok_or_else(|| TsonError::TypeNotFound(root_name.clone()))?;

    let mut schema = match root_symbol {
        Symbol::Class(node) => converter.class_schema(node)?,
        Symbol::Interface(node) => converter.interface_schema(node)?,
        Symbol::TypeAlias(node) => converter.type_alias_schema(node)?,
    };

    if let Value::Object(ref mut object) = schema {
        object.insert(
            "$schema".to_string(),
            Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
        );
        object.insert("title".to_string(), Value::String(root_name));
    }

    if !converter.emitted_defs.is_empty() {
        let mut defs = Map::new();
        let mut processed = BTreeSet::new();

        while processed.len() < converter.emitted_defs.len() {
            let def_names: Vec<String> = converter.emitted_defs.iter().cloned().collect();
            for name in def_names {
                if processed.contains(&name) {
                    continue;
                }
                processed.insert(name.clone());

                let symbol = *converter
                    .symbols
                    .get(&name)
                    .ok_or_else(|| TsonError::TypeNotFound(name.clone()))?;
                let def_schema = match symbol {
                    Symbol::Class(node) => converter.class_schema(node)?,
                    Symbol::Interface(node) => converter.interface_schema(node)?,
                    Symbol::TypeAlias(node) => converter.type_alias_schema(node)?,
                };
                defs.insert(name, def_schema);
            }
        }

        for name in processed {
            let symbol = *converter
                .symbols
                .get(&name)
                .ok_or_else(|| TsonError::TypeNotFound(name.clone()))?;
            if !defs.contains_key(&name) {
                let def_schema = match symbol {
                    Symbol::Class(node) => converter.class_schema(node)?,
                    Symbol::Interface(node) => converter.interface_schema(node)?,
                    Symbol::TypeAlias(node) => converter.type_alias_schema(node)?,
                };
                defs.insert(name, def_schema);
            }
        }
        if let Value::Object(ref mut object) = schema {
            object.insert("$defs".to_string(), Value::Object(defs));
        }
    }

    Ok(schema)
}

pub fn examples_from_schema(schema: &Value, options: ExampleOptions) -> Vec<Value> {
    (0..options.count)
        .map(|index| {
            if options.valid {
                valid_example(schema, schema, index)
            } else {
                invalid_example(schema, schema, index)
            }
        })
        .collect()
}

fn valid_example(schema: &Value, root: &Value, index: usize) -> Value {
    let schema = resolve_ref(schema, root);

    if let Some(value) = schema.get("const") {
        return value.clone();
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if let Some(value) = values.get(index % values.len().max(1)) {
            return value.clone();
        }
    }
    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        if let Some(variant) = variants.get(index % variants.len().max(1)) {
            return valid_example(variant, root, index);
        }
    }

    match schema_type(schema).as_deref() {
        Some("object") => valid_object(schema, root, index),
        Some("array") => {
            let item_schema = schema.get("items").unwrap_or(&Value::Null);
            let len = (index % 2) + 1;
            Value::Array(
                (0..len)
                    .map(|offset| valid_example(item_schema, root, index + offset))
                    .collect(),
            )
        }
        Some("string") => Value::String(format!("example-{}", index + 1)),
        Some("number") => json!(index as f64 + 1.25),
        Some("integer") => json!(index as i64 + 1),
        Some("boolean") => Value::Bool(index % 2 == 0),
        Some("null") => Value::Null,
        _ => json!({ "example": index + 1 }),
    }
}

fn valid_object(schema: &Value, root: &Value, index: usize) -> Value {
    let mut object = Map::new();
    let required = required_properties(schema);

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (name, property_schema) in properties {
            if required.contains(name) || index % 2 == 0 {
                object.insert(name.clone(), valid_example(property_schema, root, index));
            }
        }
    }

    if schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        object.insert(
            format!("extra_{}", index + 1),
            Value::String("additional value".to_string()),
        );
    }

    Value::Object(object)
}

fn invalid_example(schema: &Value, root: &Value, index: usize) -> Value {
    let schema = resolve_ref(schema, root);

    if schema.get("const").is_some() || schema.get("enum").is_some() {
        return invalid_scalar(schema);
    }

    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        if variants
            .iter()
            .any(|variant| schema_type(resolve_ref(variant, root)).as_deref() == Some("object"))
        {
            return Value::Array(vec![Value::String("invalid-union".to_string())]);
        }
        return json!({ "__tson_invalid": true });
    }

    match schema_type(schema).as_deref() {
        Some("object") => invalid_object(schema, root, index),
        Some("array") => invalid_scalar(schema),
        Some(_) => invalid_scalar(schema),
        None => Value::Null,
    }
}

fn invalid_object(schema: &Value, root: &Value, index: usize) -> Value {
    let required = required_properties(schema);
    if index % 3 == 0 {
        if let Some(name) = required.get(index % required.len().max(1)) {
            if let Value::Object(mut object) = valid_object(schema, root, index) {
                object.remove(name);
                return Value::Object(object);
            }
        }
    }

    if index % 3 == 1 {
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            if let Some((name, property_schema)) =
                properties.iter().nth(index % properties.len().max(1))
            {
                if let Value::Object(mut object) = valid_object(schema, root, index) {
                    object.insert(name.clone(), invalid_example(property_schema, root, index));
                    return Value::Object(object);
                }
            }
        }
    }

    if schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .is_some_and(|allowed| !allowed)
    {
        if let Value::Object(mut object) = valid_object(schema, root, index) {
            object.insert("__unexpected".to_string(), Value::Bool(true));
            return Value::Object(object);
        }
    }

    if let Some(name) = required.first() {
        if let Value::Object(mut object) = valid_object(schema, root, index) {
            object.remove(name);
            return Value::Object(object);
        }
    }

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some((name, property_schema)) = properties.iter().next() {
            if let Value::Object(mut object) = valid_object(schema, root, index) {
                object.insert(name.clone(), invalid_example(property_schema, root, index));
                return Value::Object(object);
            }
        }
    }

    Value::Array(vec![])
}

fn invalid_scalar(schema: &Value) -> Value {
    match schema_type(schema).as_deref() {
        Some("object") => Value::Array(vec![]),
        Some("array") => Value::Object(Map::new()),
        Some("string") => json!(123),
        Some("number") | Some("integer") => Value::String("not-a-number".to_string()),
        Some("boolean") => Value::String("not-a-boolean".to_string()),
        Some("null") => Value::String("not-null".to_string()),
        _ => Value::Null,
    }
}

fn resolve_ref<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    let Some(pointer) = reference.strip_prefix('#') else {
        return schema;
    };
    root.pointer(pointer).unwrap_or(schema)
}

fn schema_type(schema: &Value) -> Option<String> {
    match schema.get("type") {
        Some(Value::String(kind)) => Some(kind.clone()),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .find_map(Value::as_str)
            .map(ToString::to_string),
        _ => None,
    }
}

fn required_properties(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

struct Converter<'tree, 'src> {
    source: &'src [u8],
    symbols: HashMap<String, Symbol<'tree>>,
    emitted_defs: BTreeSet<String>,
    additional_properties: bool,
}

impl<'tree, 'src> Converter<'tree, 'src> {
    fn class_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let body = node
            .child_by_field_name("body")
            .ok_or(TsonError::ParseFailed)?;
        self.object_from_members(body, "public_field_definition")
    }

    fn interface_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let body = first_named_child_kind(node, "interface_body").ok_or(TsonError::ParseFailed)?;
        self.object_from_members(body, "property_signature")
    }

    fn type_alias_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let type_node = named_children(node)
            .into_iter()
            .rev()
            .find(|child| is_type_node(child.kind()))
            .ok_or(TsonError::ParseFailed)?;
        self.type_schema(type_node)
    }

    fn object_from_members(
        &mut self,
        body: Node<'tree>,
        member_kind: &str,
    ) -> Result<Value, TsonError> {
        let mut properties = Map::new();
        let mut required = Vec::new();

        for member in named_children(body) {
            if member.kind() != member_kind {
                continue;
            }
            if let Some(name_node) = member.child_by_field_name("name") {
                if name_node.kind() == "private_property_identifier" {
                    continue;
                }
                let name = property_name(name_node, self.source)?;
                let schema = match member.child_by_field_name("type") {
                    Some(annotation) => match first_named_type_child(annotation) {
                        Some(type_node) => self.type_schema(type_node)?,
                        None => json!({}),
                    },
                    None => json!({}),
                };
                if !has_child_kind(member, "?") {
                    required.push(Value::String(name.clone()));
                }
                properties.insert(name, schema);
            }
        }

        let mut object = Map::new();
        object.insert("type".to_string(), Value::String("object".to_string()));
        object.insert("properties".to_string(), Value::Object(properties));
        if !required.is_empty() {
            object.insert("required".to_string(), Value::Array(required));
        }
        object.insert(
            "additionalProperties".to_string(),
            Value::Bool(self.additional_properties),
        );
        Ok(Value::Object(object))
    }

    fn type_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        match node.kind() {
            "predefined_type" => Ok(predefined_schema(self.text(node)?)),
            "type_identifier" | "nested_type_identifier" => self.reference_schema(node),
            "object_type" | "interface_body" => {
                self.object_from_members(node, "property_signature")
            }
            "array_type" => self.array_schema(node),
            "union_type" => self.union_schema(node),
            "literal_type" => self.literal_schema(node),
            "parenthesized_type" => match first_named_type_child(node) {
                Some(child) => self.type_schema(child),
                None => Ok(json!({})),
            },
            "generic_type" => self.generic_schema(node),
            _ => Ok(json!({
                "description": format!("Unsupported TypeScript type: {}", self.text(node)?)
            })),
        }
    }

    fn reference_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let name = self
            .text(node)?
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_string();
        if self.symbols.contains_key(&name) {
            self.emitted_defs.insert(name.clone());
            Ok(json!({ "$ref": format!("#/$defs/{name}") }))
        } else {
            Ok(json!({
                "description": format!("Unresolved TypeScript type reference: {name}")
            }))
        }
    }

    fn array_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let item_type = named_children(node)
            .into_iter()
            .find(|child| is_type_node(child.kind()));
        let items = match item_type {
            Some(child) => self.type_schema(child)?,
            None => json!({}),
        };
        Ok(json!({ "type": "array", "items": items }))
    }

    fn generic_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let text = self.text(node)?;
        if text.starts_with("Array<") || text.starts_with("ReadonlyArray<") {
            if let Some(args) = named_children(node)
                .into_iter()
                .find(|child| child.kind() == "type_arguments")
            {
                if let Some(item) = named_children(args)
                    .into_iter()
                    .find(|child| is_type_node(child.kind()))
                {
                    return Ok(json!({ "type": "array", "items": self.type_schema(item)? }));
                }
            }
        }

        Ok(json!({
            "description": format!("Unsupported TypeScript generic type: {text}")
        }))
    }

    fn union_schema(&mut self, node: Node<'tree>) -> Result<Value, TsonError> {
        let mut variants = Vec::new();
        let mut literal_values = Vec::new();
        let mut literal_type: Option<&'static str> = None;
        let mut nullable = false;

        for child in named_children(node) {
            if !is_type_node(child.kind()) {
                continue;
            }
            let text = self.text(child)?;
            if text == "undefined" || text == "void" {
                continue;
            }
            if text == "null" {
                nullable = true;
                continue;
            }

            if child.kind() == "literal_type" {
                if let Some((json_type, value)) = self.literal_value(child)? {
                    if literal_type.map_or(true, |existing| existing == json_type) {
                        literal_type = Some(json_type);
                        literal_values.push(value);
                        continue;
                    }
                }
            }

            variants.push(self.type_schema(child)?);
        }

        if !literal_values.is_empty() && variants.is_empty() {
            let mut schema = json!({
                "type": literal_type.unwrap_or("string"),
                "enum": literal_values
            });
            if nullable {
                schema = json!({ "anyOf": [schema, { "type": "null" }] });
            }
            return Ok(schema);
        }

        if variants.len() == 1 && !nullable {
            return Ok(variants.remove(0));
        }

        if nullable {
            variants.push(json!({ "type": "null" }));
        }

        if variants.is_empty() {
            Ok(json!({}))
        } else {
            Ok(json!({ "anyOf": variants }))
        }
    }

    fn literal_schema(&self, node: Node<'tree>) -> Result<Value, TsonError> {
        match self.literal_value(node)? {
            Some((json_type, value)) => Ok(json!({ "type": json_type, "const": value })),
            None => Ok(json!({})),
        }
    }

    fn literal_value(&self, node: Node<'tree>) -> Result<Option<(&'static str, Value)>, TsonError> {
        let text = self.text(node)?.trim();
        if text == "true" {
            return Ok(Some(("boolean", Value::Bool(true))));
        }
        if text == "false" {
            return Ok(Some(("boolean", Value::Bool(false))));
        }
        if text == "null" {
            return Ok(Some(("null", Value::Null)));
        }
        if let Some(value) = unquote(text) {
            return Ok(Some(("string", Value::String(value))));
        }
        if let Ok(number) = text.parse::<i64>() {
            return Ok(Some(("number", Value::Number(number.into()))));
        }
        Ok(None)
    }

    fn text(&self, node: Node<'tree>) -> Result<&'src str, TsonError> {
        node.utf8_text(self.source).map_err(|_| TsonError::Utf8)
    }
}

fn collect_symbols<'tree>(
    root: Node<'tree>,
    source: &[u8],
) -> Result<HashMap<String, Symbol<'tree>>, TsonError> {
    let mut symbols = HashMap::new();
    walk(root, &mut |node| {
        let symbol = match node.kind() {
            "class_declaration" => Some(Symbol::Class(node)),
            "interface_declaration" => Some(Symbol::Interface(node)),
            "type_alias_declaration" => Some(Symbol::TypeAlias(node)),
            _ => None,
        };
        if let Some(symbol) = symbol {
            if let Some(name) = node.child_by_field_name("name") {
                let name = name
                    .utf8_text(source)
                    .map_err(|_| TsonError::Utf8)?
                    .to_string();
                symbols.insert(name, symbol);
            }
        }
        Ok(())
    })?;
    Ok(symbols)
}

fn first_object_type(root: Node, source: &[u8]) -> Result<Option<String>, TsonError> {
    let mut found = None;
    walk(root, &mut |node| {
        if found.is_none()
            && matches!(node.kind(), "class_declaration" | "interface_declaration")
            && node.child_by_field_name("name").is_some()
        {
            let name = node.child_by_field_name("name").unwrap();
            found = Some(
                name.utf8_text(source)
                    .map_err(|_| TsonError::Utf8)?
                    .to_string(),
            );
        }
        Ok(())
    })?;
    Ok(found)
}

fn walk<'tree, F>(node: Node<'tree>, visitor: &mut F) -> Result<(), TsonError>
where
    F: FnMut(Node<'tree>) -> Result<(), TsonError>,
{
    visitor(node)?;
    for child in named_children(node) {
        walk(child, visitor)?;
    }
    Ok(())
}

fn named_children(node: Node) -> Vec<Node> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .collect()
}

fn first_named_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    named_children(node)
        .into_iter()
        .find(|child| child.kind() == kind)
}

fn first_named_type_child(node: Node) -> Option<Node> {
    named_children(node)
        .into_iter()
        .find(|child| is_type_node(child.kind()))
}

fn has_child_kind(node: Node, kind: &str) -> bool {
    (0..node.child_count())
        .filter_map(|index| node.child(index as u32))
        .any(|child| child.kind() == kind)
}

fn property_name(node: Node, source: &[u8]) -> Result<String, TsonError> {
    let text = node.utf8_text(source).map_err(|_| TsonError::Utf8)?;
    Ok(unquote(text).unwrap_or_else(|| text.to_string()))
}

fn unquote(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let first = chars.next()?;
    let last = text.chars().last()?;
    if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
        Some(text[1..text.len() - 1].to_string())
    } else {
        None
    }
}

fn predefined_schema(text: &str) -> Value {
    match text {
        "string" => json!({ "type": "string" }),
        "number" => json!({ "type": "number" }),
        "boolean" => json!({ "type": "boolean" }),
        "bigint" => json!({ "type": "integer" }),
        "null" => json!({ "type": "null" }),
        "object" => json!({ "type": "object" }),
        "any" | "unknown" => json!({}),
        "undefined" | "void" => json!({ "not": {} }),
        _ => json!({ "description": format!("Unsupported TypeScript primitive: {text}") }),
    }
}

fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "array_type"
            | "generic_type"
            | "literal_type"
            | "nested_type_identifier"
            | "object_type"
            | "parenthesized_type"
            | "predefined_type"
            | "type_identifier"
            | "union_type"
            | "interface_body"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_class_with_nested_object() {
        let source = r#"
            class User {
              name: string;
              age?: number;
              tags: string[];
              address: { street: string; zip?: number };
            }
        "#;

        let schema = schema_from_source(
            source,
            SchemaOptions {
                root_type: Some("User".to_string()),
                additional_properties: false,
            },
        )
        .unwrap();

        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["age"]["type"], "number");
        assert_eq!(schema["properties"]["tags"]["items"]["type"], "string");
        assert_eq!(
            schema["properties"]["address"]["properties"]["street"]["type"],
            "string"
        );
        assert_eq!(schema["required"], json!(["name", "tags", "address"]));
    }

    #[test]
    fn emits_defs_for_referenced_types() {
        let source = r#"
            type Role = "admin" | "user";
            interface Profile { role: Role; active: boolean }
            class User { profile: Profile }
        "#;

        let schema = schema_from_source(
            source,
            SchemaOptions {
                root_type: Some("User".to_string()),
                additional_properties: false,
            },
        )
        .unwrap();

        assert_eq!(schema["properties"]["profile"]["$ref"], "#/$defs/Profile");
        assert_eq!(schema["$defs"]["Role"]["enum"], json!(["admin", "user"]));
        assert_eq!(
            schema["$defs"]["Profile"]["properties"]["role"]["$ref"],
            "#/$defs/Role"
        );
    }

    #[test]
    fn generates_valid_examples_from_schema() {
        let source = r#"
            type Role = "admin" | "user";
            interface Profile { role: Role; active: boolean }
            class User {
              id: string;
              age?: number;
              profile: Profile;
              tags: string[];
            }
        "#;

        let schema = schema_from_source(
            source,
            SchemaOptions {
                root_type: Some("User".to_string()),
                additional_properties: false,
            },
        )
        .unwrap();
        let examples = examples_from_schema(
            &schema,
            ExampleOptions {
                count: 2,
                valid: true,
            },
        );

        assert_eq!(examples.len(), 2);
        assert!(examples[0]["id"].is_string());
        assert!(examples[0]["age"].is_number());
        assert!(examples[0]["profile"]["role"].is_string());
        assert!(examples[0]["profile"]["active"].is_boolean());
        assert!(examples[0]["tags"].is_array());
        assert!(examples[1]["age"].is_null());
    }

    #[test]
    fn generates_invalid_examples_from_schema() {
        let source = r#"
            class User {
              id: string;
              tags: string[];
            }
        "#;

        let schema = schema_from_source(
            source,
            SchemaOptions {
                root_type: Some("User".to_string()),
                additional_properties: false,
            },
        )
        .unwrap();
        let examples = examples_from_schema(
            &schema,
            ExampleOptions {
                count: 3,
                valid: false,
            },
        );

        assert_eq!(examples.len(), 3);
        assert!(examples[0].get("id").is_none());
        assert!(examples[1]["tags"].is_object());
        assert_eq!(examples[2]["__unexpected"], true);
    }
}
