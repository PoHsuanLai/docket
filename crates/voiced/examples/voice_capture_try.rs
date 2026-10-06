//! The owner's by-hand check of voiced's PipeWire device (spike V-A; run it through
//! `dev/voice-capture-try.sh`). It uses the same `PipeWireDevice` the daemon does.
//!
//! - `list`: prints the audio nodes and which one voiced would capture. Read-only: it opens no
//!   stream and never touches the microphone.
//! - `capture <seconds>`: opens the chosen microphone at 16 kHz mono S16 for that long and prints
//!   the frame sizes, the gaps between frames (jitter) and the peak level. The microphone is
//!   live while it runs. It also shows that a monitor node is refused.
//! - `play`: plays half a second of a 440 Hz tone at 24 kHz on the default output.

use std::process::ExitCode;
use std::time::{Duration, Instant};
use voiced::{
    AudioDevice, CaptureFormat, CaptureStream, NodeKind, PipeWireDevice, PlaybackFormat,
    PlaybackStream, choose_capture,
};

/// The override to try: `VOICED_INPUT`, standing in for `input = "..."` in voiced.toml.
fn configured_input() -> Option<String> {
    std::env::var("VOICED_INPUT").ok().filter(|v| !v.is_empty())
}

async fn list(device: &PipeWireDevice) -> ExitCode {
    let nodes = device.sources().await;
    for node in &nodes {
        println!(
            "{:>4}  {:<8}  {:<22}  {}",
            node.id.0,
            format!("{:?}", node.kind),
            node.media_class.0,
            node.name
        );
    }
    let default = device.default_source().await;
    println!(
        "PipeWire default source: {}",
        default.as_deref().unwrap_or("none named")
    );
    let input = configured_input();
    println!(
        "voiced.toml input: {}",
        input
            .as_deref()
            .unwrap_or("not set (VOICED_INPUT=<node.name> tries one)")
    );
    match choose_capture(&nodes, default.as_deref(), input.as_deref()) {
        Some(chosen) => println!(
            "voiced would capture: {} ({}), chosen by {}",
            chosen.node.name,
            chosen.node.id.0,
            chosen.by.word()
        ),
        None => println!("voiced would capture: nothing (no physical Audio/Source)"),
    }
    ExitCode::SUCCESS
}

async fn capture(device: &PipeWireDevice, seconds: u64) -> ExitCode {
    let nodes = device.sources().await;
    for monitor in nodes.iter().filter(|n| n.kind != NodeKind::Source) {
        let refused = device
            .open_capture(monitor, CaptureFormat { rate: 16_000 })
            .await
            .is_err();
        println!("{:?} {}: refused = {refused}", monitor.kind, monitor.name);
    }
    let default = device.default_source().await;
    let Some(node) = choose_capture(&nodes, default.as_deref(), configured_input().as_deref())
        .map(|chosen| chosen.node)
    else {
        eprintln!("no physical microphone");
        return ExitCode::FAILURE;
    };
    println!(
        "capturing {} for {seconds} s at 16 kHz mono S16 ...",
        node.name
    );
    let Ok(mut stream) = device
        .open_capture(node, CaptureFormat { rate: 16_000 })
        .await
    else {
        eprintln!("the microphone did not open");
        return ExitCode::FAILURE;
    };
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let (mut frames, mut samples, mut peak) = (0_u64, 0_u64, 0_u16);
    let (mut last, mut widest) = (Instant::now(), Duration::ZERO);
    let mut sizes = std::collections::BTreeMap::<usize, u64>::new();
    while Instant::now() < deadline {
        let Ok(Some(frame)) = tokio::time::timeout(Duration::from_secs(1), stream.next()).await
        else {
            eprintln!("no frame for a second: the stream stalled or ended");
            return ExitCode::FAILURE;
        };
        let now = Instant::now();
        widest = widest.max(now - last);
        last = now;
        frames += 1;
        samples += frame.len() as u64;
        *sizes.entry(frame.len()).or_default() += 1;
        peak = peak.max(frame.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
    }
    println!(
        "{frames} frames, {samples} samples (expect about {} )",
        seconds * 16_000
    );
    println!("frame sizes: {sizes:?}");
    println!("widest gap between frames: {} ms", widest.as_millis());
    println!("peak sample: {peak} of 32768");
    ExitCode::SUCCESS
}

async fn play(device: &PipeWireDevice) -> ExitCode {
    let Ok(mut out) = device.open_playback(PlaybackFormat { rate: 24_000 }).await else {
        eprintln!("the default output did not open");
        return ExitCode::FAILURE;
    };
    let tone: Vec<i16> = (0..12_000_i32)
        .map(|i| {
            let phase = (i * 440 % 24_000) * 2 - 24_000;
            let triangle = (phase.abs() - 12_000) * 3 / 10;
            i16::try_from(triangle).unwrap_or(0)
        })
        .collect();
    if out.write(&tone).await.is_err() {
        eprintln!("write failed");
        return ExitCode::FAILURE;
    }
    out.fade_out(0).await;
    println!("played 12000 samples at 24 kHz");
    ExitCode::SUCCESS
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let device = PipeWireDevice;
    match args.first().map(String::as_str) {
        None | Some("list") => list(&device).await,
        Some("capture") => {
            let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
            capture(&device, seconds).await
        }
        Some("play") => play(&device).await,
        Some(other) => {
            eprintln!("usage: voice_capture_try [list | capture <seconds> | play] (got {other})");
            ExitCode::FAILURE
        }
    }
}
