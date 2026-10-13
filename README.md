# speech-to-text-minecraft

Speak Russian, type into Minecraft: live speech recognition (GigaAM v3 via
[gigastt-core](https://crates.io/crates/gigastt-core), CPU-only) with a Fabric mod for
Minecraft 26.2 that streams your words into the in-game chat.

## Architecture

```
                    hold F8                      16 kHz mono        endpointing     2 parallel
microphone ────────────────┐                                          sessions
                            ├─ downmix ─ resample ── Silero VAD ── GigaAM v3 ──┬── console (drafts/FINALs)
system audio (WASAPI ───────┘                                                     └── TCP 127.0.0.1:47832
loopback, always on)                                                                  │  JSON-lines
                                                                                      ▼
                                             Fabric mod ── HUD overlay (drafts) + chat auto-send (finals,
                                             with guards: chat/inventory open → clipboard instead)
```

- The microphone channel only passes audio while **F8** is held; releasing it
  force-finalizes the current utterance.
- The loopback channel (`discord`) hears all system audio — Discord, game, music — and is
  always on.
- Every recognized event goes both to the console and to the mod over a local TCP socket
  (protocol: [`mod/README.md`](mod/README.md)).

## Quick start

1. **Requirements**: Rust 1.94+, JDK 25, Windows (for live mode), a CPU with AVX2, and a
   Minecraft 26.2 instance on Fabric (Loader 0.19.5, Fabric API 0.161.0+26.2). The
   recognition models (~250 MB) download automatically on first run.
2. **Build the CLI**:

   ```sh
   cd vocatalk && cargo build --release
   ```

3. **Build the mod** and deploy it:

   ```sh
   cd mod && gradlew.bat build
   ```

   Copy `mod/build/libs/vocatalk-<version>.jar` into the `mods` folder of your instance.
4. **Launch the instance** and join a world (the mod opens the socket in-game).
5. **Run the CLI** and speak:

   ```sh
   vocatalk\target\release\vocatalk.exe --live
   ```

   Wait for `[bridge] connected to the mod at 127.0.0.1:47832`, then hold **F8** and talk.
   Expected output:

   ```
   [ptt] held
   [me     ] draft: привет мир
   [me     ] FINAL: привет мир
   [ptt] released
   ```

   Your speech appears in the Minecraft chat; everything the system plays is sent with a
   `Discord: ` prefix.

## Hotkeys

| Key | Action |
|---|---|
| F8 (hold) | Push-to-talk for your own voice (microphone channel) |

The `discord` channel (all system audio) has no hotkey and is always recognized.

## Known limitations

- The loopback channel hears **everything** the system plays — game audio and music become
  chat lines too. Tune `--vad-threshold` / `--vad-min-silence-ms` to raise the bar; there
  is no per-app filtering.
- Russian speech only (the `e2e_rnnt` head).
- All Discord speakers are merged into one channel and share the `Discord: ` prefix — no
  speaker separation.
- CPU-only recognition: ~RTF 0.18 at the default 10 threads on the development machine
  (i.e. real time ×5 headroom, sharing the CPU with the game).

## Troubleshooting

- **The CLI stays on `[bridge] waiting for the mod ...`** — the game is not running, the
  player is not in a world, or another process holds port 47832. The CLI keeps retrying
  every 2 s and stays fully usable as a console tool.
- **No `me` transcripts** — check `--list-devices` and pick the right input with
  `--input-device <index>`; if F8 is taken by another app, `--live` refuses to start with
  an error (reassign that app's hotkey).
- **Recognition stutters / the game lags** — lower `--threads` (benchmarks:
  [`vocatalk/README.md`](vocatalk/README.md)).

## Future: CUDA

The build is CPU-only on purpose. gigastt-core ships an optional CUDA execution provider
behind a Cargo feature; the development machine has an NVIDIA GTX 1650, so switching later
should be a feature flag plus a CUDA-enabled ONNX Runtime build, with no changes to the
recognition code. ONNX Runtime also offers other execution providers (e.g. DirectML on
Windows, CoreML on macOS) — none of them are verified yet.

## Repository layout

- [`vocatalk/`](vocatalk/) — Rust CLI: batch WAV transcription, live two-channel recognition,
  F8 push-to-talk, TCP bridge. Details, benchmarks, full flag list:
  [`vocatalk/README.md`](vocatalk/README.md)
- [`mod/`](mod/) — Fabric client mod for Minecraft 26.2: TCP listener, HUD overlay, chat
  auto-send with guards. Protocol, build, testing:
  [`mod/README.md`](mod/README.md)
- `PLAN.md` — development plan (stages, fixed decisions, open questions), in Russian

## Status

All five stages are done: engine, live streaming, Fabric mod, TCP + PTT integration and
documentation. In-game end-to-end checks (live speech, PTT feel, mod guards) are manual —
see the quick start above.

## License

MIT. See [LICENSE](vocatalk/LICENSE).
