//! Deterministic compilation from model-authored semantics into an executable presentation.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use foyer_shell_protocol::{
    ChartKind, CueAction, GraphDirection, NarrationAnchor, PresentationSlide, SlideAxis,
    SlideBlockKind, SlideComposition, SlideGraph, SlideTreeNode,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompileReport {
    pub repaired_ids: usize,
    pub removed_objects: usize,
    pub rebuilt_focus_contract: bool,
}

/// Compile one authored slide. `ordinal` is its zero-based position in the presentation and is
/// used only for deterministic ids and axis normalization.
pub fn compile_slide(slide: &mut PresentationSlide, ordinal: usize) -> CompileReport {
    let mut report = CompileReport::default();
    if slide.id.trim().is_empty() {
        slide.id = format!("slide-{}", ordinal + 1);
        report.repaired_ids += 1;
    }
    slide.axis = if ordinal == 0 {
        SlideAxis::Root
    } else if slide.axis == SlideAxis::Root {
        SlideAxis::Horizontal
    } else {
        slide.axis
    };

    let code_valid = compile_code(slide, ordinal, &mut report);
    if code_valid {
        slide.composition = SlideComposition::Code;
        slide.graph = None;
        slide.blocks.clear();
    } else {
        slide.code = None;
    }

    let graph_valid = !code_valid && compile_graph(slide, ordinal, &mut report);
    if graph_valid {
        slide.composition = SlideComposition::Graph;
        slide.blocks.clear();
    } else if !code_valid {
        slide.graph = None;
        slide.composition = SlideComposition::Bento;
        compile_blocks(slide, ordinal, &mut report);
    }

    compile_focus_contract(slide, &mut report);
    report
}

fn compile_code(slide: &mut PresentationSlide, ordinal: usize, report: &mut CompileReport) -> bool {
    let Some(code) = slide.code.as_mut() else {
        return false;
    };
    code.content = bounded_code(&code.content);
    code.files.truncate(8);
    let mut file_ids = BTreeSet::new();
    for (index, file) in code.files.iter_mut().enumerate() {
        file.content = bounded_code(&file.content);
        if file.id.trim().is_empty() || file_ids.contains(&file.id) {
            file.id = format!("slide-{}-file-{}", ordinal + 1, index + 1);
            report.repaired_ids += 1;
        }
        file_ids.insert(file.id.clone());
    }
    let before = code.files.len();
    code.files
        .retain(|file| !file.path.trim().is_empty() && !file.content.trim().is_empty());
    report.removed_objects += before - code.files.len();
    if code.files.is_empty() && !code.content.trim().is_empty() {
        code.files.push(foyer_shell_protocol::CodeFile {
            id: format!("slide-{}-file-1", ordinal + 1),
            path: "example".into(),
            language: code.language.clone(),
            content: code.content.clone(),
        });
        report.repaired_ids += 1;
    }
    let Some(primary) = code.files.first() else {
        return false;
    };
    code.language = primary.language.clone();
    code.content = primary.content.clone();
    let primary_id = primary.id.clone();
    let file_ids: BTreeSet<_> = code.files.iter().map(|file| file.id.clone()).collect();
    code.steps.truncate(12);
    let mut step_ids = BTreeSet::new();
    for (index, step) in code.steps.iter_mut().enumerate() {
        if step.id.trim().is_empty() || step_ids.contains(&step.id) {
            step.id = format!("slide-{}-code-step-{}", ordinal + 1, index + 1);
            report.repaired_ids += 1;
        }
        step_ids.insert(step.id.clone());
        if step
            .file_id
            .as_ref()
            .is_none_or(|id| !file_ids.contains(id))
        {
            step.file_id = Some(primary_id.clone());
        }
        let source = step
            .file_id
            .as_ref()
            .and_then(|id| code.files.iter().find(|file| file.id == *id))
            .map(|file| file.content.as_str())
            .unwrap_or(code.content.as_str());
        let line_count = source.lines().count().max(1).min(u16::MAX as usize) as u16;
        step.start_line = step.start_line.clamp(1, line_count);
        step.end_line = step.end_line.clamp(step.start_line, line_count);
    }
    if code.steps.is_empty() {
        code.steps.push(foyer_shell_protocol::CodeStep {
            id: format!("slide-{}-code-step-1", ordinal + 1),
            label: Some("The complete example".into()),
            file_id: Some(primary_id),
            start_line: 1,
            end_line: code.content.lines().count().max(1).min(u16::MAX as usize) as u16,
        });
        report.repaired_ids += 1;
    }
    code.show_explorer &= code.files.len() > 1;
    true
}

