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
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
    pub language: Option<String>,
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
    channels: Option<u32>,

    #[serde(default)]
    channel_layout: Option<String>,

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
        .arg("stream=index,codec_name,sample_rate,channels,channel_layout:stream_tags=title,language")
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
                .get("title")
                .cloned()
                .unwrap_or_else(|| format!("Audio Track {}", audio_index + 1));

            let language = stream.tags.get("language").cloned();

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
                channels: stream.channels,
                channel_layout: stream.channel_layout,
                language,
            }
        })
        .collect();

    Ok(tracks)
}
