use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    env, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

mod code;
mod http;
mod layout;
mod math;
mod theme;

use async_channel::{Receiver, Sender};
use foyer_shell_audio::{
    AudioCommand, AudioConfig, AudioEvent, NarrationRuntime, PlaybackRuntime, RecordedNarration,
};
use foyer_shell_pi_bridge::{PiConfig, PiHarness, SidecarMessage};
use foyer_shell_presentation::{PresentationBundle, PresentationRecorder};
use foyer_shell_presentation_ui::{
    CodeSurface, CodeSurfaceState, InputEvent, Prompt, PromptState, Root, TreeSurface,
    TreeSurfaceState,
};
use foyer_shell_protocol::{
    BlockEmphasis, CompletionStatus, CueAction, EventEnvelope, GraphDirection, GraphNodeRole,
    NarrationBeat, NarrationStyle, PresentationSlide, SlideAxis, SlideBlock, SlideBlockKind,
    SlideCode, SlideComposition, SlideGraph, WorkEvent,
};
#[cfg(test)]
use foyer_shell_protocol::{ChartKind, SlideTreeNode};
use gpui::{
    App, Bounds, Context, CursorStyle, Div, Entity, FontWeight, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, PathBuilder, Render, ScrollHandle,
    Subscription, Window, WindowBounds, WindowOptions, canvas, div, img, point, prelude::*, px,
    relative, rgb, size,
};

use code::highlighted_code;
use http::FoyerShellHttpClient;
use layout::*;
use math::math_expression;
use theme::*;

const SLIDE_TRANSITION_MS: u64 = 620;

fn image_status(label: &'static str, failed: bool) -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(rgb(SURFACE_RECESSED))
        .text_sm()
        .text_color(if failed { rgb(MUTED) } else { rgb(FOREGROUND) })
        .child(
            div()
                .size_2()
                .rounded_full()
                .bg(if failed { rgb(SUBTLE) } else { rgb(FOCUS) }),
        )
        .child(label)
}

#[derive(Clone, Debug)]
struct GraphLayout {
    nodes: BTreeMap<String, VisualRect>,
    node_order: BTreeMap<String, usize>,
    edges: Vec<GraphEdgePath>,
    canvas_width: f32,
    canvas_height: f32,
}

#[derive(Clone, Debug)]
struct GraphEdgePath {
    id: String,
    from: String,
    to: String,
    label: Option<String>,
    start: (f32, f32),
    control_a: (f32, f32),
    control_b: (f32, f32),
    end: (f32, f32),
    label_bounds: VisualRect,
}

#[derive(Clone)]
struct DeckSlide {
    spec: PresentationSlide,
    x: i32,
    y: i32,
    ordinal: usize,
    revealed: bool,
}

#[derive(Clone)]
struct SlideTransition {
    from_id: Option<String>,
    to_id: String,
    direction_x: f32,
    direction_y: f32,
    started: Instant,
}

#[derive(Clone, Debug, Default)]
struct GraphViewportState {
    pan_x: f32,
    pan_y: f32,
    dragging_from: Option<(f32, f32)>,
}

struct GraphCameraTransition {
    slide_id: String,
    from_node: Option<String>,
    to_node: String,
    started: Instant,
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Exit,
}

#[derive(Clone, Copy)]
enum DeferredStart {
    Live,
    Replay,
}

pub struct PresentationView {
    slides: Vec<DeckSlide>,
    current_id: Option<String>,
    transition: Option<SlideTransition>,
    focused_blocks: BTreeSet<String>,
    graph_trace_started: Option<Instant>,
    graph_viewports: BTreeMap<String, GraphViewportState>,
    graph_camera: Option<GraphCameraTransition>,
    block_scrolls: BTreeMap<String, ScrollHandle>,
    stream_open: bool,
    narration_requests: Option<Sender<NarrationBeat>>,
    playback_controls: Option<Sender<AudioCommand>>,
    playback_paused: bool,
    playback_generation: u64,
    pending_slides: BTreeMap<String, String>,
    audio_status: String,
    activity_status: Option<String>,
    audio_progress: Option<(u64, u64)>,
    prompt_input: Entity<PromptState>,
    prompt_should_clear: bool,
    code_surfaces: BTreeMap<String, CodeSurfaceState>,
    tree_surfaces: BTreeMap<String, TreeSurfaceState>,
    replay_bundle: Option<PresentationBundle>,
    recorder: Option<Arc<Mutex<PresentationRecorder>>>,
    replay_mode: bool,
    can_exit: bool,
    deferred_start: Option<DeferredStart>,
    _subscriptions: Vec<Subscription>,
}

impl gpui::EventEmitter<PlayerEvent> for PresentationView {}

impl PresentationView {
    pub fn new(live_prompt: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(live_prompt, true, window, cx)
    }

