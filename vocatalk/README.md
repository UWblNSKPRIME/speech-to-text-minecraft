# vocatalk

Voice input for the Minecraft chat: speak Russian, get a punctuated transcript — today as a
recognition CLI and a live two-channel audio stream, tomorrow wired into a Fabric mod.

## Status

Stage 2 of 5, two modes:

- `--file` — batch: transcribe a WAV file (stage 1)
- `--live` — live: microphone and all system audio recognized in parallel, with streaming
  drafts and finals (stage 2)

The Minecraft integration (mod, TCP, push-to-talk) is roadmap — see below.

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

Exactly one of `--file` / `--live` must be given. A run looks like this (abridged, real
output — the mic was silent while the system played speech):

```
models: ~/.gigastt/models
vad: ~/.gigastt/models/vad
head: e2e_rnnt (INT8), Silero VAD attached
[me     ] microphone: Microphone (USB Advanced Audio Device) (48000 Hz, 2 ch)
live: listening on both channels (Ctrl+C to stop)
[discord] loopback: Speakers (USB Advanced Audio Device) (48000 Hz, 2 ch)
[discord] draft: не залученный сам корестователь
[discord] FINAL: не залученный сам корестователь
[discord] draft: час на сборку объектов заново.
[discord] FINAL: час на сборку объектов заново.
^C
shutting down...
[me     ] partials: 0, finals: 0
TTFP[me]: n/a (no speech detected)
[discord] partials: 4, finals: 2
TTFP[discord]: 0.91 s
```

- `draft:` — interim hypothesis, printed as a new line on every update (never overwritten)
- `FINAL:` — the utterance is closed by VAD trailing silence; empty finals are not printed
- `TTFP[...]` — rough time-to-first-partial per channel, measured from live start to that
  channel's first draft. Includes VAD and decode-stride delays; it is a coarse metric, not a
  lab-grade latency measurement.

### Options

| Flag | Description |
|---|---|
| `--file <path>` | WAV file with Russian speech (batch mode) |
| `--live` | Live two-channel mode (exactly one of `--file` / `--live`) |
| `--list-devices` | List microphone inputs and the loopback source, then exit |
| `--input-device <index>` | Microphone to use in `--live` (default: system default input) |
| `--model-dir <path>` | Model directory (defaults to `~/.gigastt/models`) |
| `--threads <N>` | Encoder intra-op threads (default: logical CPUs / 2, clamped to 2..=10) |

### How live mode works

```
microphone (cpal) ────────────┐
                             ├─▶ raw f32 chunks ─▶ mpsc ─▶ per-channel worker:
system audio (WASAPI loopback)┘        (callbacks only enqueue)   downmix → 16 kHz mono
                                             └─▶ Engine: pool of 2 sessions, shared Silero VAD
                                                  └─▶ draft / FINAL lines, stats, TTFP
```

- One `Engine`, `pool_size = 2` — a parallel recognition session per channel; a single shared
  Silero VAD does endpointing for both (a `FINAL` comes after ~0.5 s of trailing silence)
- Audio callbacks never do inference: they only push chunks into channels; a dedicated worker
  thread per channel resamples (gigastt-core's polyphase resampler), runs the engine and prints
- On Ctrl+C both streams stop, in-flight audio is drained, and each channel prints its
  partial/final counts and TTFP

### Live caveats

- The loopback channel hears **everything the system plays** — Discord, but also the game,
  music and other apps. There is no per-app filtering yet; push-to-talk gating for the
  microphone comes in stage 4
- No TCP output, hotkeys or Minecraft wiring yet (stages 3–4)
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

The full pipeline the project is heading towards:

```
microphone (PTT) ─┐
                  ├─▶ Silero VAD ─▶ GigaAM v3 ─▶ TCP 127.0.0.1 ─▶ Fabric mod ─▶ Minecraft chat
Discord loopback ─┘     (stage 2 ✓)    (stage 1 ✓)   (stage 4)        (stage 3)
```

## Roadmap

- [x] Stage 1 — recognition engine (`gigastt-core`, `e2e_rnnt`, CLI on a WAV file)
- [x] Stage 2 — live audio: microphone (cpal) + WASAPI loopback, Silero VAD endpointing,
      streaming partial/final transcripts, per-channel TTFP
- [ ] Stage 3 — Fabric mod for Minecraft: socket listener, chat overlay, auto-send with guards
      (chat window / inventory open → clipboard instead)
- [ ] Stage 4 — integration: merge engine + mod, sentence endpointing, push-to-talk hotkey,
      TCP output
- [ ] Stage 5 — documentation and tuning

## Future: CUDA

The current build is CPU-only on purpose (simplest to build and debug). The reference machine has
an NVIDIA GTX 1650, and gigastt-core ships an optional CUDA execution provider behind a Cargo
feature — switching later should be a feature flag plus a CUDA-enabled ONNX Runtime build, with
no changes to the recognition code. Not scheduled yet.

## License

MIT. See [LICENSE](LICENSE).
