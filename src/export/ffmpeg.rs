use crate::runtime::{ffmpeg_path, hide_child_console};

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

#[derive(Debug, Clone)]
pub struct ExportMixTrack {
    pub audio_index: usize,
    pub gain_db: f32,
}

pub fn export_track_wav_with_progress<F>(
    input_path: &Path,
    output_path: &Path,
    audio_index: usize,
    gain_db: f32,
    title: Option<&str>,
    duration_seconds: f64,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(f32),
{
    let gain = db_to_linear(gain_db.clamp(-60.0, 40.0));

    let mut command = Command::new(ffmpeg_path());
    hide_child_console(&mut command);

    command
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-map")
        .arg(format!("0:a:{audio_index}"))
        .arg("-vn")
        .arg("-sn")
        .arg("-dn")
        .arg("-filter:a")
        .arg(format!("volume={gain:.9}"))
        .arg("-c:a")
        .arg("pcm_s24le");

    if let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) {
        command.arg("-metadata").arg(format!("title={title}"));
    }

    command
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    run_ffmpeg_with_progress(&mut command, duration_seconds, progress)
}

pub fn export_video_mix_mkv_with_progress<F>(
    input_path: &Path,
    output_path: &Path,
    tracks: &[ExportMixTrack],
    master_gain_db: f32,
    duration_seconds: f64,
    audio_title: Option<&str>,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(f32),
{
    if tracks.is_empty() {
        return Err("No enabled audio tracks were supplied.".to_string());
    }

    let filter = build_mix_filter(tracks, master_gain_db);

    let mut command = Command::new(ffmpeg_path());
    hide_child_console(&mut command);

    command
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("[mixout]")
        .arg("-c:v")
        .arg("copy")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg("320k")
        .arg("-ac")
        .arg("2");

    if let Some(title) = audio_title.map(str::trim).filter(|title| !title.is_empty()) {
        command.arg("-metadata:s:a:0").arg(format!("title={title}"));
    }

    command
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    run_ffmpeg_with_progress(&mut command, duration_seconds, progress)
}

pub fn export_mix_wav_with_progress<F>(
    input_path: &Path,
    output_path: &Path,
    tracks: &[ExportMixTrack],
    master_gain_db: f32,
    duration_seconds: f64,
    progress: F,
) -> Result<(), String>
where
    F: FnMut(f32),
{
    if tracks.is_empty() {
        return Err("No enabled audio tracks were supplied.".to_string());
    }

    let filter = build_mix_filter(tracks, master_gain_db);

    let mut command = Command::new(ffmpeg_path());
    hide_child_console(&mut command);

    command
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input_path)
        .arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("[mixout]")
        .arg("-vn")
        .arg("-sn")
        .arg("-dn")
        .arg("-c:a")
        .arg("pcm_s24le")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg(output_path);

    run_ffmpeg_with_progress(&mut command, duration_seconds, progress)
}

fn run_ffmpeg_with_progress<F>(
    command: &mut Command,
    duration_seconds: f64,
    mut progress: F,
) -> Result<(), String>
where
    F: FnMut(f32),
{
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not launch ffmpeg: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not read ffmpeg progress output.".to_string())?;

    let reader = BufReader::new(stdout);

    progress(0.0);

    for line in reader.lines() {
        let line = line.map_err(|error| format!("Could not read ffmpeg progress: {error}"))?;

        if let Some(value) = line.strip_prefix("out_time_us=") {
            if duration_seconds > 0.0 {
                if let Ok(microseconds) = value.trim().parse::<f64>() {
                    let seconds = microseconds / 1_000_000.0;
                    progress((seconds / duration_seconds).clamp(0.0, 1.0) as f32);
                }
            }
        } else if let Some(value) = line.strip_prefix("out_time_ms=") {
            if duration_seconds > 0.0 {
                if let Ok(microseconds) = value.trim().parse::<f64>() {
                    let seconds = microseconds / 1_000_000.0;
                    progress((seconds / duration_seconds).clamp(0.0, 1.0) as f32);
                }
            }
        } else if line.trim() == "progress=end" {
            progress(1.0);
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("Could not wait for ffmpeg: {error}"))?;

    if output.status.success() {
        progress(1.0);
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);

    let message = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("ffmpeg returned an unknown error.");

    Err(message.trim().to_string())
}

fn build_mix_filter(tracks: &[ExportMixTrack], master_gain_db: f32) -> String {
    let mut parts = Vec::<String>::new();

    let mut labels = Vec::<String>::new();

    for (position, track) in tracks.iter().enumerate() {
        let label = format!("track{position}");

        let gain = db_to_linear(track.gain_db.clamp(-60.0, 40.0));

        parts.push(format!(
            "[0:a:{}]volume={gain:.9}[{label}]",
            track.audio_index
        ));

        labels.push(format!("[{label}]"));
    }

    let master_gain = db_to_linear(master_gain_db.clamp(-60.0, 40.0));

    if labels.len() == 1 {
        parts.push(format!("{}volume={master_gain:.9}[mixout]", labels[0]));
    } else {
        parts.push(format!(
            "{}amix=inputs={}:normalize=0:dropout_transition=0,volume={master_gain:.9}[mixout]",
            labels.join(""),
            labels.len()
        ));
    }

    parts.join(";")
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_track_filter_uses_track_and_master_gain() {
        let filter = build_mix_filter(
            &[ExportMixTrack {
                audio_index: 2,
                gain_db: 6.0,
            }],
            -3.0,
        );

        assert!(filter.contains("[0:a:2]volume="));

        assert!(filter.contains("[mixout]"));

        assert!(!filter.contains("amix="));
    }

    #[test]
    fn multiple_tracks_use_amix_without_normalization() {
        let filter = build_mix_filter(
            &[
                ExportMixTrack {
                    audio_index: 0,
                    gain_db: 0.0,
                },
                ExportMixTrack {
                    audio_index: 2,
                    gain_db: -6.0,
                },
            ],
            0.0,
        );

        assert!(filter.contains("amix=inputs=2:normalize=0"));
    }
}
