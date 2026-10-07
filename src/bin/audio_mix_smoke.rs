#[path = "../media/audio_mixer.rs"]
mod audio_mixer;

#[path = "../media/tracks.rs"]
mod tracks;

use audio_mixer::{AudioMixer, MixTrackConfig};

use tracks::probe_audio_tracks;

use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

fn main() {
    println!("Simple MKV Player - multi-track mixer smoke test");

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
            eprintln!("Track probe failed: {error}");

            return;
        }
    };

    if tracks.is_empty() {
        println!("No audio tracks found.");

        return;
    }

    println!();
    println!("Detected audio tracks:");

    for (i, track) in tracks.iter().enumerate() {
        println!(
            "  {}. {}  [{} / stream #{}]",
            i + 1,
            track.title,
            track.codec,
            track.stream_index,
        );
    }

    println!();
    println!("Enter tracks to mix.");

    println!("Example: 2,3");

    print!("> ");

    let _ = io::stdout().flush();

    let mut input = String::new();

    if io::stdin().read_line(&mut input).is_err() {
        eprintln!("Could not read selection.");

        return;
    }

    let mut configs = Vec::<MixTrackConfig>::new();

    for token in input.split(',') {
        let Ok(number) = token.trim().parse::<usize>() else {
            continue;
        };

        if number == 0 || number > tracks.len() {
            continue;
        }

        let track = &tracks[number - 1];

        configs.push(MixTrackConfig {
            audio_index: track.audio_index,

            volume: 1.0,

            enabled: true,
        });
    }

    if configs.is_empty() {
        eprintln!("No valid tracks selected.");

        return;
    }

    println!();
    println!("Opening Windows audio mixer...");

    let mut mixer = match AudioMixer::new() {
        Ok(mixer) => mixer,

        Err(error) => {
            eprintln!("Mixer error: {error}");

            return;
        }
    };

    println!(
        "Output: {} Hz / {} channels",
        mixer.sample_rate(),
        mixer.channels(),
    );

    println!();
    println!("Starting {} simultaneous decoder(s)...", configs.len());

    if let Err(error) = mixer.start_mix(&path, &configs, 0.0) {
        eprintln!("Could not start mix: {error}");

        return;
    }

    println!();
    println!("Playing combined mix for 15 seconds...");

    thread::sleep(Duration::from_secs(15));

    mixer.stop_all();

    println!();
    println!("SUCCESS - multi-track mix finished.");
}
