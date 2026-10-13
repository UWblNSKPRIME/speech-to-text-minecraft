//! Live two-channel recognition: microphone (cpal) + all system audio
//! (WASAPI loopback). Audio callbacks only push chunks into channels; one
//! worker thread per channel owns resampling, engine inference and printing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use gigastt_core::inference::{Engine, StreamingState, TranscriptSegment};
use gigastt_core::model::{self, ModelVariant};
use gigastt_core::vad::{SileroVad, VadConfig, VAD_MODEL_FILE};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use crate::audio::{downmix_to_mono, resample_to_engine, RawChunk};
use crate::net::{ModBridge, SpeakerNames};
use crate::ptt::{self, Ptt};

/// Samples per engine call (~0.2 s at 16 kHz): batches callback-sized chunks
/// so per-call overhead stays small without adding much latency.
const BATCH_SAMPLES: usize = 3200;

/// Two parallel sessions: one per channel (microphone / system audio).
const POOL_SIZE: usize = 2;

#[derive(Default)]
struct ChannelStats {
    partials: AtomicU64,
    finals: AtomicU64,
    first_partial: Mutex<Option<Duration>>,
}

/// Enumerate input-capable devices in a stable order (used by `--list-devices`
/// and `--input-device`).
fn input_devices(host: &cpal::Host) -> Result<Vec<cpal::Device>> {
    Ok(host
        .devices()
        .context("cannot enumerate audio devices")?
        .filter(|device| device.supports_input())
        .collect())
}

fn device_label(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "<unnamed>".to_string())
}

/// List available audio input devices and the loopback source, then exit.
pub fn list_devices() -> Result<()> {
    println!("Input devices (microphone, cpal):");
    let host = cpal::default_host();
    for (index, device) in input_devices(&host)?.iter().enumerate() {
        println!("  [{index}] {}", device_label(device));
    }

    println!("Loopback capture (all system audio, via the default render device):");
    let _ = wasapi::initialize_mta().ok();
    let enumerator =
        DeviceEnumerator::new().context("cannot create the WASAPI device enumerator")?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .context("no default render device")?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "<unnamed>".to_string());
    println!("  [0] {name}");
    Ok(())
}

/// Run the live pipeline until Ctrl+C.
pub async fn run(
    model_dir: &str,
    threads: usize,
    input_device: Option<usize>,
    vad: VadConfig,
    names: SpeakerNames,
) -> Result<()> {
    let vad_dir = model::default_vad_model_dir();
    eprintln!("models: {model_dir}");
    eprintln!("vad: {vad_dir}");
    model::ensure_model_variant(Some(ModelVariant::E2eRnnt), model_dir).await?;
    model::ensure_vad_model(&vad_dir)
        .await
        .context("failed to download the VAD model")?;
    let vad_path = std::path::Path::new(&vad_dir).join(VAD_MODEL_FILE);
    let silero = SileroVad::load(&vad_path)
        .with_context(|| format!("failed to load the VAD model from {}", vad_path.display()))?;
    eprintln!(
        "head: {} (INT8), Silero VAD attached (threshold {}, min silence {} ms)",
        ModelVariant::E2eRnnt.as_str(),
        vad.threshold,
        vad.min_silence_ms
    );

    let engine = Arc::new(
        Engine::load_with_pools_threads_variant(
            model_dir,
            Some(ModelVariant::E2eRnnt),
            POOL_SIZE,
            POOL_SIZE,
            0,
            threads,
        )
        .context("failed to load the model")?
        .with_vad(Some(silero), vad),
    );

    let bridge = ModBridge::start(names);
    let ptt = ptt::start().context("push-to-talk is required for --live")?;
    let ptt_gate = ptt.gate().clone();

    let running = Arc::new(AtomicBool::new(true));
    let (me_tx, me_rx) = sync_channel::<RawChunk>(256);
    let (discord_tx, discord_rx) = sync_channel::<RawChunk>(256);

    let mic_stream = match start_microphone(input_device, me_tx) {
        Ok(stream) => Some(stream),
        Err(err) => {
            eprintln!("[me     ] unavailable: {err:#}");
            None
        }
    };

    let loopback = thread::Builder::new()
        .name("wasapi-loopback".to_string())
        .spawn({
            let running = running.clone();
            move || loopback_capture(discord_tx, &running)
        })
        .context("failed to spawn the loopback thread")?;

    let live_start = Instant::now();
    eprintln!("live: listening on both channels (Ctrl+C to stop)");

    let mut workers = Vec::new();
    for (name, rx) in [("me", me_rx), ("discord", discord_rx)] {
        let stats = Arc::new(ChannelStats::default());
        // Only the microphone channel is gated by push-to-talk.
        let gate = (name == "me").then(|| ptt_gate.clone());
        let handle = thread::Builder::new()
            .name(format!("worker-{name}"))
            .spawn({
                let engine = engine.clone();
                let stats = stats.clone();
                let bridge = bridge.clone();
                move || channel_worker(name, rx, engine, stats, live_start, bridge, gate)
            })
            .context("failed to spawn a channel worker")?;
        workers.push((name, handle, stats));
    }

    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for Ctrl+C")?;
    eprintln!("shutting down...");
    running.store(false, Ordering::Relaxed);
    drop(mic_stream);
    if loopback.join().is_err() {
        eprintln!("[discord] loopback thread panicked");
    }

    for (name, handle, stats) in workers {
        if handle.join().is_err() {
            eprintln!("[{name:7}] worker thread panicked");
            continue;
        }
        let partials = stats.partials.load(Ordering::Relaxed);
        let finals = stats.finals.load(Ordering::Relaxed);
        eprintln!("[{name:7}] partials: {partials}, finals: {finals}");
        match *stats.first_partial.lock().expect("stats mutex poisoned") {
            Some(ttfp) => eprintln!("TTFP[{name}]: {:.2} s", ttfp.as_secs_f64()),
            None => eprintln!("TTFP[{name}]: n/a (no speech detected)"),
        }
    }
    Ok(())
}

