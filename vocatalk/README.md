# vocatalk

Voice input for the Minecraft chat: speak Russian, get a punctuated transcript — today as a
recognition CLI, tomorrow as a live microphone pipeline wired into a Fabric mod.

## Status

Stage 1 of 5. Only the recognition engine is wired in: `vocatalk` transcribes a WAV file with the
GigaAM v3 `e2e_rnnt` head (punctuation, casing and inverse text normalization are built into the
model) and reports wall time and RTF. Microphone capture, VAD and the Minecraft integration are
roadmap items — see below.

## Features

- Russian speech recognition in-process via [gigastt-core](https://crates.io/crates/gigastt-core)
  (GigaAM v3, ONNX Runtime, CPU-only build) — no server, no external processes
- `e2e_rnnt` head: output already has punctuation, capitalization and digit normalization
  (e.g. `шестьдесят тысяч` → `60 000`)
- INT8 model (~250 MB) is downloaded automatically on first run into `~/.gigastt/models/`
- Wall time and RTF printed for every run

## Requirements

- Rust 1.94 or newer
- ~250 MB of disk space for the model (downloaded on first run, cached afterwards)
- CPU with AVX2 (ONNX Runtime CPU execution provider; no GPU/CUDA required)

## Usage

```sh
cargo run --release -- --file testdata/sample.wav
```

Example output:

```
60 000 тенге — сколько будет стоить?

audio: 4.00 s
recognition: 0.95 s (wall), RTF 0.238
```

Options:

| Flag | Description |
|---|---|
| `--file <path>` | WAV file with Russian speech (required) |
| `--model-dir <path>` | Model directory (defaults to `~/.gigastt/models`) |
| `--threads <N>` | Encoder intra-op threads (default: logical CPUs / 2, clamped to 2..=10) |

## CPU usage / tuning

ONNX Runtime inference runs on CPU only, and only the encoder is parallelized
(intra-op threads; inter-op stays at 1). The default thread count is
`available_parallelism() / 2`, clamped to the range `2..=10` — on the 14-core / 28-thread
development machine that resolves to 10. The point is to stay well clear of the game:
with two recognition sessions in the future live pipeline (microphone + Discord), 10 × 2 = 20
of the 28 logical threads are ever claimed, and the rest stay free for Minecraft and the system.

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

What works today:

```
WAV file ──▶ gigastt-core (GigaAM v3, e2e_rnnt, CPU) ──▶ punctuated text + timing ──▶ stdout
```

The full pipeline the project is heading towards (not implemented yet):

```
microphone (PTT) ─┐
                  ├─▶ Silero VAD ─▶ GigaAM v3 ─▶ TCP 127.0.0.1 ─▶ Fabric mod ─▶ Minecraft chat
Discord loopback ─┘     (stage 2)    (stage 1 ✓)   (stage 4)        (stage 3)
```

## Roadmap

- [x] Stage 1 — recognition engine (`gigastt-core`, `e2e_rnnt`, CLI on a WAV file)
- [ ] Stage 2 — live audio: microphone (push-to-talk) + WASAPI loopback for Discord, Silero VAD
      on both channels, streaming partial/final transcripts
- [ ] Stage 3 — Fabric mod for Minecraft: socket listener, chat overlay, auto-send with guards
      (chat window / inventory open → clipboard instead)
- [ ] Stage 4 — integration: merge engine + mod, sentence endpointing, push-to-talk hotkey
- [ ] Stage 5 — documentation and tuning

## Future: CUDA

The current build is CPU-only on purpose (simplest to build and debug). The reference machine has
an NVIDIA GTX 1650, and gigastt-core ships an optional CUDA execution provider behind a Cargo
feature — switching later should be a feature flag plus a CUDA-enabled ONNX Runtime build, with
no changes to the recognition code. Not scheduled yet.

## License

MIT. See [LICENSE](LICENSE).