fn compile_graph(
    slide: &mut PresentationSlide,
    ordinal: usize,
    report: &mut CompileReport,
) -> bool {
    let Some(graph) = slide.graph.as_mut() else {
        return false;
    };
    graph.direction = GraphDirection::LeftToRight;
    graph.nodes.truncate(24);
    let mut node_ids = BTreeSet::new();
    for (index, node) in graph.nodes.iter_mut().enumerate() {
        if node.id.trim().is_empty() || node_ids.contains(&node.id) {
            node.id = format!("slide-{}-node-{}", ordinal + 1, index + 1);
            report.repaired_ids += 1;
        }
        node_ids.insert(node.id.clone());
    }
    let before = graph.nodes.len();
    graph.nodes.retain(|node| !node.label.trim().is_empty());
    report.removed_objects += before - graph.nodes.len();
    let node_ids: BTreeSet<_> = graph.nodes.iter().map(|node| node.id.clone()).collect();
    graph.edges.truncate(48);
    let mut edge_ids = BTreeSet::new();
    for (index, edge) in graph.edges.iter_mut().enumerate() {
        if edge.id.trim().is_empty() || edge_ids.contains(&edge.id) {
            edge.id = format!("slide-{}-edge-{}", ordinal + 1, index + 1);
            report.repaired_ids += 1;
        }
        edge_ids.insert(edge.id.clone());
    }
    let before = graph.edges.len();
    graph.edges.retain(|edge| {
        edge.from != edge.to && node_ids.contains(&edge.from) && node_ids.contains(&edge.to)
    });
    report.removed_objects += before - graph.edges.len();
    graph.nodes.len() >= 2
}

fn compile_blocks(slide: &mut PresentationSlide, ordinal: usize, report: &mut CompileReport) {
    slide.blocks.truncate(7);
    let mut block_ids = BTreeSet::new();
    for (index, block) in slide.blocks.iter_mut().enumerate() {
        if block.id.trim().is_empty() || block_ids.contains(&block.id) {
            block.id = format!("slide-{}-block-{}", ordinal + 1, index + 1);
            report.repaired_ids += 1;
        }
        block_ids.insert(block.id.clone());
        block.columns = block.columns.clamp(1, 9);
        block.rows = block.rows.clamp(1, 9);
    }
    let before = slide.blocks.len();
    slide.blocks.retain_mut(|block| {
        if block.kind == SlideBlockKind::Tree {
            block.chart = None;
            let Some(tree) = block.tree.as_mut() else {
                return false;
            };
            let mut count = 0;
            compile_tree_nodes(&mut tree.nodes, 0, &mut count);
            return !tree.nodes.is_empty();
        }
        block.tree = None;
        if block.kind != SlideBlockKind::Chart {
            block.chart = None;
            return !block.content.trim().is_empty()
                || block.kind == SlideBlockKind::Image && block.uri.is_some();
        }
        let Some(chart) = block.chart.as_mut() else {
            return false;
        };
        chart.categories.truncate(16);
        chart.series.truncate(3);
        chart.series.retain_mut(|series| {
            series.values.truncate(16);
            series.values.retain(|value| value.is_finite());
            !series.label.is_empty() && series.values.len() >= 2
        });
        chart.candles.truncate(16);
        chart.candles.retain(|candle| {
            [candle.open, candle.high, candle.low, candle.close]
                .into_iter()
                .all(f32::is_finite)
                && !candle.label.is_empty()
        });
        if chart.kind == ChartKind::Candlestick {
            return chart.candles.len() >= 2;
        }
        let point_count = chart
            .series
            .iter()
            .map(|series| series.values.len())
            .max()
            .unwrap_or_default();
        while chart.categories.len() < point_count {
            chart
                .categories
                .push((chart.categories.len() + 1).to_string());
        }
        !chart.series.is_empty()
    });
    report.removed_objects += before - slide.blocks.len();
}

fn compile_focus_contract(slide: &mut PresentationSlide, report: &mut CompileReport) {
    let focus_steps: Vec<Vec<String>> = if let Some(code) = slide.code.as_ref() {
        code.steps
            .iter()
            .map(|step| vec![step.id.clone()])
            .collect()
    } else if let Some(graph) = slide.graph.as_ref() {
        graph_traversal(graph)
            .into_iter()
            .map(|(node, incoming)| {
                let mut ids = vec![node];
                if let Some(edge) = incoming {
                    ids.push(edge);
                }
                ids
            })
            .collect()
    } else {
        slide
            .blocks
            .iter()
            .map(|block| vec![block.id.clone()])
            .collect()
    };
    let expected: Vec<_> = focus_steps
        .iter()
        .filter_map(|ids| ids.first().cloned())
        .collect();
    let authored: Vec<_> = slide
        .narration
        .focus
        .first()
        .cloned()
        .into_iter()
        .chain(
            slide
                .narration
                .anchors
                .iter()
                .filter_map(|anchor| match &anchor.cue {
                    CueAction::Focus { ids } if anchor.at_char.is_some() => ids.first().cloned(),
                    _ => None,
                }),
        )
        .collect();
    let contract_is_exact = authored == expected
        && slide.narration.anchors.len() == focus_steps.len().saturating_sub(1);
    slide.narration.focus = focus_steps.first().cloned().unwrap_or_default();
    if contract_is_exact {
        for (anchor, ids) in slide
            .narration
            .anchors
            .iter_mut()
            .zip(focus_steps.into_iter().skip(1))
        {
            anchor.cue = CueAction::Focus { ids };
        }
        return;
    }
    report.rebuilt_focus_contract = true;
    let character_count = slide.narration.text.chars().count();
    let step_count = focus_steps.len().max(1);
    slide.narration.anchors = focus_steps
        .into_iter()
        .skip(1)
        .enumerate()
        .map(|(index, ids)| NarrationAnchor {
            phrase: String::new(),
            at_char: Some(
                (character_count.saturating_mul(index + 1) / step_count).min(u32::MAX as usize)
                    as u32,
            ),
            cue: CueAction::Focus { ids },
        })
        .collect();
}