    fn build(
        live_prompt: Option<String>,
        start_immediately: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let welcome = welcome_slide();
        let block_scrolls = welcome
            .spec
            .blocks
            .iter()
            .map(|block| (block.id.clone(), ScrollHandle::new()))
            .collect();
        let prompt_input = cx.new(|cx| {
            PromptState::new(window, cx)
                .placeholder("Ask Foyer Shell to explain something…")
                .default_value(live_prompt.clone().unwrap_or_default())
        });
        let submit_subscription = cx.subscribe(&prompt_input, |view, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                view.submit_prompt(cx);
            }
        });
        let mut view = Self {
            slides: vec![welcome],
            current_id: Some("welcome".into()),
            transition: None,
            focused_blocks: BTreeSet::new(),
            graph_trace_started: None,
            graph_viewports: BTreeMap::new(),
            graph_camera: None,
            block_scrolls,
            stream_open: false,
            narration_requests: None,
            playback_controls: None,
            playback_paused: false,
            playback_generation: 0,
            pending_slides: BTreeMap::new(),
            audio_status: "Voice · OpenRouter".into(),
            activity_status: None,
            audio_progress: None,
            prompt_input,
            prompt_should_clear: false,
            code_surfaces: BTreeMap::new(),
            tree_surfaces: BTreeMap::new(),
            replay_bundle: None,
            recorder: None,
            replay_mode: false,
            can_exit: false,
            deferred_start: None,
            _subscriptions: vec![submit_subscription],
        };
        if live_prompt.is_some() && start_immediately {
            view.submit_prompt(cx);
        }
        view
    }

    pub fn live_embedded(prompt: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::build(Some(prompt), false, window, cx);
        view.slides.clear();
        view.block_scrolls.clear();
        view.current_id = None;
        view.activity_status = Some("Opening presentation…".into());
        view.can_exit = true;
        view.deferred_start = Some(DeferredStart::Live);
        view
    }

    pub fn replay(bundle: PresentationBundle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prompt_input = cx.new(|cx| PromptState::new(window, cx));
        let mut x = 0;
        let mut y = 0;
        let slides = bundle
            .slides
            .iter()
            .cloned()
            .enumerate()
            .map(|(ordinal, spec)| {
                if ordinal > 0 {
                    match spec.axis {
                        SlideAxis::Vertical => y += 1,
                        SlideAxis::Root | SlideAxis::Horizontal => x += 1,
                    }
                }
                DeckSlide {
                    spec,
                    x,
                    y,
                    ordinal,
                    revealed: true,
                }
            })
            .collect::<Vec<_>>();
        let current_id = slides.first().map(|slide| slide.spec.id.clone());
        let block_scrolls = slides
            .iter()
            .flat_map(|slide| slide.spec.blocks.iter())
            .map(|block| (block.id.clone(), ScrollHandle::new()))
            .collect();
        let mut view = Self {
            slides,
            current_id,
            transition: None,
            focused_blocks: BTreeSet::new(),
            graph_trace_started: None,
            graph_viewports: BTreeMap::new(),
            graph_camera: None,
            block_scrolls,
            stream_open: false,
            narration_requests: None,
            playback_controls: None,
            playback_paused: false,
            playback_generation: 0,
            pending_slides: BTreeMap::new(),
            audio_status: "Recorded narration · ready".into(),
            activity_status: None,
            audio_progress: None,
            prompt_input,
            prompt_should_clear: false,
            code_surfaces: BTreeMap::new(),
            tree_surfaces: BTreeMap::new(),
            replay_bundle: Some(bundle),
            recorder: None,
            replay_mode: true,
            can_exit: true,
            deferred_start: Some(DeferredStart::Replay),
            _subscriptions: Vec::new(),
        };
        view.playback_paused = true;
        view
    }

    pub fn resume_playback(&mut self, cx: &mut Context<Self>) {
        match self.deferred_start.take() {
            Some(DeferredStart::Live) => {
                self.submit_prompt(cx);
                return;
            }
            Some(DeferredStart::Replay) => {
                self.start_recorded_from(0, cx);
                cx.notify();
                return;
            }
            None => {}
        }
        if self.playback_paused
            && let Some(controls) = self.playback_controls.as_ref()
            && controls.try_send(AudioCommand::Resume).is_ok()
        {
            self.playback_paused = false;
            cx.notify();
        }
    }

    fn monitor_live_events(receiver: Receiver<Vec<EventEnvelope>>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Ok(events) = receiver.recv().await {
                if this
                    .update(cx, |view, cx| {
                        view.accept_live_batch(events);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            this.update(cx, |view, cx| {
                view.stream_open = false;
                view.narration_requests.take();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn monitor_audio_events(
        receiver: Receiver<AudioEvent>,
        recorder: Option<Arc<Mutex<PresentationRecorder>>>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv().await {
                if this
                    .update(cx, |view, cx| {
                        if view.playback_generation != generation {
                            return;
                        }
                        view.handle_audio_event(event);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            if let Some(recorder) = recorder {
                let path = recorder
                    .lock()
                    .ok()
                    .map(|recorder| recorder.path().to_path_buf());
                let (sender, finished) = async_channel::bounded(1);
                std::thread::spawn(move || {
                    let result = recorder
                        .lock()
                        .map_err(|_| "presentation recorder lock was poisoned".to_string())
                        .and_then(|mut recorder| {
                            recorder.finish_audio().map_err(|error| error.to_string())
                        })
                        .and_then(|_| {
                            path.ok_or_else(|| {
                                "presentation bundle path is unavailable".to_string()
                            })
                        })
                        .and_then(|path| {
                            PresentationBundle::open(path).map_err(|error| error.to_string())
                        });
                    let _ = sender.send_blocking(result);
                });
                if let Ok(result) = finished.recv().await {
                    this.update(cx, |view, cx| {
                        if let Ok(bundle) = result {
                            view.replay_bundle = Some(bundle);
                            view.audio_status = "Presentation saved · replay ready".into();
                        }
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn run_busy(&self) -> bool {
        self.stream_open
            || self.narration_requests.is_some()
            || !self.pending_slides.is_empty()
            || self
                .audio_progress
                .is_some_and(|(position, duration)| position < duration)
    }

    fn submit_prompt(&mut self, cx: &mut Context<Self>) {
        let prompt = self.prompt_input.read(cx).value().trim().to_string();
        if prompt.is_empty() || self.run_busy() {
            return;
        }
        self.deferred_start = None;
        self.prompt_should_clear = true;
        self.slides.clear();
        self.block_scrolls.clear();
        self.current_id = None;
        self.transition = None;
        self.focused_blocks.clear();
        self.graph_trace_started = None;
        self.graph_viewports.clear();
        self.graph_camera = None;
        self.code_surfaces.clear();
        self.tree_surfaces.clear();
        self.pending_slides.clear();
        self.stream_open = true;
        self.replay_mode = false;
        self.audio_progress = None;
        self.audio_status = "Voice · connecting".into();
        self.activity_status = Some("Starting the investigation.".into());

        let recorder = PresentationRecorder::begin(&prompt)
            .map(|recorder| Arc::new(Mutex::new(recorder)))
            .map_err(|error| eprintln!("failed to create presentation bundle: {error}"))
            .ok();
        let request_id = recorder
            .as_ref()
            .and_then(|recorder| recorder.lock().ok())
            .map(|recorder| recorder.presentation_id().to_string())
            .unwrap_or_else(|| {
                format!(
                    "presentation-live-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                )
            });
        let root = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut audio_config = AudioConfig::for_workspace(&root);
        if let Some(path) = recorder
            .as_ref()
            .and_then(|recorder| recorder.lock().ok())
            .map(|recorder| recorder.narration_dir().to_path_buf())
        {
            audio_config = audio_config.with_recording_dir(path);
        }
        let runtime = NarrationRuntime::spawn(audio_config);
        self.narration_requests = Some(runtime.requests);
        self.playback_controls = Some(runtime.controls);
        self.playback_paused = false;
        self.playback_generation = self.playback_generation.wrapping_add(1);
        self.recorder = recorder.clone();
        Self::monitor_audio_events(
            runtime.events,
            recorder.clone(),
            self.playback_generation,
            cx,
        );
        Self::monitor_live_events(spawn_live_agent(prompt, request_id, recorder), cx);
        cx.notify();
    }

    fn start_recorded_from(&mut self, ordinal: usize, cx: &mut Context<Self>) {
        let Some(bundle) = self.replay_bundle.clone() else {
            return;
        };
        if let Some(controls) = self.playback_controls.take() {
            let _ = controls.try_send(AudioCommand::Stop);
        }
        self.narration_requests.take();
        self.pending_slides.clear();
        self.focused_blocks.clear();
        self.transition = None;
        self.graph_camera = None;
        self.graph_trace_started = None;
        self.graph_viewports.clear();
        self.code_surfaces.clear();
        self.tree_surfaces.clear();
        self.block_scrolls = self
            .slides
            .iter()
            .flat_map(|slide| slide.spec.blocks.iter())
            .map(|block| (block.id.clone(), ScrollHandle::new()))
            .collect();
        self.audio_progress = None;
        self.playback_paused = false;
        self.replay_mode = true;
        let start = ordinal.min(self.slides.len().saturating_sub(1));
        if let Some(slide) = self.slides.get(start) {
            self.current_id = Some(slide.spec.id.clone());
        }
        let recordings = self
            .slides
            .iter()
            .skip(start)
            .filter_map(|slide| {
                let path = bundle.narration_path(&slide.spec.narration.id);
                if !path.is_file() {
                    return None;
                }
                self.pending_slides
                    .insert(slide.spec.narration.id.clone(), slide.spec.id.clone());
                Some(RecordedNarration {
                    beat: slide.spec.narration.clone(),
                    path,
                })
            })
            .collect::<Vec<_>>();
        if recordings.is_empty() {
            self.audio_status = "This presentation has no retained narration".into();
            return;
        }
        self.playback_generation = self.playback_generation.wrapping_add(1);
        let runtime = PlaybackRuntime::spawn(recordings);
        self.playback_controls = Some(runtime.controls);
        Self::monitor_audio_events(runtime.events, None, self.playback_generation, cx);
    }

    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        let Some(controls) = self.playback_controls.as_ref() else {
            return;
        };
        let command = if self.playback_paused {
            AudioCommand::Resume
        } else {
            AudioCommand::Pause
        };
        if controls.try_send(command).is_ok() {
            self.playback_paused = !self.playback_paused;
            cx.notify();
        }
    }

    fn replay_current(&mut self, cx: &mut Context<Self>) {
        let ordinal = self.current_slide().map_or(0, |slide| slide.ordinal);
        self.start_recorded_from(ordinal, cx);
        cx.notify();
    }

    fn replay_beginning(&mut self, cx: &mut Context<Self>) {
        self.start_recorded_from(0, cx);
        cx.notify();
    }

    fn accept_live_batch(&mut self, events: Vec<EventEnvelope>) {
        for event in events {
            match event.event {
                WorkEvent::SlidePlanned { slide } => {
                    self.activity_status = None;
                    self.register_slide(slide);
                }
                WorkEvent::PresenceProposed { cue } => {
                    self.activity_status = Some(cue.text.clone());
                    if cue.speak && self.slides.is_empty() {
                        let beat = NarrationBeat {
                            id: cue.id,
                            text: cue.text,
                            style: NarrationStyle::Neutral,
                            style_degree: 0.9,
                            focus: Vec::new(),
                            anchors: Vec::new(),
                        };
                        let _ = self
                            .narration_requests
                            .as_ref()
                            .and_then(|sender| sender.try_send(beat).ok());
                    }
                }
                WorkEvent::SessionCompleted {
                    status, summary, ..
                } if matches!(
                    status,
                    CompletionStatus::Failed | CompletionStatus::Cancelled
                ) =>
                {
                    self.activity_status = Some(match status {
                        CompletionStatus::Cancelled => "Presentation cancelled".into(),
                        _ => format!("Presentation failed · {summary}"),
                    });
                    self.audio_status = "Presentation unavailable".into();
                }
                _ => {}
            }
        }
    }

    fn register_slide(&mut self, mut slide: PresentationSlide) {
        if self.slides.iter().any(|entry| entry.spec.id == slide.id) {
            return;
        }
        let ordinal = self.slides.len();
        let _compile_report = foyer_shell_presentation::compile_slide(&mut slide, ordinal);
        let (x, y) = self
            .slides
            .last()
            .map_or((0, 0), |previous| match slide.axis {
                SlideAxis::Root if self.slides.is_empty() => (0, 0),
                SlideAxis::Vertical => (previous.x, previous.y + 1),
                SlideAxis::Root | SlideAxis::Horizontal => (previous.x + 1, previous.y),
            });
        let narration = slide.narration.clone();
        let slide_id = slide.id.clone();
        for block in &slide.blocks {
            self.block_scrolls.entry(block.id.clone()).or_default();
        }
        self.slides.push(DeckSlide {
            spec: slide,
            x,
            y,
            ordinal,
            revealed: false,
        });
        if self
            .narration_requests
            .as_ref()
            .is_some_and(|sender| sender.try_send(narration.clone()).is_ok())
        {
            self.pending_slides.insert(narration.id, slide_id);
            self.audio_status = format!("Voice · preparing slide {}", ordinal + 1);
        } else {
            self.reveal_slide(&slide_id);
        }
    }

    fn handle_audio_event(&mut self, event: AudioEvent) {
        match event {
            AudioEvent::WorkerReady { load_ms } => {
                self.audio_status = if load_ms == 0 {
                    "Voice · ready".into()
                } else {
                    format!("Voice · ready in {load_ms} ms")
                };
            }
            AudioEvent::Synthesizing { beat_id } => {
                let slide_number = self
                    .pending_slides
                    .get(&beat_id)
                    .and_then(|id| self.slide(id))
                    .map(|slide| slide.ordinal + 1)
                    .unwrap_or_default();
                self.audio_status = format!("Voice · synthesizing slide {slide_number}");
            }
            AudioEvent::PlaybackStarted {
                beat_id,
                focus,
                duration_ms,
                synthesis_ms,
                voice,
            } => {
                if let Some(slide_id) = self.pending_slides.remove(&beat_id) {
                    self.reveal_slide(&slide_id);
                }
                self.focused_blocks = focus.iter().cloned().collect();
                self.direct_graph_camera(&focus);
                self.graph_trace_started = None;
                self.audio_progress = Some((0, duration_ms));
                self.audio_status = format!("{voice} · {synthesis_ms} ms synth");
            }
            AudioEvent::Position {
                position_ms,
                duration_ms,
                ..
            } => self.audio_progress = Some((position_ms, duration_ms)),
            AudioEvent::Cue { action, .. } => self.apply_cue(action),
            AudioEvent::PlaybackFinished { .. } => {
                if let Some((_, duration)) = self.audio_progress {
                    self.audio_progress = Some((duration, duration));
                }
            }
            AudioEvent::RecordingStored { .. } => {}
            AudioEvent::Paused => self.playback_paused = true,
            AudioEvent::Resumed => self.playback_paused = false,
            AudioEvent::Stopped => {}
            AudioEvent::Failed { beat_id, message } => {
                eprintln!("Foyer Shell voice unavailable: {message}");
                if let Some(beat_id) = beat_id {
                    if let Some(slide_id) = self.pending_slides.remove(&beat_id) {
                        self.reveal_slide(&slide_id);
                    }
                } else {
                    let pending = std::mem::take(&mut self.pending_slides);
                    for slide_id in pending.into_values() {
                        self.reveal_slide(&slide_id);
                    }
                }
                self.audio_status = format!("Voice unavailable · {message}");
            }
        }
    }

    fn apply_cue(&mut self, action: CueAction) {
        match action {
            CueAction::Focus { ids } | CueAction::Emphasize { ids } => {
                for id in &ids {
                    if let Some(scroll) = self.block_scrolls.get(id) {
                        scroll.scroll_to_top_of_item(0);
                    }
                }
                let mut focused: BTreeSet<_> = ids.iter().cloned().collect();
                if let Some(graph) = self
                    .current_slide()
                    .and_then(|slide| slide.spec.graph.as_ref())
                {
                    for edge in &graph.edges {
                        if focused.contains(&edge.to) {
                            focused.insert(edge.id.clone());
                        }
                    }
                }
                self.focused_blocks = focused;
                self.direct_graph_camera(&ids);
                self.graph_trace_started = None;
            }
            CueAction::FollowPath { ids } => {
                let mut focused: BTreeSet<_> = ids.iter().cloned().collect();
                if let Some(graph) = self
                    .current_slide()
                    .and_then(|slide| slide.spec.graph.as_ref())
                {
                    for edge in &graph.edges {
                        if focused.contains(&edge.id) {
                            focused.insert(edge.from.clone());
                            focused.insert(edge.to.clone());
                        }
                    }
                }
                self.focused_blocks = focused;
                self.direct_graph_camera(&ids);
                self.graph_trace_started = Some(Instant::now());
            }
            CueAction::Recede { ids } => {
                for id in ids {
                    self.focused_blocks.remove(&id);
                }
            }
        }
    }

    fn direct_graph_camera(&mut self, ids: &[String]) {
        let Some(slide) = self.current_slide() else {
            return;
        };
        let Some(graph) = slide.spec.graph.as_ref() else {
            return;
        };
        let slide_id = slide.spec.id.clone();
        let node_ids: BTreeSet<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
        let target = ids
            .iter()
            .find(|id| node_ids.contains(id.as_str()))
            .cloned()
            .or_else(|| {
                ids.iter().find_map(|id| {
                    graph
                        .edges
                        .iter()
                        .find(|edge| edge.id == *id)
                        .map(|edge| edge.to.clone())
                })
            });
        let Some(to_node) = target else {
            return;
        };
        let from_node = self
            .graph_camera
            .as_ref()
            .filter(|camera| camera.slide_id == slide_id)
            .map(|camera| camera.to_node.clone());
        self.graph_camera = Some(GraphCameraTransition {
            slide_id,
            from_node,
            to_node,
            started: Instant::now(),
        });
    }

    fn reveal_slide(&mut self, id: &str) {
        if let Some(slide) = self.slides.iter_mut().find(|slide| slide.spec.id == id) {
            slide.revealed = true;
        }
        self.navigate_to(id);
    }

    fn navigate_to(&mut self, id: &str) {
        let Some(target) = self.slide(id) else {
            return;
        };
        if !target.revealed || self.current_id.as_deref() == Some(id) {
            return;
        }
        let (target_x, target_y) = (target.x, target.y);
        let (from_x, from_y) = self
            .current_slide()
            .map(|slide| (slide.x, slide.y))
            .unwrap_or((target_x, target_y));
        let delta_x = (target_x - from_x).signum() as f32;
        let delta_y = (target_y - from_y).signum() as f32;
        let from_id = self.current_id.clone();
        self.current_id = Some(id.to_string());
        self.transition = Some(SlideTransition {
            from_id,
            to_id: id.to_string(),
            direction_x: delta_x,
            direction_y: delta_y,
            started: Instant::now(),
        });
    }

    fn handle_navigation_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.keystroke.key.as_str() {
            "space" if self.replay_mode => {
                self.toggle_playback(cx);
                cx.stop_propagation();
                return;
            }
            "escape" if self.can_exit => {
                if let Some(controls) = self.playback_controls.take() {
                    let _ = controls.try_send(AudioCommand::Stop);
                }
                cx.emit(PlayerEvent::Exit);
                cx.stop_propagation();
                return;
            }
            "home" if self.replay_bundle.is_some() => {
                self.replay_beginning(cx);
                cx.stop_propagation();
                return;
            }
            _ => {}
        }
        let direction = match event.keystroke.key.as_str() {
            "left" => Some((-1, 0)),
            "right" => Some((1, 0)),
            "up" => Some((0, -1)),
            "down" => Some((0, 1)),
            _ => None,
        };
        let Some((dx, dy)) = direction else { return };
        let Some(current) = self.current_slide() else {
            return;
        };
        let target = self
            .slides
            .iter()
            .filter(|slide| slide.revealed)
            .find(|slide| slide.x == current.x + dx && slide.y == current.y + dy)
            .map(|slide| slide.spec.id.clone());
        if let Some(target) = target {
            if self.replay_bundle.is_some() {
                let ordinal = self.slide(&target).map_or(0, |slide| slide.ordinal);
                self.start_recorded_from(ordinal, cx);
            } else {
                self.navigate_to(&target);
            }
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn slide(&self, id: &str) -> Option<&DeckSlide> {
        self.slides.iter().find(|slide| slide.spec.id == id)
    }

    fn current_slide(&self) -> Option<&DeckSlide> {
        self.current_id.as_deref().and_then(|id| self.slide(id))
    }

    fn slide_layer(
        &mut self,
        slide: &DeckSlide,
        offset_x: f32,
        offset_y: f32,
        reveal_progress: f32,
        viewport_width: f32,
        viewport_height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let horizontal_padding = if viewport_width < 900.0 { 44.0 } else { 88.0 };
        let is_graph = slide.spec.graph.is_some();
        let is_code = slide.spec.code.is_some();
        let is_full_surface = is_graph || is_code;
        let content_top = if is_full_surface && viewport_height < 720.0 {
            124.0
        } else if is_full_surface {
            146.0
        } else if viewport_height < 720.0 {
            68.0
        } else {
            76.0
        };
        let content_bottom = 126.0;
        let content_width = (viewport_width - horizontal_padding * 2.0).max(520.0);
        let content_height = (viewport_height - content_top - content_bottom).max(380.0);
        let rects = pack_bento(
            &slide.spec.blocks,
            content_width,
            content_height,
            &slide.spec.id,
        );
        let title_width = (content_width * 0.72).max(320.0);
        let title_size = fit_title_size(&slide.spec.title, title_width, 54.0);

        let mut layer = div()
            .absolute()
            .left(px(offset_x))
            .top(px(offset_y))
            .w(px(viewport_width))
            .h(px(viewport_height))
            .overflow_hidden();

        if is_full_surface {
            layer = layer.child(
                div()
                    .absolute()
                    .left(px(horizontal_padding))
                    .top(px(76.0))
                    .right(px(horizontal_padding))
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .w(px(title_width))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(slide.spec.eyebrow.as_ref().map(|eyebrow| {
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(MUTED))
                                    .child(eyebrow.to_uppercase())
                            }))
                            .child(
                                div()
                                    .whitespace_normal()
                                    .text_size(px(title_size))
                                    .line_height(relative(1.08))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(FOREGROUND))
                                    .child(slide.spec.title.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(Hsla::from(rgb(MUTED)).opacity(0.64))
                            .child(format!("{:02}", slide.ordinal + 1)),
                    ),
            );
        }

        if let Some(graph) = slide.spec.graph.as_ref() {
            return layer.child(self.graph_surface(
                &slide.spec.id,
                graph,
                horizontal_padding,
                content_top,
                content_width,
                content_height,
                reveal_progress,
                cx,
            ));
        }

        if let Some(code) = slide.spec.code.as_ref() {
            return layer.child(self.code_surface(
                &slide.spec.id,
                code,
                horizontal_padding,
                content_top,
                content_width,
                content_height,
                reveal_progress,
                window,
                cx,
            ));
        }

        for (index, block) in slide.spec.blocks.iter().enumerate() {
            let Some(rect) = rects.get(&block.id) else {
                continue;
            };
            let local_progress = block_reveal_progress(reveal_progress, index);
            let axis_offset = match slide.spec.axis {
                SlideAxis::Vertical => (0.0, 24.0 * (1.0 - local_progress)),
                SlideAxis::Root | SlideAxis::Horizontal => (28.0 * (1.0 - local_progress), 0.0),
            };
            layer = layer.child(self.block(
                block,
                VisualRect {
                    x: horizontal_padding + rect.x + axis_offset.0,
                    y: content_top + rect.y + axis_offset.1,
                    ..*rect
                },
                local_progress,
                cx,
            ));
        }
        layer
    }

    fn code_surface(
        &mut self,
        slide_id: &str,
        code: &SlideCode,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        reveal_progress: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active_step = code
            .steps
            .iter()
            .find(|step| self.focused_blocks.contains(&step.id));
        if !self.code_surfaces.contains_key(slide_id) {
            let state = CodeSurfaceState::new(code, window, cx);
            let explorer = state.explorer.clone();
            self._subscriptions
                .push(cx.observe(&explorer, |_, _, cx| cx.notify()));
            self.code_surfaces.insert(slide_id.to_string(), state);
        }
        let state = self
            .code_surfaces
            .get_mut(slide_id)
            .expect("code surface exists");
        state.sync(code, active_step, window, cx);
        div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .opacity(reveal_progress)
            .child(CodeSurface::render(state, code, active_step))
    }

    fn graph_surface(
        &self,
        slide_id: &str,
        graph: &SlideGraph,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        reveal_progress: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let layout = layout_graph(graph, width, height);
        let viewport_state = self
            .graph_viewports
            .get(slide_id)
            .cloned()
            .unwrap_or_default();
        let resting_pan = clamp_graph_pan(
            (viewport_state.pan_x, viewport_state.pan_y),
            width,
            height,
            layout.canvas_width,
            layout.canvas_height,
        );
        let camera = self
            .graph_camera
            .as_ref()
            .filter(|camera| camera.slide_id == slide_id);
        let target_pan = camera
            .and_then(|camera| layout.nodes.get(&camera.to_node))
            .map(|rect| graph_pan_for_node(rect, width, height, &layout))
            .unwrap_or(resting_pan);
        let from_pan = camera
            .and_then(|camera| camera.from_node.as_ref())
            .and_then(|node| layout.nodes.get(node))
            .map(|rect| graph_pan_for_node(rect, width, height, &layout))
            .unwrap_or(resting_pan);
        let camera_progress = camera.map_or(1.0, |camera| {
            ease_in_out_cubic((camera.started.elapsed().as_millis() as f32 / 420.0).clamp(0.0, 1.0))
        });
        let pan = (
            from_pan.0 + (target_pan.0 - from_pan.0) * camera_progress,
            from_pan.1 + (target_pan.1 - from_pan.1) * camera_progress,
        );
        let graph_ids: BTreeSet<_> = graph
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .chain(graph.edges.iter().map(|edge| edge.id.as_str()))
            .collect();
        let has_focus = self
            .focused_blocks
            .iter()
            .any(|id| graph_ids.contains(id.as_str()));
        let trace_progress = self.graph_trace_started.map_or(1.0, |started| {
            (started.elapsed().as_millis() as f32 / 850.0).clamp(0.0, 1.0)
        });
        let focused = self.focused_blocks.clone();
        let node_order = layout.node_order.clone();
        let painted_edges = layout.edges.clone();

        let mut world = div()
            .absolute()
            .left(px(pan.0))
            .top(px(pan.1))
            .w(px(layout.canvas_width))
            .h(px(layout.canvas_height))
            .child(
                canvas(
                    move |_, _, _| {},
                    move |bounds, _, window, _| {
                        for edge in painted_edges {
                            let order = node_order.get(&edge.to).copied().unwrap_or_default();
                            let reveal = block_reveal_progress(reveal_progress, order + 1);
                            if reveal <= 0.001 {
                                continue;
                            }
                            let edge_focused = focused.contains(&edge.id);
                            let node_connected =
                                focused.contains(&edge.from) && focused.contains(&edge.to);
                            let muted = has_focus && !edge_focused && !node_connected;
                            paint_graph_edge(
                                window,
                                bounds,
                                &edge,
                                reveal,
                                if muted { 0x29292d } else { 0x696970 },
                                if edge_focused { 2.4 } else { 1.5 },
                            );
                            if edge_focused {
                                paint_graph_edge(
                                    window,
                                    bounds,
                                    &edge,
                                    reveal.min(trace_progress),
                                    FOCUS,
                                    3.0,
                                );
                            }
                        }
                    },
                )
                .size_full(),
            );

        for edge in &layout.edges {
            let Some(label) = edge.label.as_ref() else {
                continue;
            };
            let edge_progress = block_reveal_progress(
                reveal_progress,
                layout.node_order.get(&edge.to).copied().unwrap_or_default() + 1,
            );
            let focused_edge = self.focused_blocks.contains(&edge.id);
            let label_bounds = edge.label_bounds;
            world = world.child(
                div()
                    .absolute()
                    .left(px(label_bounds.x))
                    .top(px(label_bounds.y))
                    .w(px(label_bounds.width))
                    .h(px(label_bounds.height))
                    .px_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.0))
                    .bg(Hsla::from(rgb(SURFACE_RAISED)).opacity(0.98))
                    .border_1()
                    .border_color(if focused_edge {
                        rgb(FOCUS)
                    } else {
                        rgb(BORDER)
                    })
                    .opacity(if has_focus && !focused_edge {
                        0.34
                    } else {
                        edge_progress
                    })
                    .text_size(px(11.0))
                    .line_height(relative(1.1))
                    .text_center()
                    .whitespace_normal()
                    .overflow_hidden()
                    .text_color(if focused_edge {
                        rgb(FOREGROUND)
                    } else {
                        rgb(MUTED)
                    })
                    .child(label.clone()),
            );
        }

        for node in &graph.nodes {
            let Some(rect) = layout.nodes.get(&node.id) else {
                continue;
            };
            let index = layout.node_order.get(&node.id).copied().unwrap_or_default();
            let progress = block_reveal_progress(reveal_progress, index);
            let focused_node = self.focused_blocks.contains(&node.id);
            let opacity = if has_focus && !focused_node {
                progress * 0.3
            } else {
                progress
            };
            let accent = graph_node_color(node.role);
            let show_detail = rect.height >= 80.0;
            world = world.child(
                div()
                    .absolute()
                    .left(px(rect.x + 18.0 * (1.0 - progress)))
                    .top(px(rect.y))
                    .w(px(rect.width))
                    .h(px(rect.height))
                    .overflow_hidden()
                    .opacity(opacity)
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .rounded(px(CARD_RADIUS))
                    .border(if focused_node { px(2.0) } else { px(1.0) })
                    .border_color(if focused_node {
                        Hsla::from(rgb(FOCUS))
                    } else {
                        Hsla::from(rgb(BORDER)).opacity(0.9)
                    })
                    .bg(Hsla::from(if focused_node {
                        rgb(SURFACE_RAISED)
                    } else {
                        rgb(SURFACE)
                    })
                    .opacity(0.98))
                    .when(focused_node, |element| element.shadow_lg())
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(accent))
                            .child(graph_role_label(node.role)),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(px(if rect.height < 72.0 { 14.0 } else { 17.0 }))
                            .line_height(relative(1.18))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(FOREGROUND))
                            .child(node.label.clone()),
                    )
                    .children(
                        show_detail
                            .then(|| {
                                node.detail.as_ref().map(|detail| {
                                    div()
                                        .whitespace_normal()
                                        .text_xs()
                                        .line_height(relative(1.32))
                                        .text_color(rgb(MUTED))
                                        .child(detail.clone())
                                })
                            })
                            .flatten(),
                    ),
            );
        }
        let canvas_width = layout.canvas_width;
        let canvas_height = layout.canvas_height;
        let down_slide = slide_id.to_string();
        let move_slide = slide_id.to_string();
        let up_slide = slide_id.to_string();
        div()
            .id(gpui::SharedString::from(format!(
                "graph-viewport-{slide_id}"
            )))
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .cursor(if viewport_state.dragging_from.is_some() {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    let state = view.graph_viewports.entry(down_slide.clone()).or_default();
                    state.pan_x = pan.0;
                    state.pan_y = pan.1;
                    state.dragging_from =
                        Some((f32::from(event.position.x), f32::from(event.position.y)));
                    view.graph_camera = None;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| {
                let state = view.graph_viewports.entry(move_slide.clone()).or_default();
                if event.pressed_button != Some(MouseButton::Left) {
                    state.dragging_from = None;
                    return;
                }
                let position = (f32::from(event.position.x), f32::from(event.position.y));
                if let Some(previous) = state.dragging_from {
                    let next = clamp_graph_pan(
                        (
                            state.pan_x + position.0 - previous.0,
                            state.pan_y + position.1 - previous.1,
                        ),
                        width,
                        height,
                        canvas_width,
                        canvas_height,
                    );
                    state.pan_x = next.0;
                    state.pan_y = next.1;
                }
                state.dragging_from = Some(position);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |view, _: &MouseUpEvent, _, cx| {
                    if let Some(state) = view.graph_viewports.get_mut(&up_slide) {
                        state.dragging_from = None;
                    }
                    cx.notify();
                }),
            )
            .child(world)
    }

    fn block(
        &mut self,
        block: &SlideBlock,
        rect: VisualRect,
        opacity: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let focused = self.focused_blocks.contains(&block.id);
        let fit = fit_typography(block, rect);
        let scroll_handle = self.block_scrolls.get(&block.id);
        let text_width = (rect.width - CARD_PADDING * 2.0).max(32.0);
        let wrapped_content = if block.kind == SlideBlockKind::Code {
            block.content.clone()
        } else {
            wrap_text_to_width(&block.content, text_width, fit.font_size, false)
        };
        let mut element = div()
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.width))
            .h(px(rect.height))
            .overflow_hidden()
            .opacity(opacity)
            .rounded(px(CARD_RADIUS))
            .border(if focused { px(2.0) } else { px(1.0) })
            .border_color(if focused {
                Hsla::from(rgb(FOCUS)).opacity(0.98)
            } else {
                Hsla::from(rgb(BORDER)).opacity(0.82)
            })
            .bg(Hsla::from(if focused {
                rgb(SURFACE_RAISED)
            } else {
                rgb(SURFACE)
            })
            .opacity(0.97))
            .when(focused, |element| element.shadow_lg());

        if block.kind == SlideBlockKind::Image
            && let Some(uri) = block.uri.as_ref()
        {
            let image = if uri.starts_with("http://")
                || uri.starts_with("https://")
                || uri.starts_with("data:")
            {
                img(uri.clone())
                    .size_full()
                    .rounded(px(CARD_RADIUS - 1.0))
                    .object_fit(ObjectFit::Cover)
                    .with_loading(|| image_status("Loading image…", false).into_any_element())
                    .with_fallback(|| image_status("Image unavailable", true).into_any_element())
            } else {
                img(PathBuf::from(uri))
                    .size_full()
                    .rounded(px(CARD_RADIUS - 1.0))
                    .object_fit(ObjectFit::Cover)
                    .with_loading(|| image_status("Loading image…", false).into_any_element())
                    .with_fallback(|| image_status("Image unavailable", true).into_any_element())
            };
            element = element.child(image);
            if !block.content.is_empty() {
                element = element.child(
                    div()
                        .absolute()
                        .left_3()
                        .right_3()
                        .bottom_3()
                        .px_3()
                        .py_2()
                        .rounded(px(12.0))
                        .bg(Hsla::from(rgb(SURFACE_RECESSED)).opacity(0.9))
                        .text_sm()
                        .whitespace_normal()
                        .text_color(rgb(FOREGROUND))
                        .child(wrap_text_to_width(
                            &block.content,
                            (rect.width - 48.0).max(32.0),
                            14.0,
                            false,
                        )),
                );
            }
            return element;
        }

        let padding = CARD_PADDING;
        element = element.p(px(padding)).flex().flex_col().gap_2().children(
            block
                .title
                .as_ref()
                .or(block.language.as_ref())
                .map(|title| {
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if focused {
                            Hsla::from(rgb(FOREGROUND))
                        } else {
                            Hsla::from(rgb(MUTED)).opacity(0.72)
                        })
                        .child(title.to_uppercase())
                }),
        );

        let content = match block.kind {
            SlideBlockKind::DisplayText => div()
                .w(px(text_width))
                .max_w_full()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .overflow_x_hidden()
                .whitespace_normal()
                .text_size(px(fit.font_size))
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(relative(fit.line_height))
                .text_color(rgb(FOREGROUND))
                .child(wrapped_content.clone()),
            SlideBlockKind::Statistic => div()
                .w(px(text_width))
                .max_w_full()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .overflow_x_hidden()
                .whitespace_normal()
                .text_size(px(fit.font_size))
                .font_weight(FontWeight::BOLD)
                .line_height(relative(fit.line_height))
                .text_color(rgb(FOREGROUND))
                .child(wrapped_content.clone()),
            SlideBlockKind::Equation => div()
                .w(px(text_width))
                .max_w_full()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .justify_center()
                .overflow_x_hidden()
                .whitespace_normal()
                .child(math_expression(&block.content, fit.font_size)),
            SlideBlockKind::Code => div()
                .w(px(text_width))
                .max_w_full()
                .min_w(px(0.0))
                .overflow_x_hidden()
                .whitespace_normal()
                .font_family("monospace")
                .text_size(px(fit.font_size))
                .line_height(relative(fit.line_height))
                .text_color(Hsla::from(rgb(FOREGROUND)).opacity(0.92))
                .child(highlighted_code(
                    &wrapped_content,
                    block.language.as_deref(),
                )),
            SlideBlockKind::Callout => div()
                .w(px(text_width))
                .max_w_full()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .overflow_x_hidden()
                .whitespace_normal()
                .border_l_2()
                .border_color(if focused { rgb(FOCUS) } else { rgb(SUBTLE) })
                .pl_4()
                .text_size(px(fit.font_size))
                .line_height(relative(fit.line_height))
                .text_color(rgb(FOREGROUND))
                .child(wrapped_content.clone()),
            SlideBlockKind::Chart => block.chart.as_ref().map_or_else(
                || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("Chart unavailable")
                },
                |chart| {
                    div().size_full().child(foyer_shell_presentation_ui::chart(
                        chart,
                        opacity,
                        text_width,
                        (rect.height - CARD_PADDING * 2.0 - 24.0).max(80.0),
                    ))
                },
            ),
            SlideBlockKind::Tree => block.tree.as_ref().map_or_else(
                || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child("Tree unavailable")
                },
                |tree| {
                    let state = self
                        .tree_surfaces
                        .entry(block.id.clone())
                        .or_insert_with(|| TreeSurfaceState::new(tree, cx));
                    div().size_full().child(TreeSurface::render(state))
                },
            ),
            SlideBlockKind::Text | SlideBlockKind::Image => div()
                .w(px(text_width))
                .max_w_full()
                .min_w(px(0.0))
                .overflow_x_hidden()
                .whitespace_normal()
                .text_size(px(fit.font_size))
                .line_height(relative(fit.line_height))
                .text_color(if block.emphasis == BlockEmphasis::Quiet {
                    rgb(MUTED)
                } else {
                    rgb(FOREGROUND)
                })
                .child(wrapped_content),
        };
        let mut content_viewport = div()
            .id(gpui::SharedString::from(format!(
                "block-scroll-{}",
                block.id
            )))
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .w(px(text_width))
            .max_w_full()
            .overflow_x_hidden()
            .child(content);
        if fit.scroll {
            content_viewport = content_viewport
                .overflow_y_scroll()
                .overflow_x_hidden()
                .scrollbar_width(px(5.0));
            if let Some(handle) = scroll_handle {
                content_viewport = content_viewport.track_scroll(handle);
            }
        } else {
            content_viewport = content_viewport.overflow_hidden();
        }
        element = element.child(content_viewport);
        if fit.scroll {
            element = element.child(scroll_indicator(scroll_handle, rect, fit));
        }
        element
    }

    fn minimap(&self, cx: &mut Context<Self>) -> Div {
        if self.slides.is_empty() {
            return div();
        }
        let min_x = self.slides.iter().map(|slide| slide.x).min().unwrap_or(0);
        let max_x = self.slides.iter().map(|slide| slide.x).max().unwrap_or(0);
        let min_y = self.slides.iter().map(|slide| slide.y).min().unwrap_or(0);
        let max_y = self.slides.iter().map(|slide| slide.y).max().unwrap_or(0);
        let cell_width = 20.0;
        let cell_height = 12.0;
        let gap = 6.0;
        let width = (max_x - min_x + 1) as f32 * (cell_width + gap) - gap;
        let height = (max_y - min_y + 1) as f32 * (cell_height + gap) - gap;
        let mut map = div()
            .absolute()
            .right(px(24.0))
            .top(px(18.0))
            .w(px(width.max(44.0) + 20.0))
            .h(px(height + 20.0))
            .rounded(px(15.0))
            .border_1()
            .border_color(Hsla::from(rgb(BORDER)).opacity(0.82))
            .bg(Hsla::from(rgb(SURFACE)).opacity(0.94))
            .shadow_lg()
            .overflow_hidden();
        let map_left = ((width.max(44.0) - width) * 0.5 + 10.0).max(10.0);
        for slide in &self.slides {
            let id = slide.spec.id.clone();
            let ordinal = slide.ordinal;
            let is_current = self.current_id.as_deref() == Some(id.as_str());
            map = map.child(
                div()
                    .id(("minimap-slide", slide.ordinal))
                    .absolute()
                    .left(px(map_left + (slide.x - min_x) as f32 * (cell_width + gap)))
                    .top(px(10.0 + (slide.y - min_y) as f32 * (cell_height + gap)))
                    .w(px(cell_width))
                    .h(px(cell_height))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if is_current {
                        Hsla::from(rgb(FOCUS))
                    } else if slide.revealed {
                        Hsla::from(rgb(FOREGROUND)).opacity(0.44)
                    } else {
                        Hsla::from(rgb(BORDER)).opacity(0.75)
                    })
                    .bg(if is_current {
                        Hsla::from(rgb(FOREGROUND)).opacity(0.94)
                    } else if slide.revealed {
                        Hsla::from(rgb(FOREGROUND)).opacity(0.12)
                    } else {
                        Hsla::from(rgb(SURFACE)).opacity(0.72)
                    })
                    .cursor(if slide.revealed {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::Arrow
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| {
                            if view.replay_bundle.is_some() {
                                view.start_recorded_from(ordinal, cx);
                            } else {
                                view.navigate_to(&id);
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        map
    }

    fn player_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let control = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_3()
                .py_2()
                .rounded(px(10.0))
                .border_1()
                .border_color(rgb(BORDER))
                .text_xs()
                .text_color(rgb(FOREGROUND))
                .cursor_pointer()
                .hover(|button| button.bg(rgb(SURFACE_RAISED)))
                .child(label)
        };
        let mut toolbar = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(48.0))
            .flex()
            .justify_center()
            .child(
                div()
                    .h(px(48.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(16.0))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .bg(Hsla::from(rgb(SURFACE)).opacity(0.98))
                    .shadow_lg()
                    .child(
                        control(
                            "player-play-pause",
                            if self.playback_paused {
                                "Play"
                            } else {
                                "Pause"
                            },
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| view.toggle_playback(cx)),
                        ),
                    )
                    .child(control("player-replay-slide", "Replay slide").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| view.replay_current(cx)),
                    ))
                    .child(control("player-replay-all", "From beginning").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| view.replay_beginning(cx)),
                    )),
            );
        if self.can_exit {
            toolbar = toolbar.child(div().absolute().right(px(28.0)).bottom(px(56.0)).child(
                control("player-exit", "Done").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|view, _, _, cx| {
                        if let Some(controls) = view.playback_controls.take() {
                            let _ = controls.try_send(AudioCommand::Stop);
                        }
                        cx.emit(PlayerEvent::Exit);
                    }),
                ),
            ));
        }
        toolbar
    }
}

