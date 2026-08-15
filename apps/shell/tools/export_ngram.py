#!/usr/bin/env python3
"""Convert a trusted NeMo NGramGPULanguageModel checkpoint for the Rust runtime.

This is an offline migration tool. Python, PyTorch, and NeMo are not used by the installed
transcription service.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path
from typing import BinaryIO

MAGIC = b"SHELLNG1"
VERSION = 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path, help="source NeMo .nemo language model")
    parser.add_argument("output", type=Path, help="destination .sng file")
    parser.add_argument("--vocab-size", type=int, default=1024)
    parser.add_argument(
        "--metadata",
        type=Path,
        help="optional JSON destination for checksums and exported dimensions",
    )
    return parser.parse_args()


def write_array(output: BinaryIO, tensor: object, dtype: str, count: int) -> None:
    array = tensor.detach().cpu().numpy().reshape(-1)[:count].astype(dtype, copy=False)
    if array.size != count:
        raise ValueError(f"expected {count} values, found {array.size}")
    output.write(array.tobytes(order="C"))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def export(args: argparse.Namespace) -> dict[str, object]:
    from nemo.collections.asr.parts.submodules.ngram_lm import NGramGPULanguageModel

    checkpoint = args.checkpoint.expanduser().resolve()
    destination = args.output.expanduser().resolve()
    if not checkpoint.is_file():
        raise FileNotFoundError(checkpoint)

    model = NGramGPULanguageModel.from_nemo(
        checkpoint,
        vocab_size=args.vocab_size,
        use_triton=False,
    )
    state = model.state_dict()
    state_count = int(model.num_states)
    # NeMo pads these three arc arrays by one vocabulary. Arc ranges never address the padding.
    arc_count = int(model.num_arcs)
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".partial")
    try:
        with temporary.open("wb") as output:
            output.write(
                struct.pack(
                    "<8sIIIII",
                    MAGIC,
                    VERSION,
                    int(model.vocab_size),
                    int(model.max_order),
                    state_count,
                    arc_count,
                )
            )
            write_array(output, state["arcs_weights"], "<f4", arc_count)
            write_array(output, state["to_states"], "<u4", arc_count)
            write_array(output, state["ilabels"], "<u4", arc_count)
            write_array(output, state["backoff_weights"], "<f4", state_count)
            write_array(output, state["backoff_to_states"], "<u4", state_count)
            write_array(output, state["start_end_arcs"], "<u4", state_count * 2)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)

    digest = sha256_file(destination)
    return {
        "format": "SHELLNG1",
        "format_version": VERSION,
        "source": str(checkpoint),
        "source_sha256": sha256_file(checkpoint),
        "output": str(destination),
        "output_sha256": digest,
        "bytes": destination.stat().st_size,
        "vocab_size": int(model.vocab_size),
        "max_order": int(model.max_order),
        "state_count": state_count,
        "arc_count": arc_count,
    }


def main() -> None:
    args = parse_args()
    metadata = export(args)
    encoded = json.dumps(metadata, indent=2, sort_keys=True) + "\n"
    if args.metadata:
        path = args.metadata.expanduser().resolve()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(encoded, encoding="utf-8")
    print(encoded, end="")


if __name__ == "__main__":
    main()
