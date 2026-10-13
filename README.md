# speech-to-text-minecraft

Voice input for the Minecraft chat: Russian speech is recognized with GigaAM v3 and the
transcript is sent into Minecraft chat.

## Repository layout

- [`vocatalk/`](vocatalk/) — Rust CLI: batch WAV transcription (stage 1) and live two-channel recognition (stage 2)
- `PLAN.md` — development plan (stages, fixed decisions, open questions), in Russian

## Status

Stages 1–2 are done: a CLI transcribes a WAV file with punctuated text and encoder thread
tuning/benchmarks, and `--live` recognizes the microphone and all system audio (WASAPI
loopback) in parallel with streaming drafts/finals, Silero VAD endpointing and a per-channel
TTFP metric. Still to come: the Fabric mod (stage 3), end-to-end integration with push-to-talk
and TCP (stage 4), tuning (stage 5).

See [`vocatalk/README.md`](vocatalk/README.md) for detailed CLI documentation.

## License

MIT. See [LICENSE](vocatalk/LICENSE).