impl Render for PresentationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.prompt_should_clear {
            self.prompt_should_clear = false;
            self.prompt_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        let now = Instant::now();
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);

        let transition_progress = self.transition.as_ref().map(|transition| {
            (now.duration_since(transition.started).as_millis() as f32 / SLIDE_TRANSITION_MS as f32)
                .clamp(0.0, 1.0)
        });
        let transition_active = transition_progress.is_some_and(|progress| progress < 1.0);
        if transition_progress.is_some_and(|progress| progress >= 1.0) {
            self.transition = None;
        }
        let graph_trace_active = self
            .graph_trace_started
            .is_some_and(|started| started.elapsed().as_millis() < 850);
        let graph_camera_active = self
            .graph_camera
            .as_ref()
            .is_some_and(|camera| camera.started.elapsed().as_millis() < 420);
        if transition_active
            || graph_trace_active
            || graph_camera_active
            || self
                .audio_progress
                .is_some_and(|(position, duration)| position < duration)
        {
            window.request_animation_frame();
        }

        let prompt_busy = self.run_busy();

        let mut root = div()
            .id("foyer-shell-root")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(BACKGROUND))
            .text_color(rgb(FOREGROUND))
            .on_key_down(cx.listener(Self::handle_navigation_key));

        if let Some(transition) = self.transition.clone() {
            let progress = ease_in_out_cubic(transition_progress.unwrap_or(1.0));
            if let Some(from_id) = transition.from_id.as_ref()
                && let Some(from) = self.slide(from_id).cloned()
            {
                root = root.child(self.slide_layer(
                    &from,
                    -transition.direction_x * width * progress,
                    -transition.direction_y * height * progress,
                    1.0,
                    width,
                    height,
                    window,
                    cx,
                ));
            }
            if let Some(to) = self.slide(&transition.to_id).cloned() {
                root = root.child(self.slide_layer(
                    &to,
                    transition.direction_x * width * (1.0 - progress),
                    transition.direction_y * height * (1.0 - progress),
                    progress,
                    width,
                    height,
                    window,
                    cx,
                ));
            }
        } else if let Some(current) = self.current_slide().cloned() {
            root = root.child(self.slide_layer(&current, 0.0, 0.0, 1.0, width, height, window, cx));
        }

        root = root.child(self.minimap(cx));

        let show_player_toolbar = self.can_exit
            || self.replay_mode
            || self
                .slides
                .first()
                .is_some_and(|slide| slide.spec.id != "welcome");
        if show_player_toolbar {
            root = root.child(self.player_toolbar(cx));
        } else {
            root = root.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(50.0))
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("prompt-composer")
                            .w(px(width.min(760.0) - 36.0))
                            .h(px(48.0))
                            .px_4()
                            .flex()
                            .items_center()
                            .gap_3()
                            .overflow_hidden()
                            .rounded(px(16.0))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(Hsla::from(rgb(SURFACE)).opacity(0.98))
                            .shadow_lg()
                            .child(
                                Prompt::new(&self.prompt_input)
                                    .flex_1()
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .disabled(prompt_busy),
                            )
                            .child(
                                div()
                                    .id("run-prompt")
                                    .px_2()
                                    .py_1()
                                    .rounded(px(9.0))
                                    .border_1()
                                    .border_color(if prompt_busy {
                                        rgb(BORDER)
                                    } else {
                                        rgb(FOREGROUND)
                                    })
                                    .text_xs()
                                    .text_color(if prompt_busy {
                                        rgb(MUTED)
                                    } else {
                                        rgb(FOREGROUND)
                                    })
                                    .cursor_pointer()
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|view, _, _, cx| view.submit_prompt(cx)),
                                    )
                                    .child(if prompt_busy { "Working" } else { "Run ↵" }),
                            ),
                    ),
            );
        }

        let timeline = self
            .audio_progress
            .map(|(position, duration)| position as f32 / duration.max(1) as f32)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        root.child(
            div()
                .absolute()
                .left(px(28.0))
                .right(px(28.0))
                .bottom(px(30.0))
                .h(px(2.0))
                .rounded_full()
                .bg(rgb(BORDER))
                .child(
                    div()
                        .h_full()
                        .w(relative(timeline))
                        .rounded_full()
                        .bg(rgb(FOREGROUND)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px(28.0))
                .bottom(px(10.0))
                .text_xs()
                .text_color(rgb(MUTED))
                .child(if self.stream_open {
                    "OpenRouter planner · authoring"
                } else {
                    "← → time   ·   ↑ ↓ detail"
                }),
        )
        .child(
            div()
                .absolute()
                .right(px(28.0))
                .bottom(px(10.0))
                .text_xs()
                .text_color(rgb(MUTED))
                .child(
                    self.activity_status
                        .clone()
                        .unwrap_or_else(|| self.audio_status.clone()),
                ),
        )
    }
}

