//! Deterministic semantic presentation direction and replay state.

mod artifact;

pub use artifact::{
    PRESENTATION_SCHEMA_VERSION, PresentationBundle, PresentationManifest, PresentationRecorder,
    PresentationStatus, narration_file_name, presentation_root,
};

mod compiler;

pub use compiler::{CompileReport, compile_slide, graph_traversal};

use std::collections::{BTreeMap, BTreeSet};

use foyer_shell_protocol::{
    Annotation, ArtifactKind, CameraAction, CheckpointKind, CompletionStatus, EventEnvelope,
    LayoutIntent, NarrationBeat, PlanStepStatus, PresentationObject, PresentationOp,
    PresentationPatch, PresentationPrimitive, Primitive, Relation, RelationKind, SemanticRole,
    SourceRef, StageAction, StageObjectKind, Transition, WorkEvent,
};

#[derive(Debug, Default)]
pub struct Director {
    patch_sequence: u64,
    current_intent: Option<String>,
    plan_steps: BTreeMap<String, String>,
    active_tools: BTreeMap<String, String>,
}

impl Director {
    pub fn direct(&mut self, envelope: &EventEnvelope) -> PresentationPatch {
        self.patch_sequence += 1;
        let mut beat_id = None;
        let operations = match &envelope.event {
            WorkEvent::SessionStarted { goal, .. } => vec![
                upsert(
                    "goal",
                    Primitive::Card,
                    SemanticRole::Goal,
                    "Current goal",
                    Some(goal.clone()),
                    1.0,
                    Transition::smooth(420),
                ),
                PresentationOp::SetLayout {
                    layout: LayoutIntent::Focus,
                    ids: vec!["goal".into()],
                    transition: Transition::smooth(520),
                },
                PresentationOp::SetCamera {
                    action: CameraAction::Focus,
                    target_ids: vec!["goal".into()],
                    padding: 120.0,
                    transition: Transition::smooth(650),
                },
            ],
            WorkEvent::PlanDeclared { goal: _, steps } => {
                let mut ops = Vec::new();
                let ids: Vec<_> = steps.iter().map(|step| step.id.clone()).collect();
                for (index, step) in steps.iter().enumerate() {
                    self.plan_steps.insert(step.id.clone(), step.label.clone());
                    if step.status == PlanStepStatus::Active {
                        self.current_intent = Some(step.id.clone());
                    }
                    ops.push(upsert(
                        &step.id,
                        Primitive::Card,
                        SemanticRole::Intent,
                        &step.label,
                        Some(plan_status_label(step.status).into()),
                        if step.status == PlanStepStatus::Blocked {
                            0.35
                        } else {
                            1.0
                        },
                        Transition::smooth(320),
                    ));
                    let parent = if index == 0 {
                        "goal"
                    } else {
                        &steps[index - 1].id
                    };
                    ops.push(connect(
                        format!("relation:{parent}:{}", step.id),
                        parent,
                        &step.id,
                        "then",
                        RelationKind::Sequence,
                    ));
                }
                ops.push(PresentationOp::SetLayout {
                    layout: LayoutIntent::Flow,
                    ids,
                    transition: Transition::smooth(480),
                });
                if let Some(active) = steps
                    .iter()
                    .find(|step| step.status == PlanStepStatus::Active)
                {
                    ops.push(PresentationOp::SetFocus {
                        ids: vec![active.id.clone()],
                        transition: Transition::smooth(380),
                    });
                }
                ops
            }
            WorkEvent::Checkpoint {
                id,
                step_id,
                kind,
                title,
                body,
                status,
                related_to,
                source,
            } => {
                let (primitive, role, confidence) = match kind {
                    CheckpointKind::Progress => (Primitive::Progress, SemanticRole::Activity, 1.0),
                    CheckpointKind::Observation => {
                        (Primitive::Card, SemanticRole::Observation, 1.0)
                    }
                    CheckpointKind::Evidence => {
                        (Primitive::SourceCard, SemanticRole::Evidence, 1.0)
                    }
                    CheckpointKind::Decision => (Primitive::Callout, SemanticRole::Decision, 1.0),
                    CheckpointKind::Uncertainty => {
                        (Primitive::Callout, SemanticRole::Uncertainty, 0.5)
                    }
                };
                let mut ops = vec![attach_source(
                    upsert(
                        id,
                        primitive,
                        role,
                        title,
                        Some(body.clone()),
                        confidence,
                        Transition::smooth(360),
                    ),
                    source.clone(),
                )];
                if let Some(step_id) = step_id {
                    self.current_intent = Some(step_id.clone());
                    ops.push(connect(
                        format!("relation:{step_id}:{id}"),
                        step_id,
                        id,
                        "produces",
                        RelationKind::Related,
                    ));
                    if let Some(status) = status {
                        let label = self
                            .plan_steps
                            .get(step_id)
                            .cloned()
                            .unwrap_or_else(|| step_id.clone());
                        ops.push(upsert(
                            step_id,
                            Primitive::Card,
                            SemanticRole::Intent,
                            &label,
                            Some(plan_status_label(*status).into()),
                            if *status == PlanStepStatus::Blocked {
                                0.35
                            } else {
                                1.0
                            },
                            Transition::smooth(220),
                        ));
                    }
                }
                ops.extend(related_to.iter().map(|target| {
                    connect(
                        format!("relation:{id}:{target}"),
                        id,
                        target,
                        "relates to",
                        RelationKind::Related,
                    )
                }));
                ops
            }
            WorkEvent::ArtifactProduced {
                id,
                title,
                kind,
                uri,
                preview,
                media_type: _,
                related_to,
            } => {
                let primitive = match kind {
                    ArtifactKind::Text | ArtifactKind::Markdown | ArtifactKind::File => {
                        Primitive::Card
                    }
                    ArtifactKind::Code => Primitive::CodeBlock,
                    ArtifactKind::Image => Primitive::Image,
                    ArtifactKind::Table => Primitive::Table,
                    ArtifactKind::Chart => Primitive::Chart,
                    ArtifactKind::Equation => Primitive::Equation,
                };
                let source = uri.as_ref().map(|uri| foyer_shell_protocol::SourceRef {
                    uri: uri.clone(),
                    title: Some(title.clone()),
                    line: None,
                    media_type: None,
                });
                let mut ops = vec![attach_source(
                    upsert(
                        id,
                        primitive,
                        SemanticRole::Result,
                        title,
                        preview.clone(),
                        1.0,
                        Transition::smooth(420),
                    ),
                    source,
                )];
                ops.extend(related_to.iter().map(|target| {
                    connect(
                        format!("relation:{target}:{id}"),
                        target,
                        id,
                        "produces",
                        RelationKind::Causes,
                    )
                }));
                ops
            }
            WorkEvent::PresentationObject {
                id,
                primitive,
                role,
                title,
                body,
                uri,
                latex,
                related_to,
            } => {
                let primitive = match primitive {
                    PresentationPrimitive::Text => Primitive::Text,
                    PresentationPrimitive::Card => Primitive::Card,
                    PresentationPrimitive::Image => Primitive::Image,
                    PresentationPrimitive::Equation => Primitive::Equation,
                    PresentationPrimitive::Code => Primitive::CodeBlock,
                    PresentationPrimitive::Table => Primitive::Table,
                    PresentationPrimitive::Callout => Primitive::Callout,
                };
                let mut operation = upsert(
                    id,
                    primitive,
                    *role,
                    title,
                    body.clone(),
                    1.0,
                    Transition::smooth(420),
                );
                if let PresentationOp::Upsert { object, .. } = &mut operation {
                    if let Some(latex) = latex {
                        object
                            .metadata
                            .insert("latex".into(), serde_json::Value::String(latex.clone()));
                    }
                    if let Some(uri) = uri {
                        object.source = Some(foyer_shell_protocol::SourceRef {
                            uri: uri.clone(),
                            title: Some(title.clone()),
                            line: None,
                            media_type: None,
                        });
                    }
                }
                let mut ops = vec![operation];
                ops.extend(related_to.iter().map(|target| {
                    connect(
                        format!("relation:{target}:{id}"),
                        target,
                        id,
                        "explains",
                        RelationKind::Related,
                    )
                }));
                ops
            }
            WorkEvent::AnnotationAdded {
                id,
                target_id,
                kind,
                label,
            } => vec![PresentationOp::Annotate {
                annotation: Annotation {
                    id: id.clone(),
                    target_id: target_id.clone(),
                    kind: *kind,
                    label: label.clone(),
                },
                transition: Transition::smooth(380),
            }],
            WorkEvent::CameraDirected {
                action,
                target_ids,
                padding,
            } => {
                let mut ops = Vec::new();
                if *action == CameraAction::Focus && !target_ids.is_empty() {
                    ops.push(PresentationOp::SetFocus {
                        ids: target_ids.clone(),
                        transition: Transition::smooth(320),
                    });
                }
                ops.push(PresentationOp::SetCamera {
                    action: *action,
                    target_ids: target_ids.clone(),
                    padding: *padding,
                    transition: Transition::smooth(680),
                });
                ops
            }
            WorkEvent::Intent {
                id,
                label,
                parent_id,
            } => {
                self.current_intent = Some(id.clone());
                let mut ops = vec![upsert(
                    id,
                    Primitive::Card,
                    SemanticRole::Intent,
                    label,
                    None,
                    1.0,
                    Transition::smooth(320),
                )];
                let parent = parent_id.as_deref().unwrap_or("goal");
                ops.push(connect(
                    format!("relation:{parent}:{id}"),
                    parent,
                    id,
                    "leads to",
                    RelationKind::Sequence,
                ));
                ops.push(PresentationOp::SetFocus {
                    ids: vec![id.clone()],
                    transition: Transition::smooth(380),
                });
                ops
            }
            WorkEvent::ToolStarted {
                call_id,
                tool,
                public_summary,
            } => {
                let id = format!("tool:{call_id}");
                self.active_tools.insert(call_id.clone(), id.clone());
                let mut ops = vec![upsert(
                    &id,
                    Primitive::Progress,
                    SemanticRole::Activity,
                    public_summary.as_deref().unwrap_or(tool),
                    Some(format!("Using {tool}")),
                    1.0,
                    Transition::smooth(180),
                )];
                if let Some(intent) = &self.current_intent {
                    ops.push(connect(
                        format!("relation:{intent}:{id}"),
                        intent,
                        &id,
                        "investigates",
                        RelationKind::Related,
                    ));
                }
                ops
            }
            WorkEvent::ToolCompleted {
                call_id,
                tool,
                success,
                duration_ms,
                public_summary,
            } => {
                let id = self
                    .active_tools
                    .remove(call_id)
                    .unwrap_or_else(|| format!("tool:{call_id}"));
                vec![upsert(
                    &id,
                    Primitive::Badge,
                    SemanticRole::Activity,
                    public_summary.as_deref().unwrap_or(tool),
                    Some(format!(
                        "{} in {duration_ms} ms",
                        if *success { "Completed" } else { "Failed" }
                    )),
                    if *success { 1.0 } else { 0.0 },
                    Transition::smooth(220),
                )]
            }
            WorkEvent::Observation {
                id,
                label,
                body,
                source,
                relates_to,
            } => {
                let mut ops = vec![attach_source(
                    upsert(
                        id,
                        Primitive::Card,
                        SemanticRole::Observation,
                        label,
                        Some(body.clone()),
                        1.0,
                        Transition::smooth(340),
                    ),
                    source.clone(),
                )];
                ops.extend(relates_to.iter().map(|target| {
                    connect(
                        format!("relation:{id}:{target}"),
                        id,
                        target,
                        "relates to",
                        RelationKind::Related,
                    )
                }));
                ops
            }
            WorkEvent::Evidence {
                id,
                label,
                excerpt,
                source,
                supports,
                confidence,
            } => {
                let mut ops = vec![attach_source(
                    upsert(
                        id,
                        Primitive::SourceCard,
                        SemanticRole::Evidence,
                        label,
                        Some(excerpt.clone()),
                        *confidence,
                        Transition::smooth(360),
                    ),
                    Some(source.clone()),
                )];
                ops.extend(supports.iter().map(|target| {
                    connect(
                        format!("relation:{id}:{target}"),
                        id,
                        target,
                        "supports",
                        RelationKind::Supports,
                    )
                }));
                ops.push(PresentationOp::SetLayout {
                    layout: LayoutIntent::EvidenceStack,
                    ids: supports
                        .iter()
                        .cloned()
                        .chain(std::iter::once(id.clone()))
                        .collect(),
                    transition: Transition::smooth(480),
                });
                ops
            }
            WorkEvent::Decision {
                id,
                label,
                presentation,
                based_on,
                confidence,
            } => {
                let mut ops = vec![upsert(
                    id,
                    Primitive::Callout,
                    SemanticRole::Decision,
                    label,
                    Some(presentation.clone()),
                    *confidence,
                    Transition::smooth(440),
                )];
                ops.extend(based_on.iter().map(|source| {
                    connect(
                        format!("relation:{source}:{id}"),
                        source,
                        id,
                        "supports",
                        RelationKind::Supports,
                    )
                }));
                ops.push(PresentationOp::SetFocus {
                    ids: based_on
                        .iter()
                        .cloned()
                        .chain(std::iter::once(id.clone()))
                        .collect(),
                    transition: Transition::smooth(520),
                });
                ops
            }
            WorkEvent::Uncertainty {
                id,
                question,
                related_to,
            } => {
                let mut ops = vec![upsert(
                    id,
                    Primitive::Callout,
                    SemanticRole::Uncertainty,
                    "Open question",
                    Some(question.clone()),
                    0.5,
                    Transition::smooth(300),
                )];
                ops.extend(related_to.iter().map(|target| {
                    connect(
                        format!("relation:{id}:{target}"),
                        id,
                        target,
                        "questions",
                        RelationKind::Contradicts,
                    )
                }));
                ops
            }
            WorkEvent::NarrationProposed {
                beat_id: id,
                text,
                focus,
                anchors,
            } => {
                beat_id = Some(id.clone());
                vec![PresentationOp::QueueNarration {
                    beat: NarrationBeat {
                        id: id.clone(),
                        text: text.clone(),
                        style: Default::default(),
                        style_degree: 1.0,
                        focus: focus.clone(),
                        anchors: anchors.clone(),
                    },
                }]
            }
            WorkEvent::StageBeatPlanned { beat } => {
                let mut operations = Vec::new();
                let mut placed_ids = Vec::new();
                for (index, action) in beat.actions.iter().enumerate() {
                    let delay_ms = (index.min(3) as u32) * 90;
                    match action {
                        StageAction::Place {
                            id,
                            kind,
                            content,
                            uri,
                            entrance: _,
                            placement: _,
                        } => {
                            placed_ids.push(id.clone());
                            operations.push(PresentationOp::Upsert {
                                object: stage_object(id, *kind, content, uri.as_deref()),
                                transition: Transition {
                                    delay_ms,
                                    ..Transition::smooth(460)
                                },
                            });
                        }
                        StageAction::Annotate {
                            id,
                            target_id,
                            kind,
                            label,
                        } => operations.push(PresentationOp::Annotate {
                            annotation: Annotation {
                                id: id.clone(),
                                target_id: target_id.clone(),
                                kind: *kind,
                                label: label.clone(),
                            },
                            transition: Transition {
                                delay_ms,
                                ..Transition::smooth(360)
                            },
                        }),
                        StageAction::Arrange { ids, layout } => {
                            operations.push(PresentationOp::SetLayout {
                                layout: *layout,
                                ids: ids.clone(),
                                transition: Transition::smooth(520),
                            });
                        }
                        StageAction::Camera { action, target_ids } => {
                            operations.push(PresentationOp::SetCamera {
                                action: *action,
                                target_ids: target_ids.clone(),
                                padding: 112.0,
                                transition: Transition::smooth(620),
                            });
                        }
                        StageAction::Recede { ids, amount } => {
                            operations.push(PresentationOp::Recede {
                                ids: ids.clone(),
                                amount: *amount,
                                transition: Transition::smooth(420),
                            });
                        }
                    }
                }
                if let Some(narration) = &beat.narration {
                    beat_id = Some(narration.id.clone());
                    if !narration.focus.is_empty() {
                        operations.push(PresentationOp::SetFocus {
                            ids: narration.focus.clone(),
                            transition: Transition::smooth(360),
                        });
                    } else if !placed_ids.is_empty() {
                        operations.push(PresentationOp::SetFocus {
                            ids: placed_ids,
                            transition: Transition::smooth(360),
                        });
                    }
                    operations.push(PresentationOp::QueueNarration {
                        beat: narration.clone(),
                    });
                }
                operations
            }
            WorkEvent::PresenceProposed { cue } => {
                beat_id = Some(cue.id.clone());
                cue.speak
                    .then(|| PresentationOp::QueueNarration {
                        beat: NarrationBeat {
                            id: cue.id.clone(),
                            text: cue.text.clone(),
                            style: Default::default(),
                            style_degree: 1.0,
                            focus: Vec::new(),
                            anchors: Vec::new(),
                        },
                    })
                    .into_iter()
                    .collect()
            }
            // The primary GPUI deck consumes viewport slides directly. The retained free-form
            // presentation director intentionally ignores them.
            WorkEvent::SlidePlanned { .. } => Vec::new(),
            WorkEvent::SessionCompleted {
                status,
                summary,
                answer_markdown: _,
                artifact_ids: _,
            } => vec![
                upsert(
                    "result",
                    Primitive::Callout,
                    SemanticRole::Result,
                    completion_label(*status),
                    Some(summary.clone()),
                    if *status == CompletionStatus::Failed {
                        0.0
                    } else {
                        1.0
                    },
                    Transition::smooth(480),
                ),
                PresentationOp::SetFocus {
                    ids: vec!["result".into()],
                    transition: Transition::smooth(520),
                },
                PresentationOp::SetCamera {
                    action: CameraAction::Focus,
                    target_ids: vec!["result".into()],
                    padding: 140.0,
                    transition: Transition::smooth(700),
                },
            ],
        };

        PresentationPatch {
            sequence: self.patch_sequence,
            source_event_sequence: envelope.sequence,
            beat_id,
            operations,
        }
    }
}