/// Open the microphone (default input device, or `index` from `--list-devices`)
/// and stream chunks into `tx`. The stream runs on cpal's own callback thread.
fn start_microphone(index: Option<usize>, tx: SyncSender<RawChunk>) -> Result<Stream> {
    let host = cpal::default_host();
    let device = match index {
        Some(index) => input_devices(&host)?
            .into_iter()
            .nth(index)
            .with_context(|| format!("no input device with index {index} (see --list-devices)"))?,
        None => host
            .default_input_device()
            .context("no default microphone available")?,
    };
    let device_name = device_label(&device);
    let supported = device
        .default_input_config()
        .context("cannot query the microphone format")?;
    let rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let mut config: cpal::StreamConfig = supported.into();
    // In shared mode the event period is fixed by the device; a larger ring
    // buffer only adds slack, so a delayed read does not drop capture data
    // (AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY → cpal Xrun) under CPU bursts.
    config.buffer_size = cpal::BufferSize::Fixed(rate / 10);
    let err_fn = |err| eprintln!("[me     ] stream error: {err}");
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                let _ = tx.try_send(RawChunk {
                    samples: data.to_vec(),
                    channels,
                    rate,
                });
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                let samples = data.iter().map(|&s| s as f32 / 32768.0).collect();
                let _ = tx.try_send(RawChunk {
                    samples,
                    channels,
                    rate,
                });
            },
            err_fn,
            None,
        ),
        format => anyhow::bail!("unsupported microphone sample format: {format}"),
    }
    .context("cannot open the microphone stream")?;
    stream
        .play()
        .context("cannot start the microphone stream")?;
    eprintln!("[me     ] microphone: {device_name} ({rate} Hz, {channels} ch)");
    Ok(stream)
}

/// Capture the whole system audio (what Discord plays) via WASAPI loopback
/// and stream chunks into `tx`. Runs on a dedicated thread.
fn loopback_capture(tx: SyncSender<RawChunk>, running: &AtomicBool) {
    if let Err(err) = loopback_capture_inner(tx, running) {
        eprintln!("[discord] loopback failed: {err:#}");
    }
}