#[cfg(test)]
fn sanitize_slide(slide: &mut PresentationSlide) {
    let code_valid = if let Some(code) = slide.code.as_mut() {
        code.content = code
            .content
            .lines()
            .take(240)
            .collect::<Vec<_>>()
            .join("\n");
        code.files.truncate(8);
        let mut file_ids = BTreeSet::new();
        code.files.retain_mut(|file| {
            file.content = file
                .content
                .lines()
                .take(240)
                .collect::<Vec<_>>()
                .join("\n");
            !file.id.is_empty()
                && !file.path.is_empty()
                && !file.content.trim().is_empty()
                && file_ids.insert(file.id.clone())
        });
        if let Some(primary) = code.files.first() {
            code.language = primary.language.clone();
            code.content = primary.content.clone();
        }
        let primary_id = code.files.first().map(|file| file.id.clone());
        code.steps.truncate(12);
        let mut seen = BTreeSet::new();
        code.steps.retain_mut(|step| {
            if step
                .file_id
                .as_ref()
                .is_none_or(|id| !file_ids.contains(id))
            {
                step.file_id = primary_id.clone();
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
            !step.id.is_empty() && seen.insert(step.id.clone())
        });
        code.show_explorer &= code.files.len() > 1;
        !code.content.trim().is_empty() && !code.steps.is_empty()
    } else {
        false
    };

    if code_valid {
        slide.composition = SlideComposition::Code;
        slide.graph = None;
        slide.blocks.clear();
    } else {
        slide.code = None;
    }

    if !code_valid && let Some(graph) = slide.graph.as_mut() {
        graph.direction = GraphDirection::LeftToRight;
        graph.nodes.truncate(24);
        let mut seen = BTreeSet::new();
        graph.nodes.retain(|node| {
            !node.id.is_empty() && !node.label.is_empty() && seen.insert(node.id.clone())
        });
        let node_ids: BTreeSet<_> = graph.nodes.iter().map(|node| node.id.clone()).collect();
        let mut edge_ids = BTreeSet::new();
        graph.edges.truncate(48);
        graph.edges.retain(|edge| {
            !edge.id.is_empty()
                && edge.from != edge.to
                && node_ids.contains(&edge.from)
                && node_ids.contains(&edge.to)
                && edge_ids.insert(edge.id.clone())
        });
        if graph.nodes.len() >= 2 {
            slide.composition = SlideComposition::Graph;
            slide.blocks.clear();
        } else {
            slide.graph = None;
            if slide.composition == SlideComposition::Graph {
                slide.composition = SlideComposition::Bento;
            }
        }
    } else if !code_valid && slide.composition == SlideComposition::Graph {
        slide.composition = SlideComposition::Bento;
    }
    if slide.graph.is_none() && slide.code.is_none() {
        slide.composition = SlideComposition::Bento;
    }
    slide.blocks.truncate(7);
    slide.blocks.retain_mut(|block| {
        block.columns = block.columns.clamp(1, 9);
        block.rows = block.rows.clamp(1, 9);
        if block.kind == SlideBlockKind::Tree {
            block.chart = None;
            let Some(tree) = block.tree.as_mut() else {
                return false;
            };
            let mut count = 0usize;
            sanitize_tree_nodes(&mut tree.nodes, 0, &mut count);
            return !block.id.is_empty() && !tree.nodes.is_empty();
        }
        block.tree = None;
        if block.kind != SlideBlockKind::Chart {
            block.chart = None;
            return !block.id.is_empty();
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
            candle.open.is_finite()
                && candle.high.is_finite()
                && candle.low.is_finite()
                && candle.close.is_finite()
                && !candle.label.is_empty()
        });
        if chart.kind == ChartKind::Candlestick {
            return !block.id.is_empty() && chart.candles.len() >= 2;
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
        !block.id.is_empty() && !chart.series.is_empty()
    });
    let mut valid_ids: BTreeSet<_> = slide.blocks.iter().map(|block| block.id.clone()).collect();
    if let Some(graph) = slide.graph.as_ref() {
        valid_ids.extend(graph.nodes.iter().map(|node| node.id.clone()));
        valid_ids.extend(graph.edges.iter().map(|edge| edge.id.clone()));
    }
    if let Some(code) = slide.code.as_ref() {
        valid_ids.extend(code.steps.iter().map(|step| step.id.clone()));
    }

    // The native director owns focus ordering. Segmented narration may provide exact character
    // offsets, but only if it covers the same visual order; malformed contracts fall back to
    // evenly spaced native cues rather than highlighting plausible-looking wrong content.
    let focus_steps: Vec<Vec<String>> = if let Some(code) = slide.code.as_ref() {
        code.steps
            .iter()
            .map(|step| vec![step.id.clone()])
            .collect()
    } else if let Some(graph) = slide.graph.as_ref() {
        graph_traversal(graph)
            .into_iter()
            .map(|(node_id, incoming_edge)| {
                let mut ids = vec![node_id];
                if let Some(edge_id) = incoming_edge {
                    ids.push(edge_id);
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
    let expected_primary: Vec<_> = focus_steps
        .iter()
        .filter_map(|ids| ids.first().cloned())
        .collect();
    let authored_primary: Vec<_> = slide
        .narration
        .focus
        .first()
        .cloned()
        .into_iter()
        .chain(slide.narration.anchors.iter().filter_map(|anchor| {
            let CueAction::Focus { ids } = &anchor.cue else {
                return None;
            };
            anchor.at_char.and_then(|_| ids.first().cloned())
        }))
        .collect();
    let exact_segment_contract = authored_primary == expected_primary
        && slide.narration.anchors.len() == focus_steps.len().saturating_sub(1);
    if exact_segment_contract {
        slide.narration.focus = focus_steps.first().cloned().unwrap_or_default();
        for (anchor, ids) in slide
            .narration
            .anchors
            .iter_mut()
            .zip(focus_steps.into_iter().skip(1))
        {
            anchor.cue = CueAction::Focus { ids };
        }
    } else {
        slide.narration.focus = focus_steps.first().cloned().unwrap_or_default();
        slide.narration.anchors = focus_steps
            .into_iter()
            .skip(1)
            .enumerate()
            .map(|(index, ids)| foyer_shell_protocol::NarrationAnchor {
                phrase: format!("__foyer_shell_focus_{}__", index + 1),
                at_char: None,
                cue: CueAction::Focus { ids },
            })
            .collect();
    }

    debug_assert!(
        slide
            .narration
            .focus
            .iter()
            .chain(
                slide
                    .narration
                    .anchors
                    .iter()
                    .flat_map(|anchor| match &anchor.cue {
                        CueAction::Focus { ids }
                        | CueAction::Emphasize { ids }
                        | CueAction::Recede { ids }
                        | CueAction::FollowPath { ids } => ids.iter(),
                    })
            )
            .all(|id| valid_ids.contains(id))
    );
}

#[cfg(test)]
fn sanitize_tree_nodes(nodes: &mut Vec<SlideTreeNode>, depth: usize, count: &mut usize) {
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
        sanitize_tree_nodes(&mut node.children, depth + 1, count);
        true
    });
}

fn graph_traversal(graph: &SlideGraph) -> Vec<(String, Option<String>)> {
    let node_order: BTreeMap<_, _> = graph
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
        edges.sort_by_key(|edge| {
            node_order
                .get(edge.to.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
    }

    let mut queue: VecDeque<(String, Option<String>)> = graph
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

    // Cycles do not have a topological root. Keep them navigable and deterministic by appending
    // the remaining authored nodes, attaching the first edge from an already visited node.
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

fn clamp_graph_pan(
    pan: (f32, f32),
    viewport_width: f32,
    viewport_height: f32,
    canvas_width: f32,
    canvas_height: f32,
) -> (f32, f32) {
    let clamp_axis = |value: f32, viewport: f32, canvas: f32| {
        if canvas <= viewport {
            (viewport - canvas) * 0.5
        } else {
            value.clamp(viewport - canvas, 0.0)
        }
    };
    (
        clamp_axis(pan.0, viewport_width, canvas_width),
        clamp_axis(pan.1, viewport_height, canvas_height),
    )
}

fn graph_pan_for_node(
    node: &VisualRect,
    viewport_width: f32,
    viewport_height: f32,
    layout: &GraphLayout,
) -> (f32, f32) {
    clamp_graph_pan(
        (
            viewport_width * 0.5 - (node.x + node.width * 0.5),
            viewport_height * 0.5 - (node.y + node.height * 0.5),
        ),
        viewport_width,
        viewport_height,
        layout.canvas_width,
        layout.canvas_height,
    )
}

fn layout_graph(graph: &SlideGraph, width: f32, height: f32) -> GraphLayout {
    const MARGIN: f32 = 30.0;
    const NODE_WIDTH: f32 = 220.0;
    const NODE_HEIGHT: f32 = 108.0;
    const COLUMN_GAP: f32 = 150.0;
    const ROW_GAP: f32 = 72.0;

    let mut indegree: BTreeMap<_, usize> = graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), 0))
        .collect();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in &graph.edges {
        *indegree.entry(edge.to.clone()).or_default() += 1;
        outgoing
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
    }
    let mut queue: VecDeque<_> = graph
        .nodes
        .iter()
        .filter(|node| indegree.get(&node.id).copied().unwrap_or_default() == 0)
        .map(|node| node.id.clone())
        .collect();
    let mut logical_rank: BTreeMap<String, usize> = BTreeMap::new();
    while let Some(id) = queue.pop_front() {
        let rank = *logical_rank.entry(id.clone()).or_default();
        for target in outgoing.get(&id).into_iter().flatten() {
            logical_rank
                .entry(target.clone())
                .and_modify(|value| *value = (*value).max(rank + 1))
                .or_insert(rank + 1);
            if let Some(degree) = indegree.get_mut(target) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    queue.push_back(target.clone());
                }
            }
        }
    }

    // Cyclic components have no Kahn root. Give their remaining nodes stable successive ranks;
    // their backwards edges are routed through a feedback lane below the graph.
    for (fallback_rank, node) in graph.nodes.iter().enumerate() {
        logical_rank.entry(node.id.clone()).or_insert(fallback_rank);
    }
    let layer_count = logical_rank.values().copied().max().unwrap_or(0) + 1;
    let mut layer_members = vec![Vec::<String>::new(); layer_count];
    for node in &graph.nodes {
        let rank = logical_rank.get(&node.id).copied().unwrap_or_default();
        layer_members[rank].push(node.id.clone());
    }
    let maximum_rows = layer_members.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let has_feedback = graph.edges.iter().any(|edge| {
        logical_rank.get(&edge.to).copied().unwrap_or_default()
            <= logical_rank.get(&edge.from).copied().unwrap_or_default()
    });
    let feedback_lane = if has_feedback { 94.0 } else { 0.0 };
    let horizontal_gutter = ((width - NODE_WIDTH) * 0.5).max(MARGIN);
    let vertical_gutter = ((height - NODE_HEIGHT) * 0.5).max(MARGIN);
    let row_stack_height =
        NODE_HEIGHT * maximum_rows as f32 + ROW_GAP * maximum_rows.saturating_sub(1) as f32;
    let canvas_width = horizontal_gutter * 2.0
        + NODE_WIDTH * layer_count as f32
        + COLUMN_GAP * layer_count.saturating_sub(1) as f32;
    let canvas_height = vertical_gutter * 2.0 + row_stack_height + feedback_lane;
    let mut nodes = BTreeMap::new();
    for (layer, members) in layer_members.iter().enumerate() {
        let used_height =
            NODE_HEIGHT * members.len() as f32 + ROW_GAP * members.len().saturating_sub(1) as f32;
        let start_y = vertical_gutter + (row_stack_height - used_height) * 0.5;
        for (row, id) in members.iter().enumerate() {
            nodes.insert(
                id.clone(),
                VisualRect {
                    x: horizontal_gutter + layer as f32 * (NODE_WIDTH + COLUMN_GAP),
                    y: start_y + row as f32 * (NODE_HEIGHT + ROW_GAP),
                    width: NODE_WIDTH,
                    height: NODE_HEIGHT,
                },
            );
        }
    }

    let node_order: BTreeMap<_, _> = graph_traversal(graph)
        .into_iter()
        .enumerate()
        .map(|(index, (id, _))| (id, index))
        .collect();
    let edges = graph
        .edges
        .iter()
        .filter_map(|edge| {
            let from = nodes.get(&edge.from)?;
            let to = nodes.get(&edge.to)?;
            let (start, end, control_a, control_b, label_bounds) =
                graph_edge_geometry(from, to, graph.direction);
            Some(GraphEdgePath {
                id: edge.id.clone(),
                from: edge.from.clone(),
                to: edge.to.clone(),
                label: edge.label.clone(),
                start,
                control_a,
                control_b,
                end,
                label_bounds,
            })
        })
        .collect();
    GraphLayout {
        nodes,
        node_order,
        edges,
        canvas_width,
        canvas_height,
    }
}

fn graph_edge_geometry(
    from: &VisualRect,
    to: &VisualRect,
    direction: GraphDirection,
) -> ((f32, f32), (f32, f32), (f32, f32), (f32, f32), VisualRect) {
    match direction {
        GraphDirection::LeftToRight if to.x > from.x => {
            let start = (from.x + from.width, from.y + from.height * 0.5);
            let end = (to.x, to.y + to.height * 0.5);
            let gap = (end.0 - start.0).max(40.0);
            let bend = (gap * 0.48).max(34.0);
            let label_width = (gap - 34.0).clamp(64.0, 116.0);
            let curve_y = (start.1 + end.1) * 0.5;
            (
                start,
                end,
                (start.0 + bend, start.1),
                (end.0 - bend, end.1),
                VisualRect {
                    // The label owns the whitespace between ranks instead of sitting against the
                    // destination node, which keeps it readable on dense branches.
                    x: start.0 + (gap - label_width) * 0.5,
                    y: curve_y - 16.0,
                    width: label_width,
                    height: 32.0,
                },
            )
        }
        GraphDirection::TopToBottom if to.y > from.y => {
            let start = (from.x + from.width * 0.5, from.y + from.height);
            let end = (to.x + to.width * 0.5, to.y);
            let bend = ((end.1 - start.1) * 0.52).max(22.0);
            let label_height = ((end.1 - start.1).min(64.0) - 8.0).max(20.0);
            (
                start,
                end,
                (start.0, start.1 + bend),
                (end.0, end.1 - bend),
                VisualRect {
                    x: (start.0 + end.0) * 0.5 + 9.0,
                    y: end.1 - label_height - 4.0,
                    width: 118.0,
                    height: label_height,
                },
            )
        }
        _ => {
            let start = (from.x + from.width * 0.5, from.y + from.height);
            let end = (to.x + to.width * 0.5, to.y + to.height);
            let loop_y = start.1.max(end.1) + 62.0;
            (
                start,
                end,
                (start.0, loop_y),
                (end.0, loop_y),
                VisualRect {
                    x: (start.0 + end.0) * 0.5 - 52.0,
                    y: loop_y - 13.0,
                    width: 104.0,
                    height: 26.0,
                },
            )
        }
    }
}

fn partial_cubic(edge: &GraphEdgePath, t: f32) -> [(f32, f32); 4] {
    let mix = |a: (f32, f32), b: (f32, f32)| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    let q0 = mix(edge.start, edge.control_a);
    let q1 = mix(edge.control_a, edge.control_b);
    let q2 = mix(edge.control_b, edge.end);
    let r0 = mix(q0, q1);
    let r1 = mix(q1, q2);
    [edge.start, q0, r0, mix(r0, r1)]
}

fn paint_graph_edge(
    window: &mut Window,
    bounds: Bounds<gpui::Pixels>,
    edge: &GraphEdgePath,
    progress: f32,
    color: u32,
    width: f32,
) {
    if progress <= 0.001 {
        return;
    }
    let [start, control_a, control_b, end] = partial_cubic(edge, progress);
    let translate =
        |value: (f32, f32)| point(bounds.origin.x + px(value.0), bounds.origin.y + px(value.1));
    let mut builder = PathBuilder::stroke(px(width));
    builder.move_to(translate(start));
    builder.cubic_bezier_to(translate(end), translate(control_a), translate(control_b));
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgb(color));
    }
    if progress > 0.94 {
        let vector = (end.0 - control_b.0, end.1 - control_b.1);
        let length = (vector.0 * vector.0 + vector.1 * vector.1).sqrt().max(0.01);
        let direction = (vector.0 / length, vector.1 / length);
        let normal = (-direction.1, direction.0);
        let base = (end.0 - direction.0 * 10.0, end.1 - direction.1 * 10.0);
        let mut arrow = PathBuilder::fill();
        arrow.move_to(translate(end));
        arrow.line_to(translate((
            base.0 + normal.0 * 4.5,
            base.1 + normal.1 * 4.5,
        )));
        arrow.line_to(translate((
            base.0 - normal.0 * 4.5,
            base.1 - normal.1 * 4.5,
        )));
        arrow.line_to(translate(end));
        if let Ok(path) = arrow.build() {
            window.paint_path(path, rgb(color));
        }
    }
}

