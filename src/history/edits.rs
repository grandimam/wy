//! Static extraction of recorded edits. Session code is never evaluated or applied.
use crate::{arr, s, security};
use serde_json::{Value, json};
use tree_sitter::Node;

fn edit(file: &str, format: &str, operation: &str, text: &str) -> Value {
    let text = security::redact(text);
    json!({"file":security::redact(file),"format":format,"operation":operation,
        "text":security::short(&text, 40000),"truncated":text.chars().count()>40000})
}

fn patch(text: &str) -> Vec<Value> {
    let mut result = vec![];
    let mut file = String::new();
    let mut operation = "edit";
    let mut lines = vec![];
    let flush = |file: &str, operation: &str, lines: &[&str], result: &mut Vec<Value>| {
        if file.is_empty() {
            return;
        }
        let text = if operation == "add" {
            lines
                .iter()
                .filter_map(|l| l.strip_prefix('+'))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            lines.join("\n")
        };
        result.push(edit(
            file,
            if operation == "add" { "code" } else { "patch" },
            operation,
            &text,
        ));
    };
    for line in text.lines() {
        let header = [
            ("*** Add File: ", "add"),
            ("*** Update File: ", "edit"),
            ("*** Delete File: ", "delete"),
        ]
        .into_iter()
        .find_map(|(prefix, op)| line.strip_prefix(prefix).map(|f| (f, op)));
        if let Some((name, op)) = header {
            flush(&file, operation, &lines, &mut result);
            file = name.into();
            operation = op;
            lines.clear();
        } else if let Some(name) = line.strip_prefix("*** Move to: ") {
            lines.push(line);
            file = name.into();
            operation = "move";
        } else if line == "*** End Patch" {
            flush(&file, operation, &lines, &mut result);
            file.clear();
            lines.clear();
        } else if !file.is_empty() && line != "*** End of File" {
            lines.push(line);
        }
    }
    flush(&file, operation, &lines, &mut result);
    result.truncate(64);
    result
}

// Decode a literal, never an expression. In particular, do not execute template interpolation.
fn literal(node: Node<'_>, input: &str) -> Option<String> {
    if !["string", "template_string"].contains(&node.kind()) {
        return None;
    }
    let text = node.utf8_text(input.as_bytes()).ok()?;
    if text.starts_with('"') {
        return serde_json::from_str(text).ok();
    }
    if text.starts_with('`') && text.contains("${") {
        return None;
    }
    let body = text.get(1..text.len().checked_sub(1)?)?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '\n' => {}
            c @ ('\\' | '\'' | '"' | '`' | '$') => out.push(c),
            _ => return None,
        }
    }
    Some(out)
}

fn wrapped(input: &str) -> Vec<Value> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .is_err()
    {
        return vec![];
    }
    let Some(tree) = parser.parse(input, None) else {
        return vec![];
    };
    let mut result = vec![];
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        if node.kind() == "call_expression" {
            let name = node
                .child_by_field_name("function")
                .and_then(|n| n.utf8_text(input.as_bytes()).ok())
                .unwrap_or("");
            if matches!(name, "tools.apply_patch" | "apply_patch") {
                if let Some(text) = node
                    .child_by_field_name("arguments")
                    .and_then(|n| n.named_child(0))
                    .and_then(|n| literal(n, input))
                {
                    result.extend(patch(&text));
                }
            }
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        nodes.extend(children.into_iter().rev());
    }
    result.truncate(64);
    result
}

pub(super) fn extract(tool: &str, text: &str) -> Vec<Value> {
    let tool = tool.rsplit('.').next().unwrap_or(tool);
    if tool == "apply_patch" {
        let input: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        return patch(
            input
                .as_str()
                .or_else(|| input["patch"].as_str())
                .or_else(|| input["input"].as_str())
                .unwrap_or(text),
        );
    }
    if tool == "exec" {
        return wrapped(text);
    }
    if ["Write", "Edit", "MultiEdit"].contains(&tool) {
        let Ok(input) = serde_json::from_str::<Value>(text) else {
            return vec![];
        };
        let Some(file) = input["file_path"].as_str() else {
            return vec![];
        };
        if tool == "Write" {
            return input["content"]
                .as_str()
                .map(|t| vec![edit(file, "code", "write", t)])
                .unwrap_or_default();
        }
        let changes = if tool == "MultiEdit" {
            arr(&input["edits"]).to_vec()
        } else {
            vec![input.clone()]
        };
        let mut patches = vec![];
        for change in changes {
            if let (Some(old), Some(new)) =
                (change["old_string"].as_str(), change["new_string"].as_str())
            {
                // These line numbers are relative to the recorded replacement, not the file.
                patches.push(
                    similar::TextDiff::from_lines(old, new)
                        .unified_diff()
                        .context_radius(3)
                        .to_string(),
                );
            }
        }
        if !patches.is_empty() {
            return vec![edit(file, "patch", "edit", &patches.join("\n"))];
        }
    }
    if tool == "file_change" {
        if let Ok(changes) = serde_json::from_str::<Value>(text) {
            return arr(&changes)
                .iter()
                .filter_map(|c| {
                    c["path"]
                        .as_str()
                        .map(|file| edit(file, "patch", s(&c["kind"]), s(&c["diff"])))
                })
                .take(64)
                .collect();
        }
    }
    vec![]
}
