import assert from "node:assert/strict";
import test from "node:test";

import {
  annotationsToWorkEvents,
  cameraToWorkEvent,
  JsonLineDecoder,
  narrationToWorkEvent,
  presentationObjectsToWorkEvents,
  semanticUpdatesToWorkEvents,
  SidecarOutput,
  stageBeatToWorkEvent,
  stageSequenceToWorkEvents,
  slideSequenceToWorkEvents,
  presenceToWorkEvent,
} from "../src/protocol.mjs";

test("JSONL decoder preserves partial records", () => {
  const decoder = new JsonLineDecoder();
  assert.deepEqual(decoder.push('{"type":"sta'), []);
  assert.deepEqual(decoder.push('rt"}\n{"type":"abort"}\n'), [
    { type: "start" },
    { type: "abort" },
  ]);
});

test("slide sequences preserve axes, bento spans, and narration focus", () => {
  const [event] = slideSequenceToWorkEvents([{
    id: "slide:detail",
    axis: "vertical",
    title: "The evidence underneath",
    eyebrow: "DETAIL",
    composition: "bento",
    narration: {
      id: "voice:detail",
      text: "The stable key removes the repeated scan.",
      focus: ["claim"],
      anchors: [{ phrase: "repeated scan", cue: { type: "focus", ids: ["evidence"] } }],
    },
    blocks: [
      { id: "claim", kind: "display_text", content: "Stable key", columns: 6, rows: 3 },
      { id: "evidence", kind: "text", content: "No repeated scan", columns: 3, rows: 3 },
    ],
  }]);
  assert.equal(event.type, "slide_planned");
  assert.equal(event.slide.axis, "vertical");
  assert.equal(event.slide.blocks[0].columns, 6);
  assert.equal(event.slide.narration.anchors[0].cue.ids[0], "evidence");
});

test("slide sequences preserve full-slide semantic graphs", () => {
  const graph = {
    direction: "left_to_right",
    nodes: [
      { id: "input", label: "Input", detail: null, role: "source" },
      { id: "output", label: "Output", detail: "The result", role: "outcome" },
    ],
    edges: [{
      id: "input-output",
      from: "input",
      to: "output",
      label: "becomes",
      relation: "flows_to",
    }],
  };
  const [event] = slideSequenceToWorkEvents([{
    id: "slide:graph",
    axis: "horizontal",
    title: "The flow",
    composition: "graph",
    narration: {
      id: "voice:graph",
      text: "The input becomes the output.",
      focus: ["input"],
      anchors: [{ phrase: "becomes", cue: { type: "follow_path", ids: ["input-output"] } }],
    },
    blocks: [],
    graph,
  }]);
  assert.deepEqual(event.slide.graph, graph);
  assert.equal(event.slide.narration.anchors[0].cue.type, "follow_path");
});

test("chart cards remain a single bento focus target", () => {
  const chart = {
    kind: "area",
    categories: ["Jan", "Feb", "Mar"],
    series: [{ label: "Latency", values: [90, 62, 41] }],
  };
  const [event] = slideSequenceToWorkEvents([{
    id: "slide:chart",
    axis: "horizontal",
    title: "Latency falls",
    composition: "bento",
    narration: {
      id: "voice:chart",
      text: "The trend falls steadily.",
      focus: ["latency-chart"],
      anchors: [],
    },
    blocks: [{ id: "latency-chart", kind: "chart", content: "", chart }],
  }]);
  assert.deepEqual(event.slide.blocks[0].chart, chart);
  assert.deepEqual(event.slide.narration.focus, ["latency-chart"]);
});

test("immersive code slides preserve ordered semantic line ranges", () => {
  const code = {
    language: "rust",
    content: "fn main() {\n    run();\n}",
    steps: [
      { id: "entry", label: "Entry point", start_line: 1, end_line: 1 },
      { id: "call", label: "Execute", start_line: 2, end_line: 2 },
    ],
  };
  const [event] = slideSequenceToWorkEvents([{
    id: "slide:code",
    axis: "horizontal",
    title: "Follow the call",
    composition: "code",
    narration: {
      id: "voice:code",
      text: "Enter, then run.",
      focus: ["entry"],
      anchors: [{ phrase: "then", at_char: 7, cue: { type: "focus", ids: ["call"] } }],
    },
    blocks: [],
    code,
  }]);
  assert.deepEqual(event.slide.code, code);
  assert.equal(event.slide.composition, "code");
});

