use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use gigastt_core::inference::Engine;
use gigastt_core::model::{self, ModelVariant};

#[derive(Parser)]
#[command(
    name = "vocatalk",
    about = "Voice input for Minecraft chat — stage 1: recognition engine"
)]
struct Cli {
    /// Path to a WAV file with Russian speech
    #[arg(long)]
    file: PathBuf,

    /// Model directory (defaults to ~/.gigastt/models)
    #[arg(long)]
    model_dir: Option<PathBuf>,

    /// Encoder intra-op threads (default: logical CPUs / 2, clamped to 2..=10)
    #[arg(long)]
    threads: Option<usize>,
}

fn resolve_threads(logical_cpus: usize) -> usize {
    (logical_cpus / 2).clamp(2, 10)
}

fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| resolve_threads(n.get()))
        .unwrap_or(4)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    anyhow::ensure!(cli.file.is_file(), "file not found: {}", cli.file.display());

    let model_dir = match &cli.model_dir {
        Some(dir) => dir
            .to_str()
            .context("model directory path contains non-UTF8 characters")?
            .to_owned(),
        None => model::default_model_dir(),
    };

    eprintln!("models: {model_dir}");
    let variant = model::ensure_model_variant(Some(ModelVariant::E2eRnnt), &model_dir).await?;
    eprintln!("head: {} (INT8)", variant.as_str());

    let threads = cli.threads.unwrap_or_else(default_threads);
    anyhow::ensure!(threads >= 1, "--threads must be >= 1");
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

    let path = cli
        .file
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
        assert_eq!(cli.file, PathBuf::from("sample.wav"));
        assert!(cli.model_dir.is_none());
    }

    #[test]
    fn file_flag_is_required() {
        assert!(Cli::try_parse_from(["vocatalk"]).is_err());
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
        assert_eq!(resolve_threads(1), 2);
        assert_eq!(resolve_threads(4), 2);
        assert_eq!(resolve_threads(12), 6);
        assert_eq!(resolve_threads(28), 10);
        assert_eq!(resolve_threads(64), 10);
    }
}
