mod audio;
mod live;
mod net;
mod ptt;

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Parser;
use gigastt_core::inference::Engine;
use gigastt_core::model::{self, ModelVariant};
use gigastt_core::vad::VadConfig;

#[derive(Parser)]
#[command(
    name = "vocatalk",
    about = "Voice input for Minecraft chat: file transcription and live two-channel recognition"
)]
struct Cli {
    /// Transcribe a WAV file (batch mode)
    #[arg(long, conflicts_with = "live")]
    file: Option<PathBuf>,

    /// Live recognition: microphone (F8 push-to-talk) + all system audio (loopback), streaming drafts and finals
    #[arg(long)]
    live: bool,

    /// Model directory (defaults to ~/.gigastt/models)
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// Encoder intra-op threads (default: logical CPUs / 2, clamped to 2..=10)
    #[arg(long)]
    threads: Option<usize>,

    /// List audio devices and exit
    #[arg(long)]
    list_devices: bool,

    /// Microphone device index for --live (see --list-devices)
    #[arg(long)]
    input_device: Option<usize>,

    /// VAD speech-probability threshold for --live, in (0, 1]
    #[arg(long, default_value_t = 0.5)]
    vad_threshold: f32,

    /// Trailing silence before the VAD ends an utterance, in milliseconds
    #[arg(long, default_value_t = 600)]
    vad_min_silence_ms: u32,

    /// Chat display name for the microphone channel
    #[arg(long, default_value = "Вэвси")]
    name_me: String,

    /// Chat display name for the system-audio (loopback) channel
    #[arg(long, default_value = "Аури")]
    name_discord: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    ListDevices,
    File(PathBuf),
    Live,
}

fn resolve_mode(cli: &Cli) -> Result<Mode> {
    if cli.list_devices {
        return Ok(Mode::ListDevices);
    }
    match (&cli.file, cli.live) {
        (Some(path), false) => Ok(Mode::File(path.clone())),
        (None, true) => Ok(Mode::Live),
        // defense-in-depth: clap enforces this via conflicts_with
        (Some(_), true) => bail!("use exactly one of --file or --live"),
        (None, false) => bail!("nothing to do: pass --file <wav> or --live (see --list-devices)"),
    }
}

fn resolve_threads(cli: &Cli) -> Result<usize> {
    let threads = cli.threads.unwrap_or_else(default_threads);
    anyhow::ensure!(threads >= 1, "--threads must be >= 1");
    Ok(threads)
}

fn resolve_model_dir(cli: &Cli) -> Result<String> {
    match &cli.model_dir {
        Some(dir) => dir
            .to_str()
            .context("model directory path contains non-UTF8 characters")
            .map(str::to_owned),
        None => Ok(model::default_model_dir()),
    }
}

fn resolve_vad(cli: &Cli) -> Result<VadConfig> {
    anyhow::ensure!(
        cli.vad_threshold > 0.0 && cli.vad_threshold <= 1.0,
        "--vad-threshold must be in (0, 1]"
    );
    Ok(VadConfig {
        threshold: cli.vad_threshold,
        min_silence_ms: cli.vad_min_silence_ms,
        ..VadConfig::default()
    })
}

fn threads_for_cpus(logical_cpus: usize) -> usize {
    (logical_cpus / 2).clamp(2, 10)
}

fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| threads_for_cpus(n.get()))
        .unwrap_or(4)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match resolve_mode(&cli)? {
        Mode::ListDevices => live::list_devices(),
        Mode::File(path) => transcribe_file(&cli, &path).await,
        Mode::Live => {
            live::run(
                &resolve_model_dir(&cli)?,
                resolve_threads(&cli)?,
                cli.input_device,
                resolve_vad(&cli)?,
                net::SpeakerNames {
                    me: cli.name_me.clone(),
                    discord: cli.name_discord.clone(),
                },
            )
            .await
        }
    }
}

