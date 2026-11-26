#!/usr/bin/env python3
"""lm-ears: the Thor's ears.

A thin HTTP front door for LFM3-Audio-2B. The model itself is served by
llama-server on 127.0.0.1:8103; this process owns :8102, the port last-mile
talks to, and shapes what comes back:

    POST /v1/audio/transcriptions   (multipart, field "file")  -> OpenAI shape
    POST /transcribe                (raw WAV bytes)           -> OpenAI shape
    POST /v1/chat/completions       (audio understanding)     -> pass-through
    GET  /health                    -> is the model up
    GET  /v1/models                 -> the model that answers

The transcription answer is OpenAI-shaped (`text`) and carries the three
things a caller needs and the OpenAI shape does not have:

    words        the transcript's words, each with start/end
    latency_ms   the wall clock of the call, server side
    has_timestamps  whether the model gave word times (it does not)

LFM3-Audio is an ASR model: it writes words, never times. So every word
carries `start: null` and `end: null`, and `has_timestamps` is false. The
fields are there so the shape does not change when a model that does
give times comes along; nothing here invents a timestamp.
"""
from __future__ import annotations

import os
import re
import time

import requests
from fastapi import FastAPI, File, Form, Request, UploadFile
from fastapi.responses import JSONResponse, Response

# The model server behind the door. Overridable so a test can point at a
# scratch instance without editing the file.
UPSTREAM = os.environ.get("EARS_UPSTREAM", "http://127.0.0.1:8103").rstrip("/")
MODEL_NAME = os.environ.get("EARS_MODEL", "LFM3-Audio-2B")
REQUEST_TIMEOUT = float(os.environ.get("EARS_TIMEOUT", "120"))

app = FastAPI(title="lm-ears", version="1.0.0")

# A word for the "words" list: the word itself, stripped of the
# punctuation that is not part of it. "Hold" and "line." are words; the
# full stop is not.
_WORD_RE = re.compile(r"[A-Za-z0-9']+")


def _words(text: str) -> list[dict]:
    """The transcript's words, with null times.

    LFM3-Audio gives no timestamps, so every time is null. The list is
    there because the caller wants the words; the nulls are there so
    nobody mistakes them for times the model gave.
    """
    return [
        {"word": match.group(0), "start": None, "end": None}
        for match in _WORD_RE.finditer(text)
    ]


def _upstream_audio(data: bytes, filename: str, language: str | None) -> dict:
    """Ask the model server to transcribe audio bytes."""
    files = {"file": (filename, data, "audio/wav")}
    form = {"response_format": "json"}
    if language:
        form["language"] = language
    response = requests.post(
        f"{UPSTREAM}/v1/audio/transcriptions",
        files=files,
        data=form,
        timeout=REQUEST_TIMEOUT,
    )
    response.raise_for_status()
    return response.json()


def _shape(upstream: dict, latency_ms: float, language: str | None) -> dict:
    """llama.cpp's answer in the shape last-mile asked for."""
    text = upstream.get("text") or ""
    words = _words(text)
    return {
        "text": text,
        "words": words,
        "latency_ms": round(latency_ms, 2),
        "has_timestamps": False,
        "timestamps": None,
        "model": MODEL_NAME,
        "language": language,
        "usage": upstream.get("usage"),
    }


@app.get("/health")
def health() -> JSONResponse:
    """Is the model up behind the door."""
    try:
        response = requests.get(f"{UPSTREAM}/health", timeout=5)
        upstream_ok = response.status_code == 200
    except requests.RequestException:
        upstream_ok = False
    return JSONResponse(
        status_code=200 if upstream_ok else 503,
        content={
            "status": "ok" if upstream_ok else "degraded",
            "service": "lm-ears",
            "model": MODEL_NAME,
            "upstream": {"url": UPSTREAM, "reachable": upstream_ok},
        },
    )


@app.get("/v1/models")
def models() -> Response:
    """The model list, as the model server has it."""
    try:
        response = requests.get(f"{UPSTREAM}/v1/models", timeout=10)
    except requests.RequestException as exc:
        return JSONResponse(status_code=503, content={"error": str(exc)})
    return Response(
        content=response.content,
        status_code=response.status_code,
        media_type="application/json",
    )


@app.post("/v1/audio/transcriptions")
async def transcriptions(
    file: UploadFile = File(...),
    language: str | None = Form(default=None),
    prompt: str | None = Form(default=None),
) -> JSONResponse:
    """OpenAI-shaped transcription of a WAV file."""
    data = await file.read()
    started = time.perf_counter()
    try:
        upstream = await _run_upstream(data, file.filename or "audio.wav", language)
    except Exception as exc:  # noqa: BLE001 - the caller gets the reason
        return JSONResponse(status_code=502, content={"error": str(exc)})
    latency_ms = (time.perf_counter() - started) * 1000.0
    answer = _shape(upstream, latency_ms, language)
    if prompt:
        answer["prompt"] = prompt
    return JSONResponse(content=answer)


@app.post("/transcribe")
async def transcribe_raw(request: Request) -> JSONResponse:
    """Transcription of raw audio bytes: the push-to-talk shape.

    The body is the audio; the query string may carry `language`.
    """
    data = await request.body()
    if not data:
        return JSONResponse(status_code=400, content={"error": "empty body"})
    language = request.query_params.get("language")
    started = time.perf_counter()
    try:
        upstream = await _run_upstream(data, "audio.wav", language)
    except Exception as exc:  # noqa: BLE001
        return JSONResponse(status_code=502, content={"error": str(exc)})
    latency_ms = (time.perf_counter() - started) * 1000.0
    return JSONResponse(content=_shape(upstream, latency_ms, language))


@app.post("/v1/chat/completions")
async def chat_completions(request: Request) -> Response:
    """Pass-through to the model server.

    This is the audio-understanding door: a question about a recording
    goes in as an OpenAI chat message with an `input_audio` part, and the
    answer comes back as the model server's own chat completion.
    """
    body = await request.body()
    try:
        response = requests.post(
            f"{UPSTREAM}/v1/chat/completions",
            data=body,
            headers={"Content-Type": "application/json"},
            timeout=REQUEST_TIMEOUT,
        )
    except requests.RequestException as exc:
        return JSONResponse(status_code=502, content={"error": str(exc)})
    return Response(
        content=response.content,
        status_code=response.status_code,
        media_type="application/json",
    )


async def _run_upstream(data: bytes, filename: str, language: str | None) -> dict:
    """The upstream call, off the event loop."""
    import anyio

    return await anyio.to_thread.run_sync(
        lambda: _upstream_audio(data, filename, language)
    )
