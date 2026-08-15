//! Deterministic geometry and typography helpers for slide surfaces.

use std::collections::BTreeMap;

use foyer_shell_protocol::{BlockEmphasis, SlideBlock, SlideBlockKind};
use gpui::{Div, Hsla, ScrollHandle, div, prelude::*, px, rgb};

use crate::theme::{BORDER, FOCUS, GRID_GAP};

const GRID_TRACKS: usize = 9;
const BLOCK_STAGGER: f32 = 0.055;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VisualRect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TypographyFit {
    pub(crate) font_size: f32,
    pub(crate) line_height: f32,
    pub(crate) estimated_lines: usize,
    pub(crate) scroll: bool,
}

pub(crate) fn pack_bento(
    blocks: &[SlideBlock],
    width: f32,
    height: f32,
    seed: &str,
) -> BTreeMap<String, VisualRect> {
    let cell_width = (width - GRID_GAP * (GRID_TRACKS - 1) as f32) / GRID_TRACKS as f32;
    let cell_height = (height - GRID_GAP * (GRID_TRACKS - 1) as f32) / GRID_TRACKS as f32;
    blocks
        .iter()
        .zip(bento_template(blocks.len(), bento_variant(seed, blocks)))
        .map(|(block, (column, row, columns, rows))| {
            (
                block.id.clone(),
                VisualRect {
                    x: column as f32 * (cell_width + GRID_GAP),
                    y: row as f32 * (cell_height + GRID_GAP),
                    width: columns as f32 * cell_width
                        + columns.saturating_sub(1) as f32 * GRID_GAP,
                    height: rows as f32 * cell_height + rows.saturating_sub(1) as f32 * GRID_GAP,
                },
            )
        })
        .collect()
}

fn bento_variant(seed: &str, blocks: &[SlideBlock]) -> usize {
    seed.bytes()
        .chain(blocks.iter().flat_map(|block| block.id.bytes()))
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            hash.wrapping_mul(0x100000001b3) ^ byte as u64
        }) as usize
}

