//! Display-only fenced code in conversations. Snippets are not captured file edits.
use super::{document::Document, theme::*};

struct Fence<'a> {
    marker: u8,
    count: usize,
    indent: usize,
    info: &'a str,
}

fn fence(line: &str) -> Option<Fence<'_>> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = *rest.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let count = rest.bytes().take_while(|b| *b == marker).count();
    (count >= 3).then(|| Fence {
        marker,
        count,
        indent,
        info: rest[count..].trim(),
    })
}

/// Deliberately limited to fenced blocks: prose is preserved rather than passed
/// through a lossy Markdown cleanup. Each message is parsed independently.
pub(super) fn render(doc: &mut Document, text: &str) {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(open) = fence(line).filter(|f| f.marker != b'`' || !f.info.contains('`')) else {
            if line.is_empty() {
                doc.gap();
            } else {
                doc.text(line, TEXT);
            }
            continue;
        };
        let mut code = vec![];
        let mut closed = false;
        for line in lines.by_ref() {
            if fence(line).is_some_and(|end| {
                end.marker == open.marker && end.count >= open.count && end.info.is_empty()
            }) {
                closed = true;
                break;
            }
            let indent = line
                .bytes()
                .take_while(|b| *b == b' ')
                .count()
                .min(open.indent);
            code.push(&line[indent..]);
        }
        doc.gap();
        let language = open.info.split_whitespace().next().unwrap_or("text");
        doc.text(format!("{language} · snippet lines"), ACCENT);
        if code.is_empty() {
            doc.text("(empty code block)", MUTED);
        }
        let digits = code.len().max(1).to_string().len().max(3);
        for (index, line) in code.iter().enumerate() {
            doc.code_line(format!("{:>digits$} │ ", index + 1), line, TEXT);
        }
        if !closed {
            doc.text(
                "Unclosed code fence · shown through the end of this message",
                AMBER,
            );
        }
        doc.gap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::document::View;

    fn snippets(doc: &Document) -> Vec<(String, String)> {
        doc.lines
            .iter()
            .enumerate()
            .filter(|(i, _)| doc.code_gutters.contains_key(i))
            .map(|(_, line)| {
                (
                    line.spans[0].content.to_string(),
                    line.spans[1].content.to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn fences_preserve_code_and_leave_prose_wrappable() {
        let mut doc = Document::new(View::SessionWork, "test");
        render(
            &mut doc,
            "Before\n```rust\nfn main() {\n\tprintln!(\"中文 café\");\n\n}\n```\nAfter\n~~~unknown-lang\n  raw text\n~~~",
        );
        let rows = snippets(&doc);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0], ("  1 │ ".into(), "fn main() {".into()));
        assert_eq!(rows[1].1, "    println!(\"中文 café\");");
        assert_eq!(rows[2].1, "");
        assert_eq!(rows[4], ("  1 │ ".into(), "  raw text".into()));
        for text in [
            "Before",
            "After",
            "rust · snippet lines",
            "unknown-lang · snippet lines",
        ] {
            let index = doc
                .lines
                .iter()
                .position(|line| line.to_string() == text)
                .unwrap();
            assert!(!doc.code_gutters.contains_key(&index));
        }
    }

    #[test]
    fn only_matching_fences_close_blocks_and_indented_fences_are_supported() {
        let mut doc = Document::new(View::SessionWork, "test");
        render(
            &mut doc,
            "  ````markdown\n  ```rust\n    code\n  ```\n  ~~~~\n  ```` trailing\n  `````\nAfter",
        );
        let rows = snippets(&doc);
        assert_eq!(
            rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(),
            ["```rust", "  code", "```", "~~~~", "```` trailing"]
        );
        assert!(doc.lines.iter().any(|l| l.to_string() == "After"));
    }

    #[test]
    fn empty_and_unclosed_blocks_do_not_swallow_the_next_message() {
        let mut doc = Document::new(View::SessionWork, "test");
        render(&mut doc, "```\n```\n~~~python\nprint(1)");
        render(&mut doc, "Next message");
        assert_eq!(snippets(&doc).len(), 1);
        let text = doc
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("(empty code block)"));
        assert!(text.contains("Unclosed code fence"));
        let last = doc.lines.len() - 1;
        assert_eq!(doc.lines[last].to_string(), "Next message");
        assert!(!doc.code_gutters.contains_key(&last));
    }

    #[test]
    fn inline_backticks_and_four_space_indented_fences_stay_literal() {
        let mut doc = Document::new(View::SessionWork, "test");
        render(
            &mut doc,
            "Use `inline()`\n    ```rust\n```bad`info\n`` not a fence",
        );
        assert!(doc.code_gutters.is_empty());
        assert_eq!(doc.lines.len(), 4);
    }
}