fn plan_status_label(status: PlanStepStatus) -> &'static str {
    match status {
        PlanStepStatus::Pending => "Pending",
        PlanStepStatus::Active => "In progress",
        PlanStepStatus::Completed => "Completed",
        PlanStepStatus::Blocked => "Blocked",
    }
}

fn completion_label(status: CompletionStatus) -> &'static str {
    match status {
        CompletionStatus::Completed => "Result",
        CompletionStatus::Partial => "Partial result",
        CompletionStatus::Failed => "Task failed",
        CompletionStatus::Cancelled => "Task cancelled",
    }
}

fn stage_object(
    id: &str,
    kind: StageObjectKind,
    content: &str,
    uri: Option<&str>,
) -> PresentationObject {
    let mut metadata = BTreeMap::new();
    let (primitive, role, title, body) = match kind {
        StageObjectKind::FreeText => (
            Primitive::Text,
            SemanticRole::Observation,
            content.to_string(),
            None,
        ),
        StageObjectKind::Note => {
            let title = concise_title(content, 54);
            let body = (title != content).then(|| content.to_string());
            (Primitive::Card, SemanticRole::Observation, title, body)
        }
        StageObjectKind::Image => (
            Primitive::Image,
            SemanticRole::Result,
            concise_title(content, 54),
            None,
        ),
        StageObjectKind::Equation => {
            metadata.insert("latex".into(), serde_json::Value::String(content.into()));
            (
                Primitive::Equation,
                SemanticRole::Result,
                "Equation".into(),
                Some(content.into()),
            )
        }
        StageObjectKind::Code => (
            Primitive::CodeBlock,
            SemanticRole::Result,
            "Code".into(),
            Some(content.into()),
        ),
        StageObjectKind::Diagram => (
            Primitive::Chart,
            SemanticRole::Observation,
            "Diagram".into(),
            Some(content.into()),
        ),
    };
    PresentationObject {
        id: id.into(),
        primitive,
        role,
        title,
        body,
        source: uri.map(|uri| SourceRef {
            uri: uri.into(),
            title: Some(content.into()),
            line: None,
            media_type: None,
        }),
        confidence: 1.0,
        metadata,
    }
}