fn graph_node_color(role: GraphNodeRole) -> u32 {
    match role {
        GraphNodeRole::Source => 0xd4d4d8,
        GraphNodeRole::Process => 0xb8b8bd,
        GraphNodeRole::Decision => 0xf1f1f2,
        GraphNodeRole::Evidence => 0xa1a1a8,
        GraphNodeRole::Outcome => 0xe4e4e7,
        GraphNodeRole::Constraint => 0x86868e,
        GraphNodeRole::Concept => SUBTLE,
    }
}

fn graph_role_label(role: GraphNodeRole) -> &'static str {
    match role {
        GraphNodeRole::Source => "SOURCE",
        GraphNodeRole::Process => "PROCESS",
        GraphNodeRole::Decision => "DECISION",
        GraphNodeRole::Evidence => "EVIDENCE",
        GraphNodeRole::Outcome => "OUTCOME",
        GraphNodeRole::Constraint => "CONSTRAINT",
        GraphNodeRole::Concept => "CONCEPT",
    }
}

fn welcome_slide() -> DeckSlide {
    DeckSlide {
        spec: PresentationSlide {
            id: "welcome".into(),
            axis: SlideAxis::Root,
            title: "Ideas, explained spatially".into(),
            eyebrow: Some("SHELL".into()),
            composition: SlideComposition::Bento,
            narration: NarrationBeat {
                id: "welcome-narration".into(),
                text: String::new(),
                style: Default::default(),
                style_degree: 1.0,
                focus: Vec::new(),
                anchors: Vec::new(),
            },
            blocks: vec![
                SlideBlock {
                    id: "welcome-title".into(),
                    kind: SlideBlockKind::DisplayText,
                    title: Some("WELCOME".into()),
                    content: "Ideas become clearer when they have somewhere to live.".into(),
                    uri: None,
                    language: None,
                    chart: None,
                    tree: None,
                    columns: 4,
                    rows: 5,
                    emphasis: BlockEmphasis::Strong,
                },
                SlideBlock {
                    id: "welcome-ask".into(),
                    kind: SlideBlockKind::Text,
                    title: Some("ASK ANYTHING".into()),
                    content: "Start with a question. The answer becomes a narrated visual map."
                        .into(),
                    uri: None,
                    language: None,
                    chart: None,
                    tree: None,
                    columns: 3,
                    rows: 5,
                    emphasis: BlockEmphasis::Normal,
                },
                SlideBlock {
                    id: "welcome-voice".into(),
                    kind: SlideBlockKind::Statistic,
                    title: Some("VOICE".into()),
                    content: "Listen".into(),
                    uri: None,
                    language: None,
                    chart: None,
                    tree: None,
                    columns: 2,
                    rows: 5,
                    emphasis: BlockEmphasis::Normal,
                },
                SlideBlock {
                    id: "welcome-time".into(),
                    kind: SlideBlockKind::Callout,
                    title: Some("TIME →".into()),
                    content: "Move across the presentation as the argument develops.".into(),
                    uri: None,
                    language: None,
                    chart: None,
                    tree: None,
                    columns: 5,
                    rows: 4,
                    emphasis: BlockEmphasis::Normal,
                },
                SlideBlock {
                    id: "welcome-detail".into(),
                    kind: SlideBlockKind::Text,
                    title: Some("DETAIL ↓".into()),
                    content: "Drop into evidence, examples, and another view of the same idea."
                        .into(),
                    uri: None,
                    language: None,
                    chart: None,
                    tree: None,
                    columns: 4,
                    rows: 4,
                    emphasis: BlockEmphasis::Quiet,
                },
            ],
            graph: None,
            code: None,
        },
        x: 0,
        y: 0,
        ordinal: 0,
        revealed: true,
    }
}

