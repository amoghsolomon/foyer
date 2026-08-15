//! Versioned messages exchanged between the agent harness, presentation director, TTS, and renderer.
//!
//! The model emits semantic meaning. It never emits pixel coordinates, animation curves, or
//! individual draw calls. Those decisions belong to the director and presentation runtime.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u16 = 9;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub version: u16,
    pub sequence: u64,
    pub elapsed_ms: u64,
    #[serde(flatten)]
    pub event: WorkEvent,
}

impl EventEnvelope {
    pub fn new(sequence: u64, elapsed_ms: u64, event: WorkEvent) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            sequence,
            elapsed_ms,
            event,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkEvent {
    SessionStarted {
        session_id: String,
        goal: String,
    },
    PlanDeclared {
        goal: String,
        steps: Vec<PlanStep>,
    },
    Checkpoint {
        id: String,
        #[serde(default)]
        step_id: Option<String>,
        kind: CheckpointKind,
        title: String,
        body: String,
        #[serde(default)]
        status: Option<PlanStepStatus>,
        #[serde(default)]
        related_to: Vec<String>,
        #[serde(default)]
        source: Option<SourceRef>,
    },
    ArtifactProduced {
        id: String,
        title: String,
        kind: ArtifactKind,
        #[serde(default)]
        uri: Option<String>,
        #[serde(default)]
        preview: Option<String>,
        #[serde(default)]
        media_type: Option<String>,
        #[serde(default)]
        related_to: Vec<String>,
    },
    PresentationObject {
        id: String,
        primitive: PresentationPrimitive,
        role: SemanticRole,
        title: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        uri: Option<String>,
        #[serde(default)]
        latex: Option<String>,
        #[serde(default)]
        related_to: Vec<String>,
    },
    AnnotationAdded {
        id: String,
        target_id: String,
        kind: AnnotationKind,
        #[serde(default)]
        label: Option<String>,
    },
    CameraDirected {
        action: CameraAction,
        #[serde(default)]
        target_ids: Vec<String>,
        #[serde(default = "default_camera_padding")]
        padding: f32,
    },
    Intent {
        id: String,
        label: String,
        #[serde(default)]
        parent_id: Option<String>,
    },
    ToolStarted {
        call_id: String,
        tool: String,
        #[serde(default)]
        public_summary: Option<String>,
    },
    ToolCompleted {
        call_id: String,
        tool: String,
        success: bool,
        duration_ms: u64,
        #[serde(default)]
        public_summary: Option<String>,
    },
    Observation {
        id: String,
        label: String,
        body: String,
        #[serde(default)]
        source: Option<SourceRef>,
        #[serde(default)]
        relates_to: Vec<String>,
    },
    Evidence {
        id: String,
        label: String,
        excerpt: String,
        source: SourceRef,
        #[serde(default)]
        supports: Vec<String>,
        #[serde(default = "default_confidence")]
        confidence: f32,
    },
    Decision {
        id: String,
        label: String,
        presentation: String,
        #[serde(default)]
        based_on: Vec<String>,
        #[serde(default = "default_confidence")]
        confidence: f32,
    },
    Uncertainty {
        id: String,
        question: String,
        #[serde(default)]
        related_to: Vec<String>,
    },
    NarrationProposed {
        beat_id: String,
        text: String,
        #[serde(default)]
        focus: Vec<String>,
        #[serde(default)]
        anchors: Vec<NarrationAnchor>,
    },
    StageBeatPlanned {
        beat: StageBeat,
    },
    SlidePlanned {
        slide: PresentationSlide,
    },
    PresenceProposed {
        cue: PresenceCue,
    },
    SessionCompleted {
        #[serde(default)]
        status: CompletionStatus,
        summary: String,
        #[serde(default)]
        answer_markdown: String,
        #[serde(default)]
        artifact_ids: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub label: String,
    pub status: PlanStepStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    Active,
    Completed,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKind {
    Progress,
    Observation,
    Evidence,
    Decision,
    Uncertainty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Text,
    Markdown,
    Code,
    Image,
    Table,
    Chart,
    Equation,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationPrimitive {
    Text,
    Card,
    Image,
    Equation,
    Code,
    Table,
    Callout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationKind {
    Circle,
    Underline,
    Highlight,
    Arrow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAction {
    Frame,
    Focus,
    #[default]
    ShowAll,
    Follow,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    #[default]
    Completed,
    Partial,
    Failed,
    Cancelled,
}

fn default_confidence() -> f32 {
    1.0
}

fn default_camera_padding() -> f32 {
    112.0
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub uri: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub media_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NarrationAnchor {
    /// Text fragment or semantic anchor. The audio runtime resolves this against final PCM time.
    pub phrase: String,
    /// Exact UTF-8 character offset in the final narration, when the planner authored segmented
    /// speech. This takes precedence over phrase matching and keeps visual focus deterministic.
    #[serde(default)]
    pub at_char: Option<u32>,
    pub cue: CueAction,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CueAction {
    Focus { ids: Vec<String> },
    Emphasize { ids: Vec<String> },
    Recede { ids: Vec<String> },
    FollowPath { ids: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentationPatch {
    pub sequence: u64,
    pub source_event_sequence: u64,
    pub beat_id: Option<String>,
    pub operations: Vec<PresentationOp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PresentationOp {
    Upsert {
        object: PresentationObject,
        transition: Transition,
    },
    Connect {
        relation: Relation,
        transition: Transition,
    },
    Remove {
        id: String,
        transition: Transition,
    },
    SetFocus {
        ids: Vec<String>,
        transition: Transition,
    },
    Recede {
        ids: Vec<String>,
        amount: f32,
        transition: Transition,
    },
    SetLayout {
        layout: LayoutIntent,
        ids: Vec<String>,
        transition: Transition,
    },
    SetCamera {
        action: CameraAction,
        target_ids: Vec<String>,
        padding: f32,
        transition: Transition,
    },
    Annotate {
        annotation: Annotation,
        transition: Transition,
    },
    QueueNarration {
        beat: NarrationBeat,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentationObject {
    pub id: String,
    pub primitive: Primitive,
    pub role: SemanticRole,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub source: Option<SourceRef>,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    Text,
    Shape,
    Card,
    SourceCard,
    CodeBlock,
    Image,
    Badge,
    Group,
    Callout,
    Table,
    Chart,
    Progress,
    Equation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub target_id: String,
    pub kind: AnnotationKind,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRole {
    Goal,
    Intent,
    Activity,
    Observation,
    Evidence,
    Decision,
    Uncertainty,
    Result,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: String,
    pub kind: RelationKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Supports,
    Contradicts,
    Causes,
    DependsOn,
    BelongsTo,
    Mentions,
    Sequence,
    Related,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutIntent {
    Focus,
    Flow,
    Cluster,
    Comparison,
    Timeline,
    EvidenceStack,
    Gallery,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NarrationBeat {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub style: NarrationStyle,
    #[serde(default = "default_style_degree")]
    pub style_degree: f32,
    pub focus: Vec<String>,
    pub anchors: Vec<NarrationAnchor>,
}

/// A viewport-sized authored presentation surface. Slides form a deterministic two-axis map:
/// horizontal advances time, while vertical adds detail at the same point in time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentationSlide {
    pub id: String,
    pub axis: SlideAxis,
    pub title: String,
    #[serde(default)]
    pub eyebrow: Option<String>,
    #[serde(default)]
    pub composition: SlideComposition,
    pub narration: NarrationBeat,
    #[serde(default)]
    pub blocks: Vec<SlideBlock>,
    /// Present only for the full-viewport graph composition. The model describes meaning and
    /// connectivity; the native director owns measurement, layout, routing, and camera fitting.
    #[serde(default)]
    pub graph: Option<SlideGraph>,
    /// Present only for the immersive, full-viewport code composition. The authored steps are
    /// semantic line ranges; the native director owns scrolling, emphasis, and editor styling.
    #[serde(default)]
    pub code: Option<SlideCode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlideAxis {
    #[default]
    Root,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlideComposition {
    Hero,
    Split,
    Stack,
    Comparison,
    Media,
    Graph,
    Code,
    #[default]
    Bento,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideCode {
    #[serde(default)]
    pub language: String,
    pub content: String,
    /// Optional authored file set. When empty, `content` is treated as a single anonymous file.
    #[serde(default)]
    pub files: Vec<CodeFile>,
    /// Shows a compact project explorer beside the editor when multiple files matter.
    #[serde(default)]
    pub show_explorer: bool,
    #[serde(default)]
    pub steps: Vec<CodeStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeStep {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    /// File targeted by this step. Omitted for legacy single-file presentations.
    #[serde(default)]
    pub file_id: Option<String>,
    /// One-based inclusive source line.
    pub start_line: u16,
    /// One-based inclusive source line.
    pub end_line: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFile {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub language: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideGraph {
    #[serde(default)]
    pub direction: GraphDirection,
    #[serde(default)]
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    #[default]
    LeftToRight,
    TopToBottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub role: GraphNodeRole,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeRole {
    Source,
    Process,
    Decision,
    Evidence,
    Outcome,
    Constraint,
    #[default]
    Concept,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub relation: GraphRelation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphRelation {
    FlowsTo,
    Causes,
    DependsOn,
    Supports,
    Contradicts,
    Sequence,
    #[default]
    Related,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideBlock {
    pub id: String,
    pub kind: SlideBlockKind,
    #[serde(default)]
    pub title: Option<String>,
    pub content: String,
    #[serde(default)]
    pub uri: Option<String>,
    /// Optional language hint for syntax-highlighted code blocks, such as `rust` or `typescript`.
    #[serde(default)]
    pub language: Option<String>,
    /// Structured chart data for a chart bento card. Charts remain a single focus target: the
    /// narrator focuses the containing block rather than individual series or points.
    #[serde(default)]
    pub chart: Option<SlideChart>,
    /// Structured hierarchy for a project/file tree bento card.
    #[serde(default)]
    pub tree: Option<SlideTree>,
    #[serde(default = "default_grid_columns")]
    pub columns: u8,
    #[serde(default = "default_grid_rows")]
    pub rows: u8,
    #[serde(default)]
    pub emphasis: BlockEmphasis,
}

fn default_grid_columns() -> u8 {
    3
}

fn default_grid_rows() -> u8 {
    3
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlideBlockKind {
    DisplayText,
    Text,
    Image,
    Equation,
    Code,
    Statistic,
    Callout,
    Chart,
    Tree,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideChart {
    #[serde(default)]
    pub kind: ChartKind,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub series: Vec<ChartSeries>,
    #[serde(default)]
    pub candles: Vec<ChartCandle>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    #[default]
    Line,
    Area,
    Bar,
    Pie,
    Donut,
    Candlestick,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartSeries {
    pub label: String,
    #[serde(default)]
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartCandle {
    pub label: String,
    pub open: f32,
    pub high: f32,
    pub low: f32,
    pub close: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlideTree {
    #[serde(default)]
    pub nodes: Vec<SlideTreeNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlideTreeNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub children: Vec<SlideTreeNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEmphasis {
    Quiet,
    #[default]
    Normal,
    Strong,
}

fn default_style_degree() -> f32 {
    1.0
}

/// A small, portable delivery vocabulary chosen by the presentation planner. Audio providers translate
/// these intents into their own controls; model-specific markup never leaks into presentation state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NarrationStyle {
    #[default]
    Neutral,
    Angry,
    Confused,
    Determined,
    Embarrassed,
    Excited,
    Happy,
    Hopeful,
    Joyful,
    Regretful,
    Relieved,
    Sad,
    Shouting,
    Softvoice,
    Whispering,
}

/// A coherent authored presentation unit. Unlike trace events, a stage beat is allowed to mutate
/// the persistent board and is synchronized to its narration clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StageBeat {
    pub id: String,
    #[serde(default)]
    pub narration: Option<NarrationBeat>,
    #[serde(default)]
    pub actions: Vec<StageAction>,
}

/// Ephemeral speech used only to acknowledge latency. Presence cues never create board objects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceCue {
    pub id: String,
    pub text: String,
    pub tone: PresenceTone,
    /// Visual activity updates are frequent; only cadence-controlled cues should enter TTS.
    #[serde(default = "default_presence_speak")]
    pub speak: bool,
    #[serde(default = "default_presence_ttl")]
    pub expires_after_ms: u32,
}

fn default_presence_speak() -> bool {
    true
}

fn default_presence_ttl() -> u32 {
    4_000
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceTone {
    Checking,
    Comparing,
    Acknowledging,
    Transitioning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StageAction {
    Place {
        id: String,
        kind: StageObjectKind,
        content: String,
        #[serde(default)]
        uri: Option<String>,
        placement: PlacementIntent,
        entrance: EntranceIntent,
    },
    Annotate {
        id: String,
        target_id: String,
        kind: AnnotationKind,
        #[serde(default)]
        label: Option<String>,
    },
    Arrange {
        ids: Vec<String>,
        layout: LayoutIntent,
    },
    Camera {
        action: CameraAction,
        #[serde(default)]
        target_ids: Vec<String>,
    },
    Recede {
        ids: Vec<String>,
        amount: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageObjectKind {
    FreeText,
    Note,
    Image,
    Equation,
    Code,
    Diagram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementIntent {
    CenterStage,
    BesideFocus,
    BelowFocus,
    Background,
    NewRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntranceIntent {
    Write,
    TossFromLeft,
    TossFromRight,
    Slide,
    Unfold,
    Fade,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub duration_ms: u32,
    pub delay_ms: u32,
    pub easing: Easing,
}

impl Transition {
    pub const IMMEDIATE: Self = Self {
        duration_ms: 0,
        delay_ms: 0,
        easing: Easing::Linear,
    };

    pub const fn smooth(duration_ms: u32) -> Self {
        Self {
            duration_ms,
            delay_ms: 0,
            easing: Easing::Emphasized,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    Standard,
    Emphasized,
    Decelerate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trips_as_json() {
        let event = EventEnvelope::new(
            7,
            42,
            WorkEvent::Intent {
                id: "intent:inspect".into(),
                label: "Inspect the startup path".into(),
                parent_id: None,
            },
        );

        let json = serde_json::to_string(&event).unwrap();
        let decoded: EventEnvelope = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, event);
        assert!(json.contains("\"type\":\"intent\""));
    }

    #[test]
    fn stage_beats_are_authored_while_presence_cues_are_ephemeral() {
        let beat = StageBeat {
            id: "beat:cache".into(),
            narration: None,
            actions: vec![StageAction::Place {
                id: "image:cache".into(),
                kind: StageObjectKind::Image,
                content: "Cache key source".into(),
                uri: Some("cache.png".into()),
                placement: PlacementIntent::BesideFocus,
                entrance: EntranceIntent::TossFromRight,
            }],
        };
        let json = serde_json::to_string(&beat).unwrap();
        assert_eq!(serde_json::from_str::<StageBeat>(&json).unwrap(), beat);

        let presence = PresenceCue {
            id: "presence:checking".into(),
            text: "I’m checking that now.".into(),
            tone: PresenceTone::Checking,
            speak: true,
            expires_after_ms: default_presence_ttl(),
        };
        assert!(
            serde_json::to_string(&presence)
                .unwrap()
                .contains("checking")
        );
    }

    #[test]
    fn viewport_slides_round_trip_with_axis_and_bento_spans() {
        let event = EventEnvelope::new(
            1,
            0,
            WorkEvent::SlidePlanned {
                slide: PresentationSlide {
                    id: "detail".into(),
                    axis: SlideAxis::Vertical,
                    title: "The detail".into(),
                    eyebrow: Some("EVIDENCE".into()),
                    composition: SlideComposition::Bento,
                    narration: NarrationBeat {
                        id: "voice-detail".into(),
                        text: "This remains one natural utterance.".into(),
                        style: Default::default(),
                        style_degree: 1.0,
                        focus: vec!["claim".into()],
                        anchors: Vec::new(),
                    },
                    blocks: vec![SlideBlock {
                        id: "claim".into(),
                        kind: SlideBlockKind::DisplayText,
                        title: None,
                        content: "One claim".into(),
                        uri: None,
                        language: None,
                        chart: None,
                        tree: None,
                        columns: 6,
                        rows: 3,
                        emphasis: BlockEmphasis::Strong,
                    }],
                    graph: None,
                    code: None,
                },
            },
        );
        let encoded = serde_json::to_string(&event).unwrap();
        let decoded: EventEnvelope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn full_slide_graphs_round_trip_without_pixel_geometry() {
        let graph = SlideGraph {
            direction: GraphDirection::LeftToRight,
            nodes: vec![
                GraphNode {
                    id: "input".into(),
                    label: "Input".into(),
                    detail: Some("Raw material".into()),
                    role: GraphNodeRole::Source,
                },
                GraphNode {
                    id: "result".into(),
                    label: "Result".into(),
                    detail: None,
                    role: GraphNodeRole::Outcome,
                },
            ],
            edges: vec![GraphEdge {
                id: "input-result".into(),
                from: "input".into(),
                to: "result".into(),
                label: Some("becomes".into()),
                relation: GraphRelation::FlowsTo,
            }],
        };
        let json = serde_json::to_string(&graph).unwrap();
        assert!(!json.contains("\"x\""));
        assert!(!json.contains("\"width\""));
        assert_eq!(serde_json::from_str::<SlideGraph>(&json).unwrap(), graph);
    }

    #[test]
    fn charts_and_code_walkthroughs_round_trip_as_distinct_surfaces() {
        let chart = SlideChart {
            kind: ChartKind::Area,
            categories: vec!["Jan".into(), "Feb".into(), "Mar".into()],
            series: vec![ChartSeries {
                label: "Latency".into(),
                values: vec![90.0, 62.0, 41.0],
            }],
            candles: Vec::new(),
        };
        let chart_json = serde_json::to_string(&chart).unwrap();
        assert_eq!(
            serde_json::from_str::<SlideChart>(&chart_json).unwrap(),
            chart
        );

        let code = SlideCode {
            language: "rust".into(),
            content: "fn main() {\n    run();\n}".into(),
            files: Vec::new(),
            show_explorer: false,
            steps: vec![CodeStep {
                id: "run".into(),
                label: Some("Execute".into()),
                file_id: None,
                start_line: 2,
                end_line: 2,
            }],
        };
        let code_json = serde_json::to_string(&code).unwrap();
        assert_eq!(serde_json::from_str::<SlideCode>(&code_json).unwrap(), code);
    }
}
