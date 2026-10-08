use serde::Deserialize;
use std::{collections::HashMap, path::Path, process::Command};

#[derive(Debug, Clone)]
pub struct DetectedAudioTrack {
    /// Position among AUDIO streams: 0, 1, 2...
    pub audio_index: usize,

    /// Absolute stream index inside the container.
    pub stream_index: usize,

    pub title: String,
    pub codec: String,
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    index: usize,

    #[serde(default)]
    codec_name: Option<String>,

    #[serde(default)]
    sample_rate: Option<String>,

    #[serde(default)]
    tags: HashMap<String, String>,
}

pub fn probe_audio_tracks(path: &Path) -> Result<Vec<DetectedAudioTrack>, String> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("a")
        .arg("-show_entries")
        .arg("stream=index,codec_name,sample_rate:stream_tags=title")
        .arg("-of")
        .arg("json")
        .arg(path)
        .output()
        .map_err(|e| format!("Could not launch ffprobe: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        return Err(format!("ffprobe failed:\n{}", stderr.trim()));
    }

    let probe: ProbeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Could not parse ffprobe output: {e}"))?;

    let tracks = probe
        .streams
        .into_iter()
        .enumerate()
        .map(|(audio_index, stream)| {
            let title = stream
                .tags
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("title"))
                .map(|(_, value)| value.clone())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_default();

            let sample_rate = stream
                .sample_rate
                .as_deref()
                .and_then(|value| value.parse::<u32>().ok());

            DetectedAudioTrack {
                audio_index,
                stream_index: stream.index,
                title,
                codec: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
                sample_rate,
            }
        })
        .collect();

    Ok(tracks)
}
