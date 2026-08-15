use foyer_shell_protocol::{ChartKind, SlideChart};
use gpui::{AnyElement, Hsla, IntoElement, ParentElement, Styled, div, px, relative, rgb};
use gpui_component::chart::{AreaChart, BarChart, CandlestickChart, PieChart};

#[derive(Clone)]
struct Point {
    label: String,
    values: Vec<f64>,
}

#[derive(Clone)]
struct Candle {
    label: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

pub fn chart(spec: &SlideChart, progress: f32, width: f32, height: f32) -> AnyElement {
    let progress = progress.clamp(0.0, 1.0);
    let points = spec
        .categories
        .iter()
        .enumerate()
        .map(|(index, label)| Point {
            label: label.clone(),
            values: spec
                .series
                .iter()
                .map(|series| series.values.get(index).copied().unwrap_or(0.0) as f64)
                .collect(),
        })
        .collect::<Vec<_>>();
    let tick_margin = (points.len() / 6).max(1);
    let foreground = Hsla::from(rgb(0xf4f4f5));
    let secondary = Hsla::from(rgb(0xa1a1aa));

    let plot = match spec.kind {
        ChartKind::Line | ChartKind::Area => {
            let colors = [foreground, secondary, foreground.opacity(0.56)];
            let mut plot = AreaChart::new(points)
                .x(|point| point.label.clone())
                .tick_margin(tick_margin);
            for index in 0..spec.series.len().min(3) {
                let color = colors[index];
                plot = plot
                    .y(move |point| point.values.get(index).copied().unwrap_or(0.0))
                    .stroke(color)
                    .fill(if spec.kind == ChartKind::Area {
                        color.opacity(0.1)
                    } else {
                        color.opacity(0.0)
                    })
                    .natural();
            }
            plot.into_any_element()
        }
        ChartKind::Bar => {
            let bars = spec
                .categories
                .iter()
                .enumerate()
                .flat_map(|(category_index, category)| {
                    spec.series.iter().map(move |series| Point {
                        label: if spec.series.len() == 1 {
                            category.clone()
                        } else {
                            format!("{category} · {}", series.label)
                        },
                        values: vec![
                            series.values.get(category_index).copied().unwrap_or(0.0) as f64
                        ],
                    })
                })
                .collect::<Vec<_>>();
            BarChart::new(bars)
                .band(|point| point.label.clone())
                .value(|point| point.values.first().copied().unwrap_or(0.0))
                .fill(move |_, _, _, _| foreground)
                .tick_margin(tick_margin)
                .into_any_element()
        }
        ChartKind::Pie | ChartKind::Donut => {
            let colors = [
                foreground,
                secondary,
                foreground.opacity(0.66),
                secondary.opacity(0.48),
            ];
            let donut = spec.kind == ChartKind::Donut;
            PieChart::new(points)
                .value(|point| point.values.first().copied().unwrap_or(0.0).max(0.0) as f32)
                .color(move |point| {
                    let index = point
                        .label
                        .bytes()
                        .fold(0usize, |sum, byte| sum + byte as usize);
                    colors[index % colors.len()]
                })
                .inner_radius(if donut { 58.0 } else { 0.0 })
                .pad_angle(0.025)
                .into_any_element()
        }
        ChartKind::Candlestick => {
            let candles = spec
                .candles
                .iter()
                .map(|candle| Candle {
                    label: candle.label.clone(),
                    open: (candle.open * progress) as f64,
                    high: (candle.high * progress) as f64,
                    low: (candle.low * progress) as f64,
                    close: (candle.close * progress) as f64,
                })
                .collect::<Vec<_>>();
            CandlestickChart::new(candles)
                .x(|candle| candle.label.clone())
                .open(|candle| candle.open)
                .high(|candle| candle.high)
                .low(|candle| candle.low)
                .close(|candle| candle.close)
                .tick_margin((spec.candles.len() / 6).max(1))
                .body_width_ratio(0.62)
                .into_any_element()
        }
    };

    let revealed_width = (width * progress).max(1.0);
    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .child(
            div()
                .w(px(revealed_width))
                .h_full()
                .overflow_hidden()
                .child(div().w(px(width)).h(px(height)).child(plot)),
        )
        .child(
            div()
                .absolute()
                .left(relative(progress))
                .top_0()
                .bottom_0()
                .w(px(1.0))
                .bg(foreground.opacity(0.16 * (1.0 - progress))),
        )
        .into_any_element()
}
