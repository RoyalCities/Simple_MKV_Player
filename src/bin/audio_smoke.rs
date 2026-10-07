#[path = "../media/audio_engine.rs"]
mod audio_engine;

#[path = "../media/tracks.rs"]
mod tracks;

use audio_engine::AudioEngine;
use tracks::probe_audio_tracks;

use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

fn main() {
    println!("Simple MKV Player - audio engine smoke test");

    println!();
    println!("Choose an MKV file...");

    let Some(path) = rfd::FileDialog::new()
        .add_filter("Matroska Video", &["mkv"])
        .pick_file()
    else {
        println!("No file selected.");

        return;
    };

    let tracks = match probe_audio_tracks(&path) {
        Ok(tracks) => tracks,

        Err(error) => {
            eprintln!("Could not inspect audio tracks: {error}");

            return;
        }
    };

    if tracks.is_empty() {
        println!("No audio tracks found.");

        return;
    }

    println!();
    println!("Detected audio tracks:");

    for track in &tracks {
        println!(
            "  {}. {}  [{} / stream #{}]",
            track.audio_index + 1,
            track.title,
            track.codec,
            track.stream_index,
        );
    }

    println!();

    print!("Which track should I play? [1-{}]: ", tracks.len());

    let _ = io::stdout().flush();

    let mut input = String::new();

    if io::stdin().read_line(&mut input).is_err() {
        eprintln!("Could not read selection.");

        return;
    }

    let selection = input.trim().parse::<usize>().unwrap_or(1);

    let selection = selection.clamp(1, tracks.len());

    let track = &tracks[selection - 1];

    println!();
    println!("Opening Windows audio output...");

    let mut audio = match AudioEngine::new() {
        Ok(audio) => audio,

        Err(error) => {
            eprintln!("Audio engine error: {error}");

            return;
        }
    };

    println!(
        "Output: {} Hz / {} channels",
        audio.sample_rate(),
        audio.channels(),
    );

    println!("Playing '{}' for 10 seconds...", track.title);

    if let Err(error) = audio.start_track(&path, track.audio_index, 0.0) {
        eprintln!("Decoder error: {error}");

        return;
    }

    thread::sleep(Duration::from_secs(10));

    audio.stop();

    println!();
    println!("SUCCESS - audio smoke test finished.");
}