fn spawn_live_agent(
    prompt: String,
    request_id: String,
    recorder: Option<Arc<Mutex<PresentationRecorder>>>,
) -> Receiver<Vec<EventEnvelope>> {
    let (sender, receiver) = async_channel::unbounded();
    thread::Builder::new()
        .name("foyer-shell-slide-harness".into())
        .spawn(move || {
            let working_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let state_dir = working_dir.join(".foyer-shell/pi-live");
            if let Err(error) = fs::create_dir_all(&state_dir) {
                report_live_agent_failure(
                    &sender,
                    recorder.as_ref(),
                    &request_id,
                    &prompt,
                    format!("Could not create the presentation-planner state directory: {error}"),
                );
                return;
            }
            let config = PiConfig::for_workspace(&working_dir);
            let mut harness = match PiHarness::spawn(&config, &working_dir, &state_dir) {
                Ok(harness) => harness,
                Err(error) => {
                    eprintln!("failed to start the OpenRouter slide planner: {error}");
                    report_live_agent_failure(
                        &sender,
                        recorder.as_ref(),
                        &request_id,
                        &prompt,
                        format!("Could not start the presentation sidecar: {error}"),
                    );
                    return;
                }
            };
            if let Err(error) = harness.prompt(&request_id, &prompt) {
                eprintln!("failed to prompt the OpenRouter slide planner: {error}");
                report_live_agent_failure(
                    &sender,
                    recorder.as_ref(),
                    &request_id,
                    &prompt,
                    format!("Could not submit the presentation request: {error}"),
                );
                return;
            }
            while let Ok(message) = harness.events().recv() {
                let Ok(message) = message else {
                    continue;
                };
                match message {
                    SidecarMessage::Events { events, .. } => {
                        if let Some(recorder) = recorder.as_ref()
                            && let Ok(mut recorder) = recorder.lock()
                            && let Err(error) = recorder.record_events(&events)
                        {
                            eprintln!("failed to record presentation events: {error}");
                        }
                        if !events.is_empty() && sender.send_blocking(events).is_err() {
                            return;
                        }
                    }
                    SidecarMessage::PresentationMaterials { evidence, .. } => {
                        if let Some(recorder) = recorder.as_ref()
                            && let Ok(recorder) = recorder.lock()
                            && let Err(error) = recorder.record_evidence(&evidence)
                        {
                            eprintln!("failed to retain presentation evidence: {error}");
                        }
                    }
                    SidecarMessage::Settled { .. } => {
                        if let Some(recorder) = recorder.as_ref()
                            && let Ok(mut recorder) = recorder.lock()
                            && let Err(error) = recorder.finish_authoring()
                        {
                            eprintln!("failed to finish presentation presentation: {error}");
                        }
                        return;
                    }
                    SidecarMessage::Error {
                        component,
                        message,
                        fatal,
                        ..
                    } => {
                        eprintln!(
                            "{} sidecar error: {message}",
                            component.as_deref().unwrap_or("OpenRouter")
                        );
                        if fatal {
                            return;
                        }
                    }
                    SidecarMessage::Ready { .. } => {}
                }
            }
        })
        .expect("failed to start slide harness thread");
    receiver
}