fn concise_title(content: &str, maximum: usize) -> String {
    let mut characters = content.chars();
    let title: String = characters.by_ref().take(maximum).collect();
    if characters.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

fn upsert(
    id: &str,
    primitive: Primitive,
    role: SemanticRole,
    title: &str,
    body: Option<String>,
    confidence: f32,
    transition: Transition,
) -> PresentationOp {
    PresentationOp::Upsert {
        object: PresentationObject {
            id: id.into(),
            primitive,
            role,
            title: title.into(),
            body,
            source: None,
            confidence,
            metadata: BTreeMap::new(),
        },
        transition,
    }
}

fn attach_source(
    mut operation: PresentationOp,
    source: Option<foyer_shell_protocol::SourceRef>,
) -> PresentationOp {
    if let PresentationOp::Upsert { object, .. } = &mut operation {
        object.source = source;
    }
    operation
}

fn connect(id: String, from: &str, to: &str, label: &str, kind: RelationKind) -> PresentationOp {
    PresentationOp::Connect {
        relation: Relation {
            id,
            from: from.into(),
            to: to.into(),
            label: label.into(),
            kind,
        },
        transition: Transition::smooth(360),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentationState {
    pub objects: BTreeMap<String, PresentationObject>,
    pub relations: BTreeMap<String, Relation>,
    pub focused: BTreeSet<String>,
    pub receded: BTreeMap<String, f32>,
    pub narration_queue: Vec<NarrationBeat>,
    pub annotations: BTreeMap<String, Annotation>,
    pub camera_action: CameraAction,
    pub camera_targets: Vec<String>,
    pub camera_padding: f32,
    pub last_patch_sequence: u64,
}

impl Default for PresentationState {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            relations: BTreeMap::new(),
            focused: BTreeSet::new(),
            receded: BTreeMap::new(),
            narration_queue: Vec::new(),
            annotations: BTreeMap::new(),
            camera_action: CameraAction::ShowAll,
            camera_targets: Vec::new(),
            camera_padding: 112.0,
            last_patch_sequence: 0,
        }
    }
}

impl PresentationState {
    pub fn apply(&mut self, patch: &PresentationPatch) {
        for operation in &patch.operations {
            match operation {
                PresentationOp::Upsert { object, .. } => {
                    self.objects.insert(object.id.clone(), object.clone());
                }
                PresentationOp::Connect { relation, .. } => {
                    self.relations.insert(relation.id.clone(), relation.clone());
                }
                PresentationOp::Remove { id, .. } => {
                    self.objects.remove(id);
                    self.relations
                        .retain(|_, relation| relation.from != *id && relation.to != *id);
                    self.focused.remove(id);
                    self.receded.remove(id);
                    self.annotations
                        .retain(|_, annotation| annotation.target_id != *id);
                }
                PresentationOp::SetFocus { ids, .. } => {
                    self.focused = ids.iter().cloned().collect();
                }
                PresentationOp::Recede { ids, amount, .. } => {
                    for id in ids {
                        self.receded.insert(id.clone(), *amount);
                    }
                }
                PresentationOp::QueueNarration { beat } => self.narration_queue.push(beat.clone()),
                PresentationOp::Annotate { annotation, .. } => {
                    self.annotations
                        .insert(annotation.id.clone(), annotation.clone());
                }
                PresentationOp::SetCamera {
                    action,
                    target_ids,
                    padding,
                    ..
                } => {
                    self.camera_action = *action;
                    self.camera_targets = target_ids.clone();
                    self.camera_padding = *padding;
                }
                PresentationOp::SetLayout { .. } => {}
            }
        }
        self.last_patch_sequence = patch.sequence;
    }
}

#[cfg(test)]
mod tests {
    use foyer_shell_protocol::{
        AnnotationKind, CameraAction, CheckpointKind, EntranceIntent, EventEnvelope,
        PlacementIntent, PlanStep, PlanStepStatus, PresentationPrimitive, SemanticRole,
        StageAction, StageBeat, StageObjectKind, WorkEvent,
    };

    use super::*;

    #[test]
    fn replay_is_deterministic() {
        let events = [
            EventEnvelope::new(
                1,
                0,
                WorkEvent::SessionStarted {
                    session_id: "demo".into(),
                    goal: "Explain startup".into(),
                },
            ),
            EventEnvelope::new(
                2,
                10,
                WorkEvent::Intent {
                    id: "intent:startup".into(),
                    label: "Trace startup".into(),
                    parent_id: None,
                },
            ),
        ];

        let replay = || {
            let mut director = Director::default();
            let mut state = PresentationState::default();
            for event in &events {
                state.apply(&director.direct(event));
            }
            state
        };

        assert_eq!(replay(), replay());
    }

    #[test]
    fn authored_stage_beats_materialize_their_own_visuals() {
        let event = EventEnvelope::new(
            1,
            0,
            WorkEvent::StageBeatPlanned {
                beat: StageBeat {
                    id: "stage:cache".into(),
                    narration: Some(NarrationBeat {
                        id: "beat:cache".into(),
                        text: "The cache key forces a rebuild every launch.".into(),
                        style: Default::default(),
                        style_degree: 1.0,
                        focus: vec!["note:cache".into()],
                        anchors: Vec::new(),
                    }),
                    actions: vec![StageAction::Place {
                        id: "note:cache".into(),
                        kind: StageObjectKind::Note,
                        content: "The process-specific cache key never survives restart.".into(),
                        uri: None,
                        placement: PlacementIntent::CenterStage,
                        entrance: EntranceIntent::Slide,
                    }],
                },
            },
        );
        let mut director = Director::default();
        let mut state = PresentationState::default();
        state.apply(&director.direct(&event));

        assert_eq!(state.objects["note:cache"].primitive, Primitive::Card);
        assert_eq!(state.focused, BTreeSet::from(["note:cache".into()]));
        assert_eq!(state.narration_queue[0].id, "beat:cache");
    }

    #[test]
    fn removing_an_object_removes_its_relations() {
        let mut state = PresentationState::default();
        let mut director = Director::default();
        state.apply(&director.direct(&EventEnvelope::new(
            1,
            0,
            WorkEvent::SessionStarted {
                session_id: "demo".into(),
                goal: "Explain startup".into(),
            },
        )));
        state.apply(&director.direct(&EventEnvelope::new(
            2,
            0,
            WorkEvent::Intent {
                id: "intent:startup".into(),
                label: "Trace startup".into(),
                parent_id: None,
            },
        )));

        let removal = PresentationPatch {
            sequence: 3,
            source_event_sequence: 3,
            beat_id: None,
            operations: vec![PresentationOp::Remove {
                id: "intent:startup".into(),
                transition: Transition::IMMEDIATE,
            }],
        };
        state.apply(&removal);

        assert!(state.relations.is_empty());
    }

    #[test]
    fn checkpoints_update_a_plan_step_without_losing_its_label() {
        let mut state = PresentationState::default();
        let mut director = Director::default();
        state.apply(&director.direct(&EventEnvelope::new(
            1,
            0,
            WorkEvent::PlanDeclared {
                goal: "Verify the sidecar".into(),
                steps: vec![PlanStep {
                    id: "verify".into(),
                    label: "Verify SDK startup".into(),
                    status: PlanStepStatus::Active,
                }],
            },
        )));
        state.apply(&director.direct(&EventEnvelope::new(
            2,
            10,
            WorkEvent::Checkpoint {
                id: "checkpoint:ready".into(),
                step_id: Some("verify".into()),
                kind: CheckpointKind::Progress,
                title: "Sidecar ready".into(),
                body: "Both sessions initialized.".into(),
                status: Some(PlanStepStatus::Completed),
                related_to: Vec::new(),
                source: None,
            },
        )));

        let step = &state.objects["verify"];
        assert_eq!(step.title, "Verify SDK startup");
        assert_eq!(step.body.as_deref(), Some("Completed"));
    }

    #[test]
    fn presentation_actions_remain_semantic_until_the_renderer() {
        let mut state = PresentationState::default();
        let mut director = Director::default();
        for event in [
            WorkEvent::PresentationObject {
                id: "equation:latency".into(),
                primitive: PresentationPrimitive::Equation,
                role: SemanticRole::Result,
                title: "Latency".into(),
                body: None,
                uri: None,
                latex: Some("L = L_m + L_t".into()),
                related_to: Vec::new(),
            },
            WorkEvent::AnnotationAdded {
                id: "annotation:latency".into(),
                target_id: "equation:latency".into(),
                kind: AnnotationKind::Circle,
                label: Some("critical path".into()),
            },
            WorkEvent::CameraDirected {
                action: CameraAction::Focus,
                target_ids: vec!["equation:latency".into()],
                padding: 96.0,
            },
        ] {
            state.apply(&director.direct(&EventEnvelope::new(1, 0, event)));
        }

        let equation = &state.objects["equation:latency"];
        assert_eq!(equation.primitive, Primitive::Equation);
        assert_eq!(equation.metadata["latex"], "L = L_m + L_t");
        assert_eq!(
            state.annotations["annotation:latency"].target_id,
            "equation:latency"
        );
        assert_eq!(state.camera_action, CameraAction::Focus);
        assert_eq!(state.camera_targets, ["equation:latency"]);
    }
}
