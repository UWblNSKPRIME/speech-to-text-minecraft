# speech-to-text-minecraft

Voice input for the Minecraft chat: Russian speech is recognized with GigaAM v3 and the
transcript is sent into Minecraft chat.

## Repository layout

- [`vocatalk/`](vocatalk/) — Rust CLI: batch WAV transcription (stage 1) and live two-channel recognition (stage 2)
- [`mod/`](mod/) — Fabric client mod for Minecraft 26.2: TCP listener, HUD overlay, chat auto-send (stage 3)
- `PLAN.md` — development plan (stages, fixed decisions, open questions), in Russian

## Status

Stages 1–3 are done: a CLI transcribes a WAV file with punctuated text and encoder thread
tuning/benchmarks, `--live` recognizes the microphone and all system audio (WASAPI
loopback) in parallel with streaming drafts/finals, Silero VAD endpointing and a per-channel
TTFP metric, and the Fabric mod receives transcripts over a local socket, shows drafts on
a HUD overlay and sends finals to chat (built and deployed; in-game smoke test pending).
Still to come: end-to-end integration with push-to-talk and TCP (stage 4), tuning (stage 5).

See [`vocatalk/README.md`](vocatalk/README.md) and [`mod/README.md`](mod/README.md) for details.

## License

MIT. See [LICENSE](vocatalk/LICENSE).