fn report_live_agent_failure(
    sender: &Sender<Vec<EventEnvelope>>,
    recorder: Option<&Arc<Mutex<PresentationRecorder>>>,
    request_id: &str,
    prompt: &str,
    message: String,
) {
    let events = vec![
        EventEnvelope::new(
            0,
            0,
            WorkEvent::SessionStarted {
                session_id: request_id.to_string(),
                goal: prompt.to_string(),
            },
        ),
        EventEnvelope::new(
            1,
            0,
            WorkEvent::SessionCompleted {
                status: CompletionStatus::Failed,
                summary: message.clone(),
                answer_markdown: message,
                artifact_ids: Vec::new(),
            },
        ),
    ];
    if let Some(recorder) = recorder
        && let Ok(mut recorder) = recorder.lock()
        && let Err(error) = recorder.record_events(&events)
    {
        eprintln!("failed to record presentation startup failure: {error}");
    }
    let _ = sender.send_blocking(events);
}

pub fn init(cx: &mut App) {
    foyer_shell_presentation_ui::init(cx);
    cx.set_http_client(Arc::new(FoyerShellHttpClient::new()));
}

#[allow(dead_code)]
fn main() {
    let arguments: Vec<_> = env::args().skip(1).collect();
    let live_prompt = arguments
        .first()
        .filter(|argument| argument.as_str() == "--live")
        .map(|_| {
            if arguments.len() > 1 {
                arguments[1..].join(" ")
            } else {
                "Explain this project clearly.".into()
            }
        });
    gpui_platform::application().run(move |cx: &mut App| {
        init(cx);
        let bounds = Bounds::centered(None, size(px(1600.0), px(900.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    focus: true,
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| PresentationView::new(live_prompt.clone(), window, cx));
                    let prompt_input = view.read(cx).prompt_input.clone();
                    prompt_input.update(cx, |input, cx| {
                        input.focus(window, cx);
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("failed to open Foyer Shell window");
        window
            .update(cx, |_, _, _| {})
            .expect("failed to initialize window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, columns: u8, rows: u8) -> SlideBlock {
        SlideBlock {
            id: id.into(),
            kind: SlideBlockKind::Text,
            title: None,
            content: id.into(),
            uri: None,
            language: None,
            chart: None,
            tree: None,
            columns,
            rows,
            emphasis: BlockEmphasis::Normal,
        }
    }

    #[test]
    fn bento_layout_is_bounded_and_non_overlapping() {
        let blocks = [
            block("hero", 6, 3),
            block("stat", 3, 3),
            block("left", 3, 2),
            block("middle", 3, 2),
            block("right", 3, 2),
            block("footer", 9, 2),
        ];
        let positions = pack_bento(&blocks, 1_200.0, 620.0, "test-slide");
        assert_eq!(positions.len(), blocks.len());
        for rect in positions.values() {
            assert!(rect.x >= 0.0 && rect.y >= 0.0);
            assert!(rect.x + rect.width <= 1_200.1);
            assert!(rect.y + rect.height <= 620.1);
        }
        let rectangles: Vec<_> = positions.values().collect();
        for (index, a) in rectangles.iter().enumerate() {
            for b in rectangles.iter().skip(index + 1) {
                let overlaps = a.x < b.x + b.width
                    && a.x + a.width > b.x
                    && a.y < b.y + b.height
                    && a.y + a.height > b.y;
                assert!(!overlaps, "bento rectangles overlap: {a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn block_reveals_are_staggered_but_finish_together() {
        assert!(block_reveal_progress(0.2, 0) > block_reveal_progress(0.2, 2));
        assert_eq!(block_reveal_progress(1.0, 0), 1.0);
        assert_eq!(block_reveal_progress(1.0, 5), 1.0);
    }

    #[test]
    fn slide_sanitization_rebuilds_a_complete_focus_sequence() {
        let mut slide = welcome_slide().spec;
        slide.narration.focus = vec!["welcome-title".into(), "other-slide".into()];
        slide.narration.anchors = vec![foyer_shell_protocol::NarrationAnchor {
            phrase: "missing".into(),
            at_char: None,
            cue: CueAction::Focus {
                ids: vec!["other-slide".into()],
            },
        }];
        sanitize_slide(&mut slide);
        assert_eq!(slide.narration.focus, ["welcome-title"]);
        assert_eq!(slide.narration.anchors.len(), slide.blocks.len() - 1);
        let focused_ids: Vec<_> = slide
            .narration
            .focus
            .iter()
            .chain(slide.narration.anchors.iter().flat_map(|anchor| {
                let CueAction::Focus { ids } = &anchor.cue else {
                    panic!("director focus sequence must use focus cues")
                };
                ids.iter()
            }))
            .cloned()
            .collect();
        assert_eq!(
            focused_ids,
            slide
                .blocks
                .iter()
                .map(|block| block.id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn slide_sanitization_preserves_exact_segment_offsets() {
        let mut slide = welcome_slide().spec;
        slide.narration.focus = vec![slide.blocks[0].id.clone()];
        slide.narration.anchors = slide
            .blocks
            .iter()
            .skip(1)
            .enumerate()
            .map(|(index, block)| foyer_shell_protocol::NarrationAnchor {
                phrase: String::new(),
                at_char: Some((index as u32 + 1) * 12),
                cue: CueAction::Focus {
                    ids: vec![block.id.clone()],
                },
            })
            .collect();
        sanitize_slide(&mut slide);
        assert_eq!(
            slide
                .narration
                .anchors
                .iter()
                .map(|anchor| anchor.at_char)
                .collect::<Vec<_>>(),
            [Some(12), Some(24), Some(36), Some(48)]
        );
    }

    #[test]
    fn chart_is_one_focusable_bento_block_not_a_mark_sequence() {
        let mut slide = welcome_slide().spec;
        slide.blocks = vec![SlideBlock {
            id: "trend".into(),
            kind: SlideBlockKind::Chart,
            title: Some("Latency".into()),
            content: String::new(),
            uri: None,
            language: None,
            chart: Some(foyer_shell_protocol::SlideChart {
                kind: foyer_shell_protocol::ChartKind::Line,
                categories: vec!["Jan".into(), "Feb".into()],
                series: vec![foyer_shell_protocol::ChartSeries {
                    label: "Latency".into(),
                    values: vec![90.0, 42.0],
                }],
                candles: Vec::new(),
            }),
            tree: None,
            columns: 6,
            rows: 4,
            emphasis: BlockEmphasis::Strong,
        }];
        slide.narration.focus = vec!["trend".into()];
        slide.narration.anchors = vec![foyer_shell_protocol::NarrationAnchor {
            phrase: "point".into(),
            at_char: Some(4),
            cue: CueAction::Focus {
                ids: vec!["trend-series-1".into()],
            },
        }];
        sanitize_slide(&mut slide);
        assert_eq!(slide.narration.focus, ["trend"]);
        assert!(slide.narration.anchors.is_empty());
    }

    #[test]
    fn code_walkthrough_focus_follows_authored_step_order() {
        let mut slide = welcome_slide().spec;
        slide.composition = SlideComposition::Code;
        slide.code = Some(SlideCode {
            language: "rust".into(),
            content: "fn main() {\n    run();\n}".into(),
            files: Vec::new(),
            show_explorer: false,
            steps: vec![
                foyer_shell_protocol::CodeStep {
                    id: "entry".into(),
                    label: Some("Entry".into()),
                    file_id: None,
                    start_line: 1,
                    end_line: 1,
                },
                foyer_shell_protocol::CodeStep {
                    id: "call".into(),
                    label: Some("Call".into()),
                    file_id: None,
                    start_line: 2,
                    end_line: 2,
                },
            ],
        });
        slide.narration.focus = vec!["entry".into()];
        slide.narration.anchors = vec![foyer_shell_protocol::NarrationAnchor {
            phrase: String::new(),
            at_char: Some(10),
            cue: CueAction::Focus {
                ids: vec!["call".into()],
            },
        }];
        sanitize_slide(&mut slide);
        assert_eq!(slide.composition, SlideComposition::Code);
        assert!(slide.blocks.is_empty());
        assert_eq!(slide.narration.focus, ["entry"]);
        assert_eq!(
            slide.narration.anchors[0].cue,
            CueAction::Focus {
                ids: vec!["call".into()]
            }
        );
    }

    #[test]
    fn typography_shrinks_before_enabling_scroll() {
        let mut text = block("copy", 3, 3);
        text.content = "A compact presentation".into();
        let rect = VisualRect {
            x: 0.0,
            y: 0.0,
            width: 360.0,
            height: 180.0,
        };
        let short_fit = fit_typography(&text, rect);
        assert!(!short_fit.scroll);

        text.content = "A detailed presentation with enough words to wrap naturally across several lines while remaining readable in the presentation surface.".into();
        let wrapped_fit = fit_typography(&text, rect);
        assert!(wrapped_fit.font_size <= short_fit.font_size);
        assert!(!wrapped_fit.scroll);

        text.content = std::iter::repeat_n(
            "This deliberately long paragraph exercises the readable-size floor and internal overflow.",
            24,
        )
        .collect::<Vec<_>>()
        .join(" ");
        let overflow_fit = fit_typography(&text, rect);
        assert_eq!(overflow_fit.font_size, 14.0);
        assert!(overflow_fit.scroll);
    }

    #[test]
    fn wrapping_estimate_honors_explicit_lines_and_long_tokens() {
        assert!(estimate_wrapped_lines("one\ntwo\nthree", 500.0, 16.0, false) >= 3);
        assert!(estimate_wrapped_lines(&"x".repeat(80), 120.0, 16.0, true) > 1);
    }

    fn graph_node(id: &str, role: GraphNodeRole) -> foyer_shell_protocol::GraphNode {
        foyer_shell_protocol::GraphNode {
            id: id.into(),
            label: id.into(),
            detail: None,
            role,
        }
    }

    #[test]
    fn graph_layout_is_canvas_bounded_and_non_overlapping() {
        let graph = SlideGraph {
            direction: GraphDirection::LeftToRight,
            nodes: vec![
                graph_node("source", GraphNodeRole::Source),
                graph_node("left", GraphNodeRole::Process),
                graph_node("right", GraphNodeRole::Process),
                graph_node("outcome", GraphNodeRole::Outcome),
            ],
            edges: vec![
                foyer_shell_protocol::GraphEdge {
                    id: "source-left".into(),
                    from: "source".into(),
                    to: "left".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
                foyer_shell_protocol::GraphEdge {
                    id: "source-right".into(),
                    from: "source".into(),
                    to: "right".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
                foyer_shell_protocol::GraphEdge {
                    id: "left-outcome".into(),
                    from: "left".into(),
                    to: "outcome".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
            ],
        };
        let layout = layout_graph(&graph, 1_200.0, 620.0);
        assert_eq!(layout.nodes.len(), 4);
        assert_eq!(layout.edges.len(), 3);
        let rectangles: Vec<_> = layout.nodes.values().collect();
        for rect in &rectangles {
            assert!(rect.x >= 0.0 && rect.y >= 0.0);
            assert!(rect.x + rect.width <= layout.canvas_width);
            assert!(rect.y + rect.height <= layout.canvas_height);
        }
        for (index, a) in rectangles.iter().enumerate() {
            for b in rectangles.iter().skip(index + 1) {
                assert!(
                    !(a.x < b.x + b.width
                        && a.x + a.width > b.x
                        && a.y < b.y + b.height
                        && a.y + a.height > b.y)
                );
            }
        }
        for edge in &layout.edges {
            for node in layout.nodes.values() {
                let label = edge.label_bounds;
                let overlaps = label.x < node.x + node.width
                    && label.x + label.width > node.x
                    && label.y < node.y + node.height
                    && label.y + label.height > node.y;
                assert!(
                    !overlaps,
                    "edge label overlaps a node: {label:?} and {node:?}"
                );
            }
        }
    }

    #[test]
    fn branching_graph_traversal_visits_every_node_in_topological_order() {
        let graph = SlideGraph {
            direction: GraphDirection::LeftToRight,
            nodes: vec![
                graph_node("root", GraphNodeRole::Source),
                graph_node("left", GraphNodeRole::Process),
                graph_node("right", GraphNodeRole::Process),
                graph_node("left-leaf", GraphNodeRole::Outcome),
                graph_node("right-leaf", GraphNodeRole::Outcome),
            ],
            edges: vec![
                foyer_shell_protocol::GraphEdge {
                    id: "root-left".into(),
                    from: "root".into(),
                    to: "left".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
                foyer_shell_protocol::GraphEdge {
                    id: "root-right".into(),
                    from: "root".into(),
                    to: "right".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
                foyer_shell_protocol::GraphEdge {
                    id: "left-leaf-edge".into(),
                    from: "left".into(),
                    to: "left-leaf".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
                foyer_shell_protocol::GraphEdge {
                    id: "right-leaf-edge".into(),
                    from: "right".into(),
                    to: "right-leaf".into(),
                    label: None,
                    relation: foyer_shell_protocol::GraphRelation::FlowsTo,
                },
            ],
        };
        let traversal = graph_traversal(&graph);
        assert_eq!(
            traversal
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["root", "left", "right", "left-leaf", "right-leaf"]
        );
        assert_eq!(traversal.len(), graph.nodes.len());
    }

    #[test]
    fn long_graphs_create_a_pannable_world_instead_of_cramping_nodes() {
        let nodes: Vec<_> = (0..8)
            .map(|index| graph_node(&format!("node-{index}"), GraphNodeRole::Process))
            .collect();
        let edges = (0..7)
            .map(|index| foyer_shell_protocol::GraphEdge {
                id: format!("edge-{index}"),
                from: format!("node-{index}"),
                to: format!("node-{}", index + 1),
                label: Some("continues".into()),
                relation: foyer_shell_protocol::GraphRelation::FlowsTo,
            })
            .collect();
        let layout = layout_graph(
            &SlideGraph {
                direction: GraphDirection::LeftToRight,
                nodes,
                edges,
            },
            1_000.0,
            520.0,
        );
        assert!(layout.canvas_width > 1_000.0);
        assert!(layout.nodes.values().all(|node| node.width >= 220.0));
    }

    #[test]
    fn graph_sanitization_drops_invalid_edges_and_blocks() {
        let mut slide = welcome_slide().spec;
        slide.composition = SlideComposition::Graph;
        slide.graph = Some(SlideGraph {
            direction: GraphDirection::TopToBottom,
            nodes: vec![
                graph_node("a", GraphNodeRole::Source),
                graph_node("b", GraphNodeRole::Outcome),
            ],
            edges: vec![foyer_shell_protocol::GraphEdge {
                id: "bad".into(),
                from: "a".into(),
                to: "missing".into(),
                label: None,
                relation: foyer_shell_protocol::GraphRelation::Related,
            }],
        });
        sanitize_slide(&mut slide);
        assert!(slide.blocks.is_empty());
        assert!(slide.graph.as_ref().unwrap().edges.is_empty());
        assert_eq!(
            slide.graph.as_ref().unwrap().direction,
            GraphDirection::LeftToRight
        );
    }

    #[test]
    fn non_graph_slides_are_always_bento() {
        for composition in [
            SlideComposition::Hero,
            SlideComposition::Split,
            SlideComposition::Stack,
            SlideComposition::Comparison,
            SlideComposition::Media,
        ] {
            let mut slide = welcome_slide().spec;
            slide.composition = composition;
            slide.graph = None;
            sanitize_slide(&mut slide);
            assert_eq!(slide.composition, SlideComposition::Bento);
        }
    }
}
