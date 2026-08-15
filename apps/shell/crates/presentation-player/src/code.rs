//! Lightweight read-only syntax highlighting for presentation snippets.
//!
//! Foyer Shell does not need Zed's editable buffer, language server, or incremental parse stack.
//! GPUI's native `StyledText` runs are enough for short authored snippets, so this tokenizer keeps
//! startup and memory costs low while covering the common languages emitted by the planner.

use std::ops::Range;

use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, StyledText, rgb};

const KEYWORD: u32 = 0xc4b5fd;
const STRING: u32 = 0x86d9ad;
const NUMBER: u32 = 0xf0b780;
const COMMENT: u32 = 0x72727b;
const TYPE: u32 = 0x8ecae6;
const CONSTANT: u32 = 0xe5c07b;

pub(crate) fn highlighted_code(code: &str, language: Option<&str>) -> StyledText {
    let language = language.unwrap_or("text").to_ascii_lowercase();
    let mut ranges = lexical_ranges(code, &language);
    ranges.sort_by_key(|(range, _)| range.start);
    StyledText::new(code.to_string()).with_highlights(ranges)
}

fn lexical_ranges(code: &str, language: &str) -> Vec<(Range<usize>, HighlightStyle)> {
    let bytes = code.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let line_comment = (byte == b'/' && bytes.get(index + 1) == Some(&b'/'))
            || (byte == b'-' && bytes.get(index + 1) == Some(&b'-'))
            || (byte == b'#' && language != "rust");
        if line_comment {
            let end = code[index..]
                .find('\n')
                .map_or(bytes.len(), |offset| index + offset);
            ranges.push((index..end, style(COMMENT, None, Some(FontStyle::Italic))));
            index = end;
            continue;
        }
        if matches!(byte, b'\'' | b'"' | b'`') {
            let quote = byte;
            let start = index;
            index += 1;
            let mut escaped = false;
            while index < bytes.len() {
                let current = bytes[index];
                index += 1;
                if escaped {
                    escaped = false;
                } else if current == b'\\' {
                    escaped = true;
                } else if current == quote {
                    break;
                }
            }
            ranges.push((start..index, style(STRING, None, None)));
            continue;
        }
        if byte.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || matches!(bytes[index], b'.' | b'_' | b'x'))
            {
                index += 1;
            }
            ranges.push((start..index, style(NUMBER, None, None)));
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let token = &code[start..index];
            if is_keyword(token, language) {
                ranges.push((
                    start..index,
                    style(KEYWORD, Some(FontWeight::SEMIBOLD), None),
                ));
            } else if matches!(token, "true" | "false" | "null" | "None" | "True" | "False") {
                ranges.push((start..index, style(CONSTANT, None, None)));
            } else if token.chars().next().is_some_and(char::is_uppercase) {
                ranges.push((start..index, style(TYPE, None, None)));
            }
            continue;
        }
        // UTF-8 punctuation and identifiers remain neutral, but always advance at a character
        // boundary so GPUI highlight byte ranges stay valid.
        index += code[index..].chars().next().map_or(1, char::len_utf8);
    }
    ranges
}

fn style(color: u32, weight: Option<FontWeight>, font_style: Option<FontStyle>) -> HighlightStyle {
    HighlightStyle {
        color: Some(Hsla::from(rgb(color))),
        font_weight: weight,
        font_style,
        ..Default::default()
    }
}

fn is_keyword(token: &str, language: &str) -> bool {
    let common = matches!(
        token,
        "if" | "else"
            | "for"
            | "while"
            | "return"
            | "break"
            | "continue"
            | "async"
            | "await"
            | "match"
            | "switch"
            | "case"
            | "try"
            | "catch"
            | "throw"
            | "new"
    );
    common
        || match language {
            "rust" | "rs" => matches!(
                token,
                "fn" | "let"
                    | "mut"
                    | "pub"
                    | "impl"
                    | "trait"
                    | "struct"
                    | "enum"
                    | "use"
                    | "mod"
                    | "crate"
                    | "self"
                    | "Self"
                    | "where"
                    | "move"
                    | "ref"
            ),
            "python" | "py" => matches!(
                token,
                "def"
                    | "class"
                    | "import"
                    | "from"
                    | "as"
                    | "in"
                    | "is"
                    | "not"
                    | "and"
                    | "or"
                    | "lambda"
                    | "yield"
                    | "with"
                    | "pass"
                    | "elif"
                    | "except"
            ),
            "typescript" | "ts" | "javascript" | "js" => matches!(
                token,
                "const"
                    | "let"
                    | "var"
                    | "function"
                    | "class"
                    | "interface"
                    | "type"
                    | "extends"
                    | "implements"
                    | "export"
                    | "import"
                    | "from"
                    | "of"
                    | "this"
            ),
            "sql" => matches!(
                token.to_ascii_uppercase().as_str(),
                "SELECT"
                    | "FROM"
                    | "WHERE"
                    | "JOIN"
                    | "ON"
                    | "AS"
                    | "INSERT"
                    | "UPDATE"
                    | "DELETE"
                    | "GROUP"
                    | "ORDER"
                    | "BY"
                    | "LIMIT"
                    | "AND"
                    | "OR"
            ),
            "shell" | "bash" | "sh" => matches!(
                token,
                "then" | "fi" | "do" | "done" | "function" | "local" | "export" | "case" | "esac"
            ),
            _ => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexer_ranges_are_sorted_non_overlapping_and_utf8_safe() {
        let code = "fn café() { let answer = 42; // useful\n}";
        let ranges = lexical_ranges(code, "rust");
        for (index, (range, _)) in ranges.iter().enumerate() {
            assert!(code.is_char_boundary(range.start));
            assert!(code.is_char_boundary(range.end));
            if let Some((previous, _)) = index.checked_sub(1).and_then(|i| ranges.get(i)) {
                assert!(previous.end <= range.start);
            }
        }
    }
}