fn compile_tree_nodes(nodes: &mut Vec<SlideTreeNode>, depth: usize, count: &mut usize) {
    if depth > 5 || *count >= 80 {
        nodes.clear();
        return;
    }
    nodes.truncate(24);
    let mut ids = BTreeSet::new();
    nodes.retain_mut(|node| {
        if node.id.is_empty()
            || node.label.is_empty()
            || !ids.insert(node.id.clone())
            || *count >= 80
        {
            return false;
        }
        *count += 1;
        compile_tree_nodes(&mut node.children, depth + 1, count);
        true
    });
}

pub fn graph_traversal(graph: &SlideGraph) -> Vec<(String, Option<String>)> {
    let order: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();
    let mut indegree: BTreeMap<_, usize> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), 0))
        .collect();
    let mut outgoing: BTreeMap<&str, Vec<&foyer_shell_protocol::GraphEdge>> = BTreeMap::new();
    for edge in &graph.edges {
        *indegree.entry(edge.to.clone()).or_default() += 1;
        outgoing.entry(&edge.from).or_default().push(edge);
    }
    for edges in outgoing.values_mut() {
        edges.sort_by_key(|edge| order.get(edge.to.as_str()).copied().unwrap_or(usize::MAX));
    }
    let mut queue: VecDeque<_> = graph
        .nodes
        .iter()
        .filter(|node| indegree.get(&node.id).copied().unwrap_or_default() == 0)
        .map(|node| (node.id.clone(), None))
        .collect();
    let mut visited = BTreeSet::new();
    let mut traversal = Vec::with_capacity(graph.nodes.len());
    while let Some((id, incoming)) = queue.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        traversal.push((id.clone(), incoming));
        for edge in outgoing.get(id.as_str()).into_iter().flatten() {
            if let Some(degree) = indegree.get_mut(&edge.to) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    queue.push_back((edge.to.clone(), Some(edge.id.clone())));
                }
            }
        }
    }
    for node in &graph.nodes {
        if visited.insert(node.id.clone()) {
            let incoming = graph
                .edges
                .iter()
                .find(|edge| edge.to == node.id && visited.contains(&edge.from))
                .map(|edge| edge.id.clone());
            traversal.push((node.id.clone(), incoming));
        }
    }
    traversal
}

fn bounded_code(content: &str) -> String {
    content.lines().take(240).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_owns_axis_ids_and_focus_order() {
        let mut slide: PresentationSlide = serde_json::from_value(serde_json::json!({
            "id": "",
            "axis": "root",
            "title": "Compiled",
            "composition": "bento",
            "narration": {
                "id": "voice",
                "text": "First claim. Then evidence.",
                "focus": ["wrong"],
                "anchors": []
            },
            "blocks": [
                { "id": "", "kind": "display_text", "content": "Claim", "columns": 99, "rows": 0 },
                { "id": "", "kind": "text", "content": "Evidence", "columns": 3, "rows": 3 }
            ]
        }))
        .unwrap();

        let report = compile_slide(&mut slide, 2);
        assert_eq!(slide.id, "slide-3");
        assert_eq!(slide.axis, SlideAxis::Horizontal);
        assert_eq!(slide.blocks[0].id, "slide-3-block-1");
        assert_eq!(slide.blocks[1].id, "slide-3-block-2");
        assert_eq!(slide.narration.focus, vec!["slide-3-block-1"]);
        assert_eq!(slide.narration.anchors.len(), 1);
        assert!(slide.narration.anchors[0].at_char.is_some());
        assert!(report.rebuilt_focus_contract);
    }

    #[test]
    fn compiler_clamps_code_ranges_without_model_repair() {
        let mut slide: PresentationSlide = serde_json::from_value(serde_json::json!({
            "id": "code",
            "axis": "root",
            "title": "Code",
            "composition": "code",
            "narration": { "id": "voice", "text": "The function.", "focus": [], "anchors": [] },
            "code": {
                "language": "rust",
                "content": "",
                "files": [{ "id": "file", "path": "src/lib.rs", "language": "rust", "content": "fn main() {}" }],
                "steps": [{ "id": "step", "file_id": "file", "start_line": 90, "end_line": 120 }]
            }
        }))
        .unwrap();
        compile_slide(&mut slide, 0);
        let step = &slide.code.unwrap().steps[0];
        assert_eq!((step.start_line, step.end_line), (1, 1));
    }
}
