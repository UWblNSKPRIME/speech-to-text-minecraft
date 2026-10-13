# speech-to-text-minecraft

Voice input for the Minecraft chat: Russian speech is recognized with GigaAM v3 and the
transcript is sent into Minecraft chat.

## Repository layout

- [`vocatalk/`](vocatalk/) — Rust recognition CLI (stage 1) and the future live microphone pipeline
- `PLAN.md` — development plan (stages, fixed decisions, open questions), in Russian

## Status

Stage 1 is done: a CLI transcribes a WAV file and prints punctuated text, with encoder thread
tuning and benchmarks. Still to come: live microphone + VAD (stage 2), the Fabric mod (stage 3),
end-to-end integration (stage 4) and tuning (stage 5).

See [`vocatalk/README.md`](vocatalk/README.md) for detailed CLI documentation.

## License

MIT. See [LICENSE](vocatalk/LICENSE).
