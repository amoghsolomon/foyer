//! Compact LaTeX math layout for presentation expressions.
//!
//! The parser intentionally targets display-math primitives rather than TeX documents. It handles
//! grouped rows, fractions, roots, scripts, Greek symbols, large operators, and common relations,
//! then lays them out as native GPUI elements with the installed Noto Sans Math font.

use gpui::{Div, FontWeight, Hsla, div, prelude::*, px, relative, rgb};

use crate::theme::{FOREGROUND, MUTED};

#[derive(Clone, Debug, PartialEq)]
enum MathNode {
    Row(Vec<MathNode>),
    Text(String),
    Fraction(Box<MathNode>, Box<MathNode>),
    Root(Box<MathNode>),
    Script {
        base: Box<MathNode>,
        superscript: Option<Box<MathNode>>,
        subscript: Option<Box<MathNode>>,
    },
}

pub(crate) fn math_expression(latex: &str, font_size: f32) -> Div {
    let cleaned = latex.trim().trim_matches('$');
    let mut parser = MathParser::new(cleaned);
    let expression = parser.row(None);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .font_family("Noto Sans Math")
        .text_color(rgb(FOREGROUND))
        .child(render_node(&expression, font_size.max(18.0)))
}

fn render_node(node: &MathNode, size: f32) -> Div {
    match node {
        MathNode::Row(nodes) => div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px((size * 0.075).max(1.0)))
            .children(nodes.iter().map(|node| render_node(node, size))),
        MathNode::Text(text) => div()
            .flex_none()
            .text_size(px(size))
            .line_height(relative(1.05))
            .child(text.clone()),
        MathNode::Fraction(numerator, denominator) => div()
            .flex_none()
            .min_w(px(size * 1.1))
            .flex()
            .flex_col()
            .items_center()
            .child(render_node(numerator, size * 0.72).px_1())
            .child(
                div()
                    .w_full()
                    .border_t_1()
                    .border_color(Hsla::from(rgb(FOREGROUND)).opacity(0.9)),
            )
            .child(render_node(denominator, size * 0.72).px_1()),
        MathNode::Root(value) => div()
            .flex_none()
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(px(size * 1.18))
                    .line_height(relative(0.96))
                    .child("√"),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(Hsla::from(rgb(FOREGROUND)).opacity(0.88))
                    .pt(px(2.0))
                    .child(render_node(value, size * 0.9)),
            ),
        MathNode::Script {
            base,
            superscript,
            subscript,
        } => div()
            .flex_none()
            .flex()
            .items_center()
            .child(render_node(base, size))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .ml(px(1.0))
                    .children(superscript.as_ref().map(|node| {
                        render_node(node, size * 0.58)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(FOREGROUND))
                    }))
                    .children(subscript.as_ref().map(|node| {
                        render_node(node, size * 0.58)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(Hsla::from(rgb(MUTED)).opacity(0.96))
                    })),
            ),
    }
}

struct MathParser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> MathParser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn row(&mut self, terminator: Option<char>) -> MathNode {
        let mut nodes = Vec::new();
        while let Some(character) = self.peek() {
            if Some(character) == terminator {
                self.bump();
                break;
            }
            if character.is_whitespace() {
                self.bump();
                continue;
            }
            let mut atom = self.atom();
            let mut superscript = None;
            let mut subscript = None;
            loop {
                match self.peek() {
                    Some('^') => {
                        self.bump();
                        superscript = Some(Box::new(self.script_value()));
                    }
                    Some('_') => {
                        self.bump();
                        subscript = Some(Box::new(self.script_value()));
                    }
                    _ => break,
                }
            }
            if superscript.is_some() || subscript.is_some() {
                atom = MathNode::Script {
                    base: Box::new(atom),
                    superscript,
                    subscript,
                };
            }
            nodes.push(atom);
        }
        MathNode::Row(nodes)
    }

    fn atom(&mut self) -> MathNode {
        match self.bump() {
            Some('{') => self.row(Some('}')),
            Some('\\') => {
                let command = self.command();
                match command.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        MathNode::Fraction(Box::new(self.group()), Box::new(self.group()))
                    }
                    "sqrt" => MathNode::Root(Box::new(self.group())),
                    "left" | "right" => self.atom(),
                    "," | ";" | "quad" | "qquad" => MathNode::Text(" ".into()),
                    _ => MathNode::Text(command_symbol(&command).to_string()),
                }
            }
            Some(character) => MathNode::Text(character.to_string()),
            None => MathNode::Text(String::new()),
        }
    }

    fn group(&mut self) -> MathNode {
        self.skip_whitespace();
        if self.peek() == Some('{') {
            self.bump();
            self.row(Some('}'))
        } else {
            self.atom()
        }
    }

    fn script_value(&mut self) -> MathNode {
        self.skip_whitespace();
        self.group()
    }

    fn command(&mut self) -> String {
        let start = self.cursor;
        while self.peek().is_some_and(char::is_alphabetic) {
            self.bump();
        }
        if self.cursor == start {
            self.bump();
        }
        self.source[start..self.cursor].to_string()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.cursor += character.len_utf8();
        Some(character)
    }
}

fn command_symbol(command: &str) -> &str {
    match command {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "theta" => "θ",
        "lambda" => "λ",
        "mu" => "μ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "phi" => "φ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Omega" => "Ω",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "times" => "×",
        "cdot" => "·",
        "pm" => "±",
        "mp" => "∓",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "rightarrow" | "to" => "→",
        "Rightarrow" => "⇒",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "cup" => "∪",
        "cap" => "∩",
        "forall" => "∀",
        "exists" => "∃",
        "ldots" | "dots" => "…",
        "text" | "mathrm" | "mathbf" | "mathit" => "",
        unknown => unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_builds_fraction_root_and_scripts() {
        let mut parser = MathParser::new(r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}");
        let parsed = parser.row(None);
        let MathNode::Row(nodes) = parsed else {
            panic!("expected row")
        };
        assert!(matches!(nodes.first(), Some(MathNode::Fraction(_, _))));
    }
}
