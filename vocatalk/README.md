# vocatalk

Voice input for the Minecraft chat: speak Russian, get a punctuated transcript streamed into
the game — via a live two-channel audio pipeline, a TCP bridge and a Fabric mod.

## What it is

Two modes in one binary:

- `--file` — batch: transcribe a WAV file
- `--live` — live: microphone (F8 push-to-talk) and all system audio recognized in parallel,
  with streaming drafts and finals sent to the console and to the Fabric mod over TCP

For the project overview, quick start and troubleshooting see the
[root README](../README.md); this document covers CLI details: flags, benchmarks and the
live pipeline internals. The Fabric mod itself lives in [`../mod/`](../mod/).

## Features

- Russian speech recognition in-process via [gigastt-core](https://crates.io/crates/gigastt-core)
  (GigaAM v3, ONNX Runtime, CPU-only build) — no server, no external processes
- `e2e_rnnt` head: output already has punctuation, capitalization and digit normalization
  (e.g. `шестьдесят тысяч` → `60 000`)
- INT8 model (~250 MB) and the Silero VAD model (~2 MB) are downloaded automatically on first
  run into `~/.gigastt/models/`
- Batch mode prints wall time and RTF for every run
- Live mode: two independent channels, streaming `draft` lines and VAD-endpointed `FINAL`
  lines, per-channel TTFP metric and exit statistics
- F8 push-to-talk for the microphone channel with force-finalization on release
- TCP bridge to the Fabric mod (`127.0.0.1:47832`, JSON-lines) with auto-reconnect
- No GPU, no external services

## Requirements

- Rust 1.94 or newer
- ~250 MB of disk space for the model (downloaded on first run, cached afterwards)
- CPU with AVX2 (ONNX Runtime CPU execution provider; no GPU/CUDA required)
- Windows for `--live` (WASAPI loopback); `--file` is platform-independent

## Usage — batch

```sh
cargo run --release -- --file testdata/sample.wav
```

Example output:

```
60 000 тенге — сколько будет стоить?

audio: 4.00 s
recognition: 0.95 s (wall), RTF 0.238
```

## Usage — live

```sh
cargo run --release -- --live
```

Exactly one of `--file` / `--live` must be given. Hold **F8** to talk: microphone samples
only reach the recognition engine while the key is down. A run looks like this (abridged,
real output):

```
models: ~/.gigastt/models
vad: ~/.gigastt/models/vad
head: e2e_rnnt (INT8), Silero VAD attached (threshold 0.5, min silence 600 ms)
[bridge] waiting for the mod on 127.0.0.1:47832 (transcripts go to the console either way)
[me     ] microphone: Microphone (USB Advanced Audio Device) (48000 Hz, 2 ch)
live: listening on both channels (Ctrl+C to stop)
[discord] loopback: Speakers (USB Advanced Audio Device) (48000 Hz, 2 ch)
[bridge] connected to the mod at 127.0.0.1:47832
[ptt] held
[me     ] draft: привет мир
[me     ] FINAL: привет мир
[ptt] released
[discord] draft: не залученный сам корестователь
[discord] FINAL: не залученный сам корестователь
^C
shutting down...
[me     ] partials: 2, finals: 1
TTFP[me]: 0.64 s
[discord] partials: 4, finals: 2
TTFP[discord]: 0.91 s
```

- `draft:` — interim hypothesis, printed as a new line on every update (never overwritten)
- `FINAL:` — the utterance is closed by VAD trailing silence, or force-finalized when PTT is
  released; empty finals are not printed
- `[ptt] held` / `[ptt] released` — printed once per push-to-talk transition
- `[bridge] ...` — connection state of the TCP link to the Fabric mod, once per event
- `TTFP[...]` — rough time-to-first-partial per channel, measured from live start to that
  channel's first draft. Includes VAD and decode-stride delays; it is a coarse metric, not a
  lab-grade latency measurement.

### Push-to-talk (F8)

- The microphone channel is gated by **F8** (global hotkey, works while the game has focus):
  samples only enter the engine while the key is held; while it is up, buffered microphone
  audio is dropped and no drafts are produced for the `me` channel.
- On release the current utterance is force-finalized (`flush_state`): the leftover audio
  batch is decoded and the tail is closed immediately as a `FINAL` — the text does not wait
  for the VAD's trailing silence.
- The `discord` channel (system audio) is always on and is never gated.
- If F8 is already taken by another application, `--live` exits with an error at startup.

### Minecraft integration

Start the Minecraft instance with the [Fabric mod](../mod/) first, then run
`vocatalk --live`. What happens:

- The CLI connects to the mod at `127.0.0.1:47832` and streams every `draft`/`final` of both
  channels as JSON lines (protocol: see [`mod/README.md`](../mod/README.md)). The console
  output stays as-is — both sinks receive the same events.
- If the mod is not running (or the game is closed later), the CLI keeps working: it prints
  to the console as usual and retries the connection every 2 s, logging the transition once
  (`[bridge] waiting ...` / `[bridge] connected ...` / `[bridge] mod disconnected ...`).
  Lines queued while disconnected are dropped as stale (the console is the source of truth).
- In the game: `draft` lines update the HUD overlay, `final` lines are auto-sent to chat
  (`Discord: <text>` for the loopback channel), with guards for an open chat window /
  inventory (clipboard + notice instead).

### Options

| Flag | Description |
|---|---|
| `--file <path>` | WAV file with Russian speech (batch mode) |
| `--live` | Live two-channel mode (exactly one of `--file` / `--live`) |
| `--list-devices` | List microphone inputs and the loopback source, then exit |
| `--input-device <index>` | Microphone to use in `--live` (default: system default input) |
| `--model-dir <path>` | Model directory (defaults to `~/.gigastt/models`) |
| `--threads <N>` | Encoder intra-op threads (default: logical CPUs / 2, clamped to 2..=10) |
| `--vad-threshold <f32>` | VAD speech-probability threshold in (0, 1] (default: 0.5) |
| `--vad-min-silence-ms <u32>` | Trailing silence before the VAD ends an utterance (default: 600) |

### How live mode works

```
microphone (cpal) ──── F8 gate ─┐
                                ├─▶ raw f32 chunks ─▶ mpsc ─▶ per-channel worker:
system audio (WASAPI loopback) ─┘        (callbacks only enqueue)   downmix → 16 kHz mono
                                                 └─▶ Engine: pool of 2 sessions, shared Silero VAD
                                                      └─▶ draft / FINAL ─▶ console + TCP bridge ─▶ mod
```

- One `Engine`, `pool_size = 2` — a parallel recognition session per channel; a single shared
  Silero VAD does endpointing for both (a `FINAL` comes after `--vad-min-silence-ms` of
  trailing silence, or immediately on PTT release for the microphone)
- Audio callbacks never do inference: they only push chunks into channels; a dedicated worker
  thread per channel resamples (gigastt-core's polyphase resampler), runs the engine and emits
  drafts/finals to the console and the mod bridge
- The mod bridge is a daemon thread with a 100-line queue: workers never block on it; while
  the mod is away the queue is drained (stale) and the connection is retried every 2 s
- On Ctrl+C both streams stop, in-flight audio is drained, and each channel prints its
  partial/final counts and TTFP

### Live caveats

- The loopback channel hears **everything the system plays** — Discord, but also the game,
  music and other apps. There is no per-app filtering yet; everything heard goes to chat
  (with the `Discord: ` prefix)
- The microphone channel may occasionally report `stream error: A buffer underrun or
  overrun` — a dropped-buffer glitch from the Windows audio stack under load. The pipeline
  keeps running; the stream uses a 100 ms ring buffer to make these rare
- Both channels on CPU: at `--threads 10` a busy pair can occupy ~20 of 28 logical threads
  (see below)

## CPU usage / tuning

ONNX Runtime inference runs on CPU only, and only the encoder is parallelized
(intra-op threads; inter-op stays at 1). The default thread count is
`available_parallelism() / 2`, clamped to the range `2..=10` — on the 14-core / 28-thread
development machine that resolves to 10. The point is to stay well clear of the game:
with the two recognition sessions of the live pipeline (microphone + system audio),
10 × 2 = 20 of the 28 logical threads are ever claimed, and the rest stay free for Minecraft
and the system.

Returns diminish quickly past 6 threads — on a 4 s sample from the
[golos](https://huggingface.co/datasets/golos) test set (Xeon E5-2683 v3, `testdata/sample.wav`):

| `--threads` | Wall time | RTF |
|---|---|---|
| 1 | 4.34 s | 1.084 |
| 2 | 2.25 s | 0.562 |
| 4 | 1.28 s | 0.319 |
| 6 | 0.94 s | 0.234 |
| 8 | 0.80 s | 0.201 |
| 10 (default) | 0.74 s | 0.184 |

Going from 8 to 10 threads buys ~8% over 6 → 8; anything above 10 is not worth the CPU it
takes from the game, which is why the default is capped there.

## How it works

Both modes:

```
WAV file ────────▶ gigastt-core (GigaAM v3, e2e_rnnt, CPU) ─▶ punctuated text ─▶ stdout
mic/system audio ─▶ downmix → 16 kHz → Silero VAD endpointing ─┘
```

The end-to-end path (microphone → chat) is shown in the
[root README](../README.md#architecture); below are the live-mode internals.

## Roadmap

- [x] Stage 1 — recognition engine (`gigastt-core`, `e2e_rnnt`, CLI on a WAV file)
- [x] Stage 2 — live audio: microphone (cpal) + WASAPI loopback, Silero VAD endpointing,
      streaming partial/final transcripts, per-channel TTFP
- [x] Stage 3 — Fabric mod for Minecraft: socket listener, chat overlay, auto-send with guards
      (chat window / inventory open → clipboard instead)
- [x] Stage 4 — integration: TCP bridge to the mod, F8 push-to-talk with force-finalize on
      release, VAD threshold flags
- [x] Stage 5 — documentation: root quick start/limitations, CLI and mod references

GPU acceleration (CUDA or another ONNX Runtime execution provider) is not scheduled yet —
see [Future: CUDA](../README.md#future-cuda) in the root README.

## License

MIT. See [LICENSE](LICENSE).