fn bento_template(count: usize, variant: usize) -> Vec<(usize, usize, usize, usize)> {
    match count.clamp(1, 7) {
        1 => vec![(0, 0, 9, 9)],
        2 => match variant % 3 {
            0 => vec![(0, 0, 6, 9), (6, 0, 3, 9)],
            1 => vec![(0, 0, 5, 9), (5, 0, 4, 9)],
            _ => vec![(0, 0, 4, 9), (4, 0, 5, 9)],
        },
        3 => match variant % 5 {
            0 => vec![(0, 0, 5, 9), (5, 0, 4, 4), (5, 4, 4, 5)],
            1 => vec![(0, 0, 6, 5), (6, 0, 3, 5), (0, 5, 9, 4)],
            2 => vec![(0, 0, 4, 9), (4, 0, 5, 5), (4, 5, 5, 4)],
            3 => vec![(0, 0, 3, 6), (3, 0, 6, 6), (0, 6, 9, 3)],
            _ => vec![(0, 0, 9, 4), (0, 4, 5, 5), (5, 4, 4, 5)],
        },
        4 => match variant % 8 {
            0 => vec![(0, 0, 6, 5), (6, 0, 3, 5), (0, 5, 4, 4), (4, 5, 5, 4)],
            1 => vec![(0, 0, 5, 9), (5, 0, 4, 3), (5, 3, 4, 3), (5, 6, 4, 3)],
            2 => vec![(0, 0, 9, 4), (0, 4, 3, 5), (3, 4, 3, 5), (6, 4, 3, 5)],
            3 => vec![(0, 0, 6, 6), (6, 0, 3, 6), (0, 6, 5, 3), (5, 6, 4, 3)],
            4 => vec![(0, 0, 4, 5), (4, 0, 5, 5), (0, 5, 6, 4), (6, 5, 3, 4)],
            5 => vec![(0, 0, 3, 9), (3, 0, 6, 3), (3, 3, 6, 3), (3, 6, 6, 3)],
            6 => vec![(0, 0, 7, 4), (7, 0, 2, 4), (0, 4, 4, 5), (4, 4, 5, 5)],
            _ => vec![(0, 0, 5, 6), (5, 0, 4, 6), (0, 6, 3, 3), (3, 6, 6, 3)],
        },
        5 => match variant % 4 {
            0 => vec![
                (0, 0, 4, 5),
                (4, 0, 3, 5),
                (7, 0, 2, 5),
                (0, 5, 5, 4),
                (5, 5, 4, 4),
            ],
            1 => vec![
                (0, 0, 6, 4),
                (6, 0, 3, 4),
                (0, 4, 3, 5),
                (3, 4, 3, 5),
                (6, 4, 3, 5),
            ],
            2 => vec![
                (0, 0, 3, 6),
                (3, 0, 3, 6),
                (6, 0, 3, 6),
                (0, 6, 4, 3),
                (4, 6, 5, 3),
            ],
            _ => vec![
                (0, 0, 5, 5),
                (5, 0, 4, 5),
                (0, 5, 3, 4),
                (3, 5, 2, 4),
                (5, 5, 4, 4),
            ],
        },
        6 => match variant % 4 {
            0 => vec![
                (0, 0, 4, 5),
                (4, 0, 2, 5),
                (6, 0, 3, 5),
                (0, 5, 3, 4),
                (3, 5, 3, 4),
                (6, 5, 3, 4),
            ],
            1 => vec![
                (0, 0, 3, 4),
                (3, 0, 3, 4),
                (6, 0, 3, 4),
                (0, 4, 4, 5),
                (4, 4, 2, 5),
                (6, 4, 3, 5),
            ],
            2 => vec![
                (0, 0, 5, 5),
                (5, 0, 2, 5),
                (7, 0, 2, 5),
                (0, 5, 2, 4),
                (2, 5, 3, 4),
                (5, 5, 4, 4),
            ],
            _ => vec![
                (0, 0, 2, 6),
                (2, 0, 4, 6),
                (6, 0, 3, 6),
                (0, 6, 3, 3),
                (3, 6, 3, 3),
                (6, 6, 3, 3),
            ],
        },
        _ => match variant % 3 {
            0 => vec![
                (0, 0, 4, 5),
                (4, 0, 3, 5),
                (7, 0, 2, 5),
                (0, 5, 3, 4),
                (3, 5, 2, 4),
                (5, 5, 2, 4),
                (7, 5, 2, 4),
            ],
            1 => vec![
                (0, 0, 3, 4),
                (3, 0, 4, 4),
                (7, 0, 2, 4),
                (0, 4, 2, 5),
                (2, 4, 2, 5),
                (4, 4, 3, 5),
                (7, 4, 2, 5),
            ],
            _ => vec![
                (0, 0, 2, 5),
                (2, 0, 3, 5),
                (5, 0, 2, 5),
                (7, 0, 2, 5),
                (0, 5, 3, 4),
                (3, 5, 3, 4),
                (6, 5, 3, 4),
            ],
        },
    }
}