test("presentation sequences preserve narrated micro-beat order", () => {
  const events = stageSequenceToWorkEvents([
    {
      id: "stage:one",
      narration: { id: "beat:one", text: "First, the worker handles the task.", focus: ["worker"] },
      actions: [{
        op: "place",
        id: "worker",
        kind: "free_text",
        content: "Worker handles the task",
        placement: "center_stage",
        entrance: "write",
      }],
    },
    {
      id: "stage:two",
      narration: { id: "beat:two", text: "Then, the overlooker checks the result.", focus: ["overlooker"] },
      actions: [{
        op: "place",
        id: "overlooker",
        kind: "free_text",
        content: "Overlooker checks the result",
        placement: "beside_focus",
        entrance: "slide",
      }],
    },
  ]);

  assert.deepEqual(events.map((event) => event.beat.id), ["stage:one", "stage:two"]);
  assert.deepEqual(events.map((event) => event.beat.narration.id), ["beat:one", "beat:two"]);
});

test("stage beats and presence cues remain distinct", () => {
  const event = stageBeatToWorkEvent({
    id: "stage:cache",
    actions: [{
      op: "place",
      id: "text:cache",
      kind: "free_text",
      content: "The cache key changes every launch.",
      placement: "center_stage",
      entrance: "write",
    }],
  }, {
    id: "beat:cache",
    text: "Here’s the changing cache key.",
    focus: ["text:cache"],
  });
  assert.equal(event.type, "stage_beat_planned");
  assert.equal(event.beat.actions[0].entrance, "write");
  assert.equal(event.beat.narration.id, "beat:cache");

  const presence = presenceToWorkEvent({
    id: "presence:one",
    text: "I’m checking that now.",
  });
  assert.equal(presence.type, "presence_proposed");
  assert.equal(presence.cue.expires_after_ms, 4_000);
  assert.equal(presence.cue.speak, true);
  assert.equal("actions" in presence.cue, false);

  const visualOnly = presenceToWorkEvent({
    id: "presence:visual",
    text: "Reading the relevant source.",
    speak: false,
  });
  assert.equal(visualOnly.cue.speak, false);
});

test("sidecar output creates ordered protocol envelopes", () => {
  const records = [];
  const output = new SidecarOutput((record) => records.push(record));
  output.begin("request-1");
  const envelopes = output.events([
    { type: "session_started", session_id: "request-1", goal: "Test" },
    { type: "intent", id: "step", label: "Inspect", parent_id: null },
  ]);
  assert.equal(envelopes[0].sequence, 1);
  assert.equal(envelopes[1].sequence, 2);
  assert.equal(records[0].type, "events");
  assert.equal(records[0].request_id, "request-1");
});

test("presentation output converts into renderer work events", () => {
  const events = semanticUpdatesToWorkEvents([{
    kind: "evidence",
    id: "evidence:one",
    label: "Cache entry",
    body: "The key is stable.",
    related_to: ["decision:cache"],
    source: { uri: "src/cache.rs", line: 12 },
  }]);
  assert.deepEqual(events[0].supports, ["decision:cache"]);
  assert.equal(events[0].source.media_type, null);

  const narration = narrationToWorkEvent({
    id: "beat:one",
    text: "This is the stable cache key.",
    focus: ["evidence:one"],
    anchors: [{
      phrase: "cache key",
      cue: { type: "emphasize", ids: ["evidence:one"] },
    }],
  });
  assert.equal(narration.type, "narration_proposed");
  assert.equal(narration.anchors[0].cue.type, "emphasize");

  const [equation] = presentationObjectsToWorkEvents([{
    id: "equation:latency",
    primitive: "equation",
    role: "evidence",
    title: "End-to-end latency",
    latex: "L = L_m + L_t + L_a",
    related_to: ["evidence:one"],
  }]);
  assert.equal(equation.type, "presentation_object");
  assert.equal(equation.latex, "L = L_m + L_t + L_a");

  const [circle] = annotationsToWorkEvents([{
    id: "annotation:key",
    target_id: "evidence:one",
    kind: "circle",
  }]);
  assert.equal(circle.target_id, "evidence:one");
  assert.deepEqual(cameraToWorkEvent({ action: "focus", target_ids: ["evidence:one"] }), {
    type: "camera_directed",
    action: "focus",
    target_ids: ["evidence:one"],
    padding: 112,
  });
});
