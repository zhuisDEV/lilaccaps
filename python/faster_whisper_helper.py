# /// script
# requires-python = ">=3.10,<3.14"
# dependencies = [
#   "faster-whisper==1.2.1",
# ]
# ///

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

from faster_whisper import WhisperModel  # ty: ignore[unresolved-import]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--download-root", type=Path, required=True)
    parser.add_argument("--language")
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.check:
        print(json.dumps({"status": "ok", "faster_whisper": "1.2.1"}))
        return

    args.download_root.mkdir(parents=True, exist_ok=True)
    model = WhisperModel(
        args.model,
        device="auto",
        compute_type="int8",
        download_root=str(args.download_root),
    )
    segments, info = model.transcribe(
        str(args.audio),
        language=args.language,
        beam_size=5,
        word_timestamps=True,
        vad_filter=True,
        vad_parameters={"min_silence_duration_ms": 500},
    )

    output_segments: list[dict[str, Any]] = []
    output_words: list[dict[str, Any]] = []
    for segment in segments:
        words: list[dict[str, Any]] = []
        for word in segment.words or []:
            if word.start is None or word.end is None or not word.word:
                continue
            item = {
                "text": word.word,
                "start": word.start,
                "end": word.end,
                "confidence": word.probability,
            }
            words.append(item)
            output_words.append(item)
        output_segments.append(
            {
                "text": segment.text,
                "start": segment.start,
                "end": segment.end,
                "words": words,
            }
        )

    print(
        json.dumps(
            {
                "language": info.language,
                "language_probability": info.language_probability,
                "duration": info.duration,
                "duration_after_vad": info.duration_after_vad,
                "segments": output_segments,
                "words": output_words,
            },
            ensure_ascii=False,
        )
    )


if __name__ == "__main__":
    main()
