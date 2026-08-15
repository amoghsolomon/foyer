#!/usr/bin/env python3
"""Private framed Chatterbox Nano worker for foyer-shell-tts."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import sys
import time
import traceback

import numpy as np
import torch
from chatterbox.tts_turbo import ChatterboxTurboTTS


READY_PREFIX = "FOYER_SHELL_TTS_READY "
RESPONSE_PREFIX = "FOYER_SHELL_TTS_RESPONSE "


def emit(prefix: str, payload: dict[str, object]) -> None:
    sys.stdout.write(prefix + json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def deterministic_seed(text: str, style: str) -> int:
    digest = hashlib.sha256(f"{style}\0{text}".encode("utf-8")).digest()
    return int.from_bytes(digest[:4], "little")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference", required=True)
    parser.add_argument("--device", default="cuda")
    parser.add_argument("--threads", type=int, default=4)
    args = parser.parse_args()

    if args.device != "cuda":
        raise RuntimeError("Chatterbox Nano CPU fallback is disabled")
    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is unavailable to Chatterbox Nano")
    torch.set_num_threads(args.threads)
    torch.set_num_interop_threads(1)

    started = time.perf_counter()
    # Third-party progress messages are diagnostics, never protocol frames.
    with contextlib.redirect_stdout(sys.stderr):
        model = ChatterboxTurboTTS.from_pretrained(device="cuda", nano=True)
        model.prepare_conditionals(args.reference)
    emit(
        READY_PREFIX,
        {
            "sample_rate": model.sr,
            "load_ms": round((time.perf_counter() - started) * 1000),
        },
    )

    for raw_line in sys.stdin:
        try:
            request = json.loads(raw_line)
            text = request["text"]
            style = request["style"]
            if not isinstance(text, str) or not isinstance(style, str):
                raise ValueError("text and style must be strings")
            torch.manual_seed(deterministic_seed(text, style))
            with torch.inference_mode(), contextlib.redirect_stdout(sys.stderr):
                wav = model.generate(text)
            audio = wav.squeeze(0).detach().cpu().numpy()
            pcm = (
                np.rint(np.clip(audio, -1.0, 1.0) * np.iinfo(np.int16).max)
                .astype("<i2", copy=False)
                .tobytes()
            )
            emit(
                RESPONSE_PREFIX,
                {
                    "ok": True,
                    "sample_rate": model.sr,
                    "byte_len": len(pcm),
                },
            )
            sys.stdout.buffer.write(pcm)
            sys.stdout.buffer.flush()
        except Exception as error:
            traceback.print_exc(file=sys.stderr)
            emit(RESPONSE_PREFIX, {"ok": False, "error": str(error)})


if __name__ == "__main__":
    main()
