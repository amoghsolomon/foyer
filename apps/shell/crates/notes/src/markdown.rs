//! Conservative Markdown rendering helpers. HTML is never executed.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownBlock {
    Heading { level: u8, text: String },
    ListItem(String),
    Code(String),
    Paragraph(String),
}

pub fn markdown_blocks(source: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut code = String::new();
    let mut in_fence = false;
    let flush_paragraph = |paragraph: &mut String, blocks: &mut Vec<MarkdownBlock>| {
        let text = paragraph.trim();
        if !text.is_empty() {
            blocks.push(MarkdownBlock::Paragraph(text.to_string()));
        }
        paragraph.clear();
    };
    for line in source.replace("\r\n", "\n").lines() {
        if line.trim_start().starts_with("```") {
            if in_fence {
                blocks.push(MarkdownBlock::Code(code.trim_end().to_string()));
                code.clear();
                in_fence = false;
            } else {
                flush_paragraph(&mut paragraph, &mut blocks);
                in_fence = true;
            }
            continue;
        }
        if in_fence {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
            continue;
        }
        if let Some(rest) = heading(line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            blocks.push(rest);
        } else if let Some(text) = line.trim_start().strip_prefix("- ") {
            flush_paragraph(&mut paragraph, &mut blocks);
            blocks.push(MarkdownBlock::ListItem(text.to_string()));
        } else if line.trim().is_empty() {
            flush_paragraph(&mut paragraph, &mut blocks);
        } else {
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(line);
        }
    }
    if in_fence {
        blocks.push(MarkdownBlock::Code(code.trim_end().to_string()));
    }
    flush_paragraph(&mut paragraph, &mut blocks);
    if blocks.is_empty() {
        blocks.push(MarkdownBlock::Paragraph(String::new()));
    }
    blocks
}

fn heading(line: &str) -> Option<MarkdownBlock> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&level) && trimmed.chars().nth(level) == Some(' ') {
        Some(MarkdownBlock::Heading {
            level: level as u8,
            text: trimmed[level + 1..].to_string(),
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_stays_literal() {
        let blocks = markdown_blocks("# Title\n\n<script>alert(1)</script>");
        assert!(matches!(
            &blocks[0],
            MarkdownBlock::Heading { text, .. } if text == "Title"
        ));
        assert!(matches!(
            &blocks[1],
            MarkdownBlock::Paragraph(text) if text.contains("<script>alert(1)</script>")
        ));
    }
}
