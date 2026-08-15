export const PROTOCOL_VERSION = 9;

export class JsonLineDecoder {
  #buffer = "";

  push(chunk) {
    this.#buffer += chunk.toString("utf8");
    const records = [];
    while (true) {
      const newline = this.#buffer.indexOf("\n");
      if (newline < 0) break;
      const line = this.#buffer.slice(0, newline).replace(/\r$/, "");
      this.#buffer = this.#buffer.slice(newline + 1);
      if (!line.trim()) continue;
      records.push(JSON.parse(line));
    }
    return records;
  }
}

export class SidecarOutput {
  #requestId = null;
  #sequence = 0;
  #startedAt = 0;
  #write;

  constructor(write) {
    this.#write = write;
  }

  begin(requestId) {
    this.#requestId = requestId;
    this.#sequence = 0;
    this.#startedAt = performance.now();
  }

  message(type, fields = {}) {
    this.#write({ protocol_version: PROTOCOL_VERSION, type, ...fields });
  }

  events(events) {
    if (!this.#requestId || events.length === 0) return [];
    const elapsedMs = Math.max(0, Math.round(performance.now() - this.#startedAt));
    const envelopes = events.map((event) => ({
      version: PROTOCOL_VERSION,
      sequence: ++this.#sequence,
      elapsed_ms: elapsedMs,
      ...event,
    }));
    this.message("events", { request_id: this.#requestId, events: envelopes });
    return envelopes;
  }
}

export function semanticUpdatesToWorkEvents(updates) {
  return updates.flatMap((update) => {
    const common = { id: update.id, label: update.label };
    switch (update.kind) {
      case "intent":
        return [{
          type: "intent",
          ...common,
          parent_id: update.parent_id ?? null,
        }];
      case "observation":
        return [{
          type: "observation",
          ...common,
          body: update.body ?? update.label,
          source: normalizeSource(update.source),
          relates_to: update.related_to ?? [],
        }];
      case "evidence":
        return [{
          type: "evidence",
          ...common,
          excerpt: update.body ?? update.label,
          source: normalizeSource(update.source) ?? {
            uri: "agent://public-observation",
            title: "Agent observation",
          },
          supports: update.related_to ?? [],
          confidence: update.confidence ?? 1,
        }];
      case "decision":
        return [{
          type: "decision",
          ...common,
          presentation: update.body ?? update.label,
          based_on: update.related_to ?? [],
          confidence: update.confidence ?? 1,
        }];
      case "uncertainty":
        return [{
          type: "uncertainty",
          id: update.id,
          question: update.body ?? update.label,
          related_to: update.related_to ?? [],
        }];
      default:
        return [];
    }
  });
}

export function presentationObjectsToWorkEvents(objects = []) {
  return objects.map((object) => ({
    type: "presentation_object",
    id: object.id,
    primitive: object.primitive,
    role: object.role,
    title: object.title,
    body: object.body ?? null,
    uri: object.uri ?? null,
    latex: object.latex ?? null,
    related_to: object.related_to ?? [],
  }));
}

export function annotationsToWorkEvents(annotations = []) {
  return annotations.map((annotation) => ({
    type: "annotation_added",
    id: annotation.id,
    target_id: annotation.target_id,
    kind: annotation.kind,
    label: annotation.label ?? null,
  }));
}

export function cameraToWorkEvent(camera) {
  if (!camera) return null;
  return {
    type: "camera_directed",
    action: camera.action,
    target_ids: camera.target_ids ?? [],
    padding: camera.padding ?? 112,
  };
}

export function narrationToWorkEvent(narration) {
  if (!narration) return null;
  return {
    type: "narration_proposed",
    beat_id: narration.id,
    text: narration.text,
    focus: narration.focus ?? [],
    anchors: (narration.anchors ?? []).map((anchor) => ({
      phrase: anchor.phrase,
      at_char: anchor.at_char ?? null,
      cue: {
        type: anchor.cue.type,
        ids: anchor.cue.ids ?? [],
      },
    })),
  };
}

export function stageBeatToWorkEvent(stage, narration) {
  if (!stage) return null;
  return {
    type: "stage_beat_planned",
    beat: {
      id: stage.id,
      narration: narration ? {
        id: narration.id,
        text: narration.text,
        style: narration.style ?? "neutral",
        style_degree: narration.style_degree ?? 1,
        focus: narration.focus ?? [],
        anchors: (narration.anchors ?? []).map((anchor) => ({
          phrase: anchor.phrase,
          at_char: anchor.at_char ?? null,
          cue: {
            type: anchor.cue.type,
            ids: anchor.cue.ids ?? [],
          },
        })),
      } : null,
      actions: stage.actions ?? [],
    },
  };
}

export function stageSequenceToWorkEvents(beats = []) {
  return beats
    .map((beat) => stageBeatToWorkEvent({
      id: beat.id,
      actions: beat.actions ?? [],
    }, beat.narration ?? null))
    .filter(Boolean);
}

export function slideToWorkEvent(slide) {
  if (!slide) return null;
  return {
    type: "slide_planned",
    slide: {
      id: slide.id,
      axis: slide.axis ?? "horizontal",
      title: slide.title,
      eyebrow: slide.eyebrow ?? null,
      composition: slide.composition ?? "bento",
      narration: {
        id: slide.narration.id,
        text: slide.narration.text,
        style: slide.narration.style ?? "neutral",
        style_degree: slide.narration.style_degree ?? 1,
        focus: slide.narration.focus ?? [],
        anchors: (slide.narration.anchors ?? []).map((anchor) => ({
          phrase: anchor.phrase,
          at_char: anchor.at_char ?? null,
          cue: {
            type: anchor.cue.type,
            ids: anchor.cue.ids ?? [],
          },
        })),
      },
      blocks: slide.blocks ?? [],
      graph: slide.graph ?? null,
      code: slide.code ?? null,
    },
  };
}

export function slideSequenceToWorkEvents(slides = []) {
  return slides.map(slideToWorkEvent).filter(Boolean);
}

export function presenceToWorkEvent(cue) {
  return {
    type: "presence_proposed",
    cue: {
      id: cue.id,
      text: cue.text,
      tone: cue.tone ?? "checking",
      speak: cue.speak ?? true,
      expires_after_ms: cue.expires_after_ms ?? 4_000,
    },
  };
}

export function extractAssistantText(messages) {
  const assistant = [...messages].reverse().find((message) => message.role === "assistant");
  if (!assistant) return "";
  if (typeof assistant.content === "string") return assistant.content.trim();
  if (!Array.isArray(assistant.content)) return "";
  return assistant.content
    .filter((part) => part?.type === "text")
    .map((part) => part.text)
    .join("\n")
    .trim();
}

export function publicToolSummary(tool, args) {
  const target = ["path", "file_path", "query", "pattern", "command"]
    .map((key) => args?.[key])
    .find((value) => typeof value === "string");
  if (!target) return `Using ${tool}`;
  return `${tool}: ${truncate(target, 72)}`;
}

export function truncate(text, length = 240) {
  const value = String(text ?? "").trim();
  const characters = [...value];
  return characters.length <= length ? value : `${characters.slice(0, length).join("")}…`;
}

function normalizeSource(source) {
  if (!source?.uri) return null;
  return {
    uri: source.uri,
    title: source.title ?? null,
    line: source.line ?? null,
    media_type: source.media_type ?? null,
  };
}