fn loopback_capture_inner(tx: SyncSender<RawChunk>, running: &AtomicBool) -> Result<()> {
    wasapi::initialize_mta()
        .ok()
        .context("COM initialization failed")?;
    let enumerator =
        DeviceEnumerator::new().context("cannot create the WASAPI device enumerator")?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .context("no default render device for loopback")?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "<unnamed>".to_string());

    let mut audio_client = device
        .get_iaudioclient()
        .context("cannot create the WASAPI audio client")?;
    let mix = audio_client
        .get_mixformat()
        .context("cannot query the device mix format")?;
    let rate = mix.get_samplespersec();
    let channels = mix.get_nchannels() as usize;
    let format = WaveFormat::new(32, 32, &SampleType::Float, rate as usize, channels, None);
    let (_, min_period) = audio_client
        .get_device_period()
        .context("cannot query the device period")?;
    // Direction::Capture on a render device = AUDCLNT_STREAMFLAGS_LOOPBACK.
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: min_period,
    };
    audio_client
        .initialize_client(&format, &Direction::Capture, &mode)
        .context("cannot initialize the loopback capture")?;
    let event = audio_client
        .set_get_eventhandle()
        .context("cannot create the WASAPI event handle")?;
    let capture = audio_client
        .get_audiocaptureclient()
        .context("cannot create the WASAPI capture client")?;
    let blockalign = format.get_blockalign() as usize;
    audio_client
        .start_stream()
        .context("cannot start the loopback stream")?;
    eprintln!("[discord] loopback: {name} ({rate} Hz, {channels} ch)");

    let mut bytes: VecDeque<u8> = VecDeque::with_capacity(blockalign * 4096);
    let mut samples: Vec<f32> = Vec::with_capacity(4096 * channels);
    while running.load(Ordering::Relaxed) {
        capture
            .read_from_device_to_deque(&mut bytes)
            .context("loopback read failed")?;
        let frames = bytes.len() / blockalign;
        for _ in 0..frames {
            let mut buf = [0u8; 4];
            for byte in &mut buf {
                *byte = bytes.pop_front().expect("deque holds the drained bytes");
            }
            samples.push(f32::from_le_bytes(buf));
        }
        if !samples.is_empty() {
            let chunk = RawChunk {
                samples: std::mem::take(&mut samples),
                channels,
                rate,
            };
            if tx.send(chunk).is_err() {
                break;
            }
        }
        if event.wait_for_event(200).is_err() {
            eprintln!("[discord] loopback event wait failed");
            break;
        }
    }
    audio_client.stop_stream().ok();
    Ok(())
}

/// Per-channel worker: drain chunks, downmix, resample, run the engine,
/// emit drafts/finals (console + mod bridge), collect stats. Owns one pool
/// session. The microphone channel only admits samples while PTT is held and
/// flushes the tail on release.
fn channel_worker(
    name: &'static str,
    rx: Receiver<RawChunk>,
    engine: Arc<Engine>,
    stats: Arc<ChannelStats>,
    live_start: Instant,
    bridge: ModBridge,
    ptt_gate: Option<Arc<Ptt>>,
) {
    let mut guard = match engine.pool.checkout_blocking() {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("[{name:7}] no session available: {err}");
            return;
        }
    };
    let mut state: StreamingState = engine.create_state(false);
    let mut pending: Vec<f32> = Vec::new();
    let mut decode = |batch: &[f32], state: &mut StreamingState| match engine
        .process_chunk(batch, state, &mut guard)
    {
        Ok(segments) => {
            for segment in &segments {
                emit_segment(name, segment, &stats, live_start, &bridge);
            }
        }
        Err(err) => eprintln!("[{name:7}] decode error: {err}"),
    };
    let flush_tail = |state: &mut StreamingState| {
        if let Some(tail) = engine.flush_state(state) {
            emit_segment(name, &tail, &stats, live_start, &bridge);
        }
    };

    let mut was_held = ptt::admits(ptt_gate.as_ref());
    loop {
        let held = ptt::admits(ptt_gate.as_ref());
        if ptt::released_edge(was_held, held) {
            // PTT let go: decode the leftover batch and force-finalize the
            // utterance so the tail is not lost.
            if !pending.is_empty() {
                let batch = std::mem::take(&mut pending);
                decode(&batch, &mut state);
            }
            flush_tail(&mut state);
        }
        was_held = held;

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                if !held {
                    // PTT is up: microphone audio never reaches the engine.
                    continue;
                }
                let mono = downmix_to_mono(&chunk.samples, chunk.channels);
                match resample_to_engine(&mono, chunk.rate) {
                    Ok(audio) => pending.extend_from_slice(&audio),
                    Err(err) => {
                        eprintln!("[{name:7}] resample failed: {err}");
                        continue;
                    }
                }
                while pending.len() >= BATCH_SAMPLES {
                    let batch: Vec<f32> = pending.drain(..BATCH_SAMPLES).collect();
                    decode(&batch, &mut state);
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    if !pending.is_empty() {
        decode(&pending, &mut state);
    }
    flush_tail(&mut state);
    // Exit stats (partials/finals/TTFP) are printed by run() after join.
}

/// Print a segment and forward it to the mod bridge (one JSON line).
fn emit_segment(
    name: &str,
    segment: &TranscriptSegment,
    stats: &ChannelStats,
    live_start: Instant,
    bridge: &ModBridge,
) {
    if segment.text.trim().is_empty() {
        return;
    }
    if segment.is_final {
        println!("[{name:7}] FINAL: {}", segment.text);
        stats.finals.fetch_add(1, Ordering::Relaxed);
        bridge.send("final", name, &segment.text);
    } else {
        println!("[{name:7}] draft: {}", segment.text);
        stats.partials.fetch_add(1, Ordering::Relaxed);
        bridge.send("draft", name, &segment.text);
        let mut first = stats.first_partial.lock().expect("stats mutex poisoned");
        first.get_or_insert_with(|| live_start.elapsed());
    }
}