async fn transcribe_file(cli: &Cli, file: &Path) -> Result<()> {
    anyhow::ensure!(file.is_file(), "file not found: {}", file.display());

    let model_dir = resolve_model_dir(cli)?;
    let threads = resolve_threads(cli)?;

    eprintln!("models: {model_dir}");
    let variant = model::ensure_model_variant(Some(ModelVariant::E2eRnnt), &model_dir).await?;
    eprintln!("head: {} (INT8)", variant.as_str());
    eprintln!("using {threads} encoder threads");

    let engine = Engine::load_with_pools_threads_variant(
        &model_dir,
        Some(ModelVariant::E2eRnnt),
        1,
        1,
        0,
        threads,
    )
    .context("failed to load the model")?;

    let path = file
        .to_str()
        .context("audio file path contains non-UTF8 characters")?;

    let mut guard = engine
        .pool
        .checkout()
        .await
        .context("session pool is empty")?;
    let started = Instant::now();
    let result = engine
        .transcribe_file(path, &mut guard)
        .context("transcription failed")?;
    let wall = started.elapsed();

    println!("{}", result.text);

    let wall_s = wall.as_secs_f64();
    let rtf = if result.duration_s > 0.0 {
        wall_s / result.duration_s
    } else {
        f64::NAN
    };
    println!();
    println!("audio: {:.2} s", result.duration_s);
    println!("recognition: {wall_s:.2} s (wall), RTF {rtf:.3}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_file_flag() {
        let cli = Cli::parse_from(["vocatalk", "--file", "sample.wav"]);
        assert_eq!(cli.file, Some(PathBuf::from("sample.wav")));
        assert!(!cli.live);
        assert!(cli.model_dir.is_none());
    }

    #[test]
    fn file_and_live_together_is_error() {
        let parsed = Cli::try_parse_from(["vocatalk", "--file", "a.wav", "--live"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn neither_file_nor_live_is_error() {
        let cli = Cli::parse_from(["vocatalk"]);
        let err = resolve_mode(&cli).unwrap_err();
        assert!(err.to_string().contains("--file"));
        assert!(err.to_string().contains("--live"));
    }

    #[test]
    fn live_without_file_is_ok() {
        let cli = Cli::parse_from(["vocatalk", "--live"]);
        assert_eq!(resolve_mode(&cli).unwrap(), Mode::Live);
    }

    #[test]
    fn file_mode_resolves_to_path() {
        let cli = Cli::parse_from(["vocatalk", "--file", "a.wav"]);
        assert_eq!(
            resolve_mode(&cli).unwrap(),
            Mode::File(PathBuf::from("a.wav"))
        );
    }

    #[test]
    fn parses_model_dir() {
        let cli = Cli::parse_from(["vocatalk", "--file", "a.wav", "--model-dir", "D:/models"]);
        assert_eq!(cli.model_dir.as_deref(), Some(Path::new("D:/models")));
    }

    #[test]
    fn threads_flag_defaults_to_none() {
        let cli = Cli::parse_from(["vocatalk", "--file", "a.wav"]);
        assert!(cli.threads.is_none());
    }

    #[test]
    fn threads_flag_parses() {
        let cli = Cli::parse_from(["vocatalk", "--file", "a.wav", "--threads", "6"]);
        assert_eq!(cli.threads, Some(6));
    }

    #[test]
    fn default_thread_count_is_clamped() {
        assert_eq!(threads_for_cpus(1), 2);
        assert_eq!(threads_for_cpus(4), 2);
        assert_eq!(threads_for_cpus(12), 6);
        assert_eq!(threads_for_cpus(28), 10);
        assert_eq!(threads_for_cpus(64), 10);
    }

    #[test]
    fn vad_flags_have_documented_defaults() {
        let cli = Cli::parse_from(["vocatalk", "--live"]);
        let vad = resolve_vad(&cli).unwrap();
        assert_eq!(vad.threshold, 0.5);
        assert_eq!(vad.min_silence_ms, 600);
    }

    #[test]
    fn vad_flags_parse_custom_values() {
        let cli = Cli::parse_from([
            "vocatalk",
            "--live",
            "--vad-threshold",
            "0.7",
            "--vad-min-silence-ms",
            "300",
        ]);
        let vad = resolve_vad(&cli).unwrap();
        assert_eq!(vad.threshold, 0.7);
        assert_eq!(vad.min_silence_ms, 300);
    }

    #[test]
    fn vad_threshold_out_of_range_is_error() {
        for bad in ["0", "1.5", "nan"] {
            let cli = Cli::parse_from(["vocatalk", "--live", "--vad-threshold", bad]);
            let err = resolve_vad(&cli).unwrap_err();
            assert!(err.to_string().contains("--vad-threshold"), "bad={bad}");
        }
    }

    #[test]
    fn name_flags_have_documented_defaults() {
        let cli = Cli::parse_from(["vocatalk", "--live"]);
        assert_eq!(cli.name_me, "Вэвси");
        assert_eq!(cli.name_discord, "Аури");
    }

    #[test]
    fn name_flags_parse_custom_values() {
        let cli = Cli::parse_from([
            "vocatalk",
            "--live",
            "--name-me",
            "Test",
            "--name-discord",
            "Bot",
        ]);
        assert_eq!(cli.name_me, "Test");
        assert_eq!(cli.name_discord, "Bot");
    }
}