pub(crate) fn wrap_text_to_width(
    text: &str,
    width: f32,
    font_size: f32,
    monospace: bool,
) -> String {
    let glyph_width = font_size * if monospace { 0.64 } else { 0.58 };
    let capacity = (width / glyph_width).floor().max(1.0) as usize;
    text.lines()
        .map(|line| wrap_line(line, capacity))
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_line(line: &str, capacity: usize) -> String {
    let mut output = String::new();
    let mut used = 0usize;
    for word in line.split_whitespace() {
        let mut remainder = word;
        while remainder.chars().count() > capacity {
            if used > 0 {
                output.push('\n');
            }
            let byte_index = remainder
                .char_indices()
                .nth(capacity)
                .map_or(remainder.len(), |(index, _)| index);
            output.push_str(&remainder[..byte_index]);
            output.push('\n');
            remainder = &remainder[byte_index..];
            used = 0;
        }
        let word_length = remainder.chars().count();
        if used > 0 && used + 1 + word_length > capacity {
            output.push('\n');
            used = 0;
        }
        if used > 0 {
            output.push(' ');
            used += 1;
        }
        output.push_str(remainder);
        used += word_length;
    }
    output
}

pub(crate) fn fit_title_size(text: &str, width: f32, height: f32) -> f32 {
    [34.0, 30.0, 27.0, 24.0, 21.0]
        .into_iter()
        .find(|font_size| {
            let lines = estimate_wrapped_lines(text, width, *font_size, false);
            lines as f32 * *font_size * 1.08 <= height
        })
        .unwrap_or(21.0)
}

pub(crate) fn fit_typography(block: &SlideBlock, rect: VisualRect) -> TypographyFit {
    let padding = 48.0;
    let title_height = if block.title.is_some() { 24.0 } else { 0.0 };
    let available_width = (rect.width - padding - 8.0).max(64.0);
    let available_height = (rect.height - padding - title_height).max(32.0);
    let (sizes, line_height, monospace): (&[f32], f32, bool) = match block.kind {
        SlideBlockKind::DisplayText => (&[46.0, 40.0, 35.0, 30.0, 26.0, 22.0], 1.1, false),
        SlideBlockKind::Statistic => (&[52.0, 46.0, 40.0, 34.0, 29.0, 25.0], 1.08, false),
        SlideBlockKind::Equation => (&[34.0, 30.0, 27.0, 24.0, 21.0, 18.0], 1.2, false),
        SlideBlockKind::Code => (&[16.0, 15.0, 14.0, 13.0, 12.0], 1.48, true),
        SlideBlockKind::Chart | SlideBlockKind::Tree => (&[16.0], 1.3, false),
        SlideBlockKind::Callout => (&[21.0, 19.0, 17.0, 15.0, 14.0], 1.42, false),
        SlideBlockKind::Text | SlideBlockKind::Image => match block.emphasis {
            BlockEmphasis::Quiet => (&[16.0, 15.0, 14.0, 13.0], 1.46, false),
            BlockEmphasis::Normal => (&[19.0, 18.0, 17.0, 16.0, 15.0, 14.0], 1.46, false),
            BlockEmphasis::Strong => (&[23.0, 21.0, 19.0, 17.0, 15.0], 1.4, false),
        },
    };
    for font_size in sizes {
        let lines = estimate_wrapped_lines(&block.content, available_width, *font_size, monospace);
        if lines as f32 * *font_size * line_height <= available_height {
            return TypographyFit {
                font_size: *font_size,
                line_height,
                estimated_lines: lines,
                scroll: false,
            };
        }
    }
    let font_size = *sizes.last().expect("every typography ladder has a minimum");
    let estimated_lines =
        estimate_wrapped_lines(&block.content, available_width, font_size, monospace);
    TypographyFit {
        font_size,
        line_height,
        estimated_lines,
        scroll: estimated_lines as f32 * font_size * line_height > available_height,
    }
}

pub(crate) fn estimate_wrapped_lines(
    text: &str,
    width: f32,
    font_size: f32,
    monospace: bool,
) -> usize {
    let glyph_width = font_size * if monospace { 0.62 } else { 0.53 };
    let capacity = (width / glyph_width).floor().max(1.0) as usize;
    text.lines()
        .map(|line| {
            if line.is_empty() {
                return 1;
            }
            let mut lines = 1usize;
            let mut used = 0usize;
            for word in line.split_whitespace() {
                let word_len = word.chars().count();
                let required = word_len + usize::from(used > 0);
                if used > 0 && used + required > capacity {
                    lines += 1;
                    used = word_len;
                } else {
                    used += required;
                }
                if used > capacity {
                    lines += (used - 1) / capacity;
                    used = (used - 1) % capacity + 1;
                }
            }
            lines
        })
        .sum::<usize>()
        .max(1)
}

pub(crate) fn scroll_indicator(
    handle: Option<&ScrollHandle>,
    rect: VisualRect,
    fit: TypographyFit,
) -> Div {
    let track_height = (rect.height - 48.0).max(24.0);
    let estimated_content = fit.estimated_lines as f32 * fit.font_size * fit.line_height;
    let viewport_height = (rect.height - 48.0).max(24.0);
    let estimated_ratio =
        (viewport_height / estimated_content.max(viewport_height)).clamp(0.12, 1.0);
    let (ratio, progress) = handle.map_or((estimated_ratio, 0.0), |handle| {
        let maximum = f32::from(handle.max_offset().y);
        let viewport = f32::from(handle.bounds().size.height);
        if maximum <= 0.5 || viewport <= 0.5 {
            (estimated_ratio, 0.0)
        } else {
            let ratio = (viewport / (viewport + maximum)).clamp(0.12, 1.0);
            let progress = (-f32::from(handle.offset().y) / maximum).clamp(0.0, 1.0);
            (ratio, progress)
        }
    });
    let thumb_height = (track_height * ratio).max(14.0);
    let thumb_top = (track_height - thumb_height) * progress;
    div()
        .absolute()
        .right(px(7.0))
        .top(px(24.0))
        .w(px(3.0))
        .h(px(track_height))
        .rounded_full()
        .bg(Hsla::from(rgb(BORDER)).opacity(0.42))
        .child(
            div()
                .absolute()
                .top(px(thumb_top))
                .w_full()
                .h(px(thumb_height))
                .rounded_full()
                .bg(Hsla::from(rgb(FOCUS)).opacity(0.72)),
        )
}

pub(crate) fn block_reveal_progress(slide_progress: f32, index: usize) -> f32 {
    let start = index as f32 * BLOCK_STAGGER;
    ease_out_cubic(((slide_progress - start) / (1.0 - start).max(0.2)).clamp(0.0, 1.0))
}

fn ease_out_cubic(value: f32) -> f32 {
    1.0 - (1.0 - value).powi(3)
}

pub(crate) fn ease_in_out_cubic(value: f32) -> f32 {
    if value < 0.5 {
        4.0 * value.powi(3)
    } else {
        1.0 - (-2.0 * value + 2.0).powi(3) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_block(id: &str) -> SlideBlock {
        SlideBlock {
            id: id.into(),
            kind: SlideBlockKind::Text,
            title: None,
            content: id.into(),
            uri: None,
            language: None,
            chart: None,
            tree: None,
            columns: 3,
            rows: 3,
            emphasis: BlockEmphasis::Normal,
        }
    }

    #[test]
    fn four_card_bentos_have_stable_but_varied_silhouettes() {
        let blocks = [
            test_block("one"),
            test_block("two"),
            test_block("three"),
            test_block("four"),
        ];
        let mut layouts = std::collections::BTreeSet::new();
        for index in 0..32 {
            let layout = pack_bento(&blocks, 900.0, 600.0, &format!("slide-{index}"));
            layouts.insert(
                layout
                    .values()
                    .map(|rect| {
                        (
                            rect.x.round() as i32,
                            rect.y.round() as i32,
                            rect.width.round() as i32,
                            rect.height.round() as i32,
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            assert_eq!(
                layout,
                pack_bento(&blocks, 900.0, 600.0, &format!("slide-{index}"))
            );
        }
        assert!(layouts.len() >= 3);
    }

    #[test]
    fn every_bento_variant_preserves_authored_left_to_right_read_order() {
        for count in 1..=7 {
            for variant in 0..64 {
                let template = bento_template(count, variant);
                assert_eq!(template.len(), count);
                for pair in template.windows(2) {
                    let (left_column, left_row, _, _) = pair[0];
                    let (right_column, right_row, _, _) = pair[1];
                    assert!(
                        (left_row, left_column) <= (right_row, right_column),
                        "count {count}, variant {variant} changed read order: {template:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn long_display_text_is_measured_as_wrapped_content() {
        let block = SlideBlock {
            id: "headline".into(),
            kind: SlideBlockKind::DisplayText,
            title: None,
            content: "A narrated presentation that moves through time and into progressively deeper detail"
                .into(),
            uri: None,
            language: None,
            chart: None,
            tree: None,
            columns: 9,
            rows: 4,
            emphasis: BlockEmphasis::Strong,
        };
        let fit = fit_typography(
            &block,
            VisualRect {
                x: 0.0,
                y: 0.0,
                width: 860.0,
                height: 240.0,
            },
        );
        assert!(fit.estimated_lines >= 2);
        assert!(!fit.scroll);
    }

    #[test]
    fn long_unbroken_text_is_counted_across_multiple_lines() {
        assert!(estimate_wrapped_lines(&"x".repeat(120), 320.0, 24.0, false) >= 5);
    }

    #[test]
    fn wrapped_text_contains_hard_boundaries_for_gpui() {
        let wrapped = wrap_text_to_width(
            "Everyday places can hide extraordinary missions in plain sight",
            320.0,
            32.0,
            false,
        );
        assert!(wrapped.contains('\n'));
        assert!(wrapped.lines().all(|line| line.chars().count() <= 17));
    }
}
