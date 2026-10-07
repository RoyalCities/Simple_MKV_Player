use crate::media::{player::MpvPlayer, tracks::probe_audio_tracks};

use eframe::egui;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct AudioTrack {
    pub number: usize,
    pub audio_index: usize,
    pub stream_index: usize,
    pub name: String,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub channel_layout: Option<String>,
    pub language: Option<String>,
    pub enabled: bool,
    pub volume: f32,
}

pub struct SimpleMkvPlayer {
    current_file: Option<PathBuf>,
    tracks: Vec<AudioTrack>,

    player: MpvPlayer,

    playing: bool,
    position: f64,
    duration: f64,

    status: String,
    last_poll: Instant,
}

impl SimpleMkvPlayer {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            current_file: None,
            tracks: Vec::new(),

            player: MpvPlayer::new(),

            playing: false,
            position: 0.0,
            duration: 0.0,

            status: "Open an MKV file.".into(),
            last_poll: Instant::now(),
        }
    }

    fn open_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Matroska Video", &["mkv"])
            .add_filter("Video Files", &["mkv", "mp4", "webm"])
            .pick_file()
        else {
            return;
        };

        self.current_file = Some(path.clone());
        self.tracks.clear();

        self.playing = false;
        self.position = 0.0;
        self.duration = 0.0;

        // -----------------------------------------------
        // Detect real audio streams
        // -----------------------------------------------

        match probe_audio_tracks(&path) {
            Ok(detected) => {
                self.tracks = detected
                    .into_iter()
                    .enumerate()
                    .map(|(i, track)| AudioTrack {
                        number: i + 1,
                        audio_index: track.audio_index,
                        stream_index: track.stream_index,
                        name: track.title,
                        codec: track.codec,
                        sample_rate: track.sample_rate,
                        channels: track.channels,
                        channel_layout: track.channel_layout,
                        language: track.language,
                        enabled: true,
                        volume: 1.0,
                    })
                    .collect();
            }

            Err(error) => {
                self.status = format!("Audio probe failed: {error}");
            }
        }

        // -----------------------------------------------
        // Start mpv
        // -----------------------------------------------

        match self.player.load(&path) {
            Ok(()) => {
                self.status = format!(
                    "Loaded video with {} audio track{}.",
                    self.tracks.len(),
                    if self.tracks.len() == 1 { "" } else { "s" }
                );

                // Give mpv a brief chance to finish loading metadata.
                std::thread::sleep(Duration::from_millis(100));

                self.duration = self.player.duration().unwrap_or(0.0);

                self.position = self.player.position().unwrap_or(0.0);

                self.playing = !self.player.paused().unwrap_or(true);
            }

            Err(error) => {
                self.status = format!("Playback error: {error}");
            }
        }
    }

    fn poll_player(&mut self) {
        if self.last_poll.elapsed() < Duration::from_millis(250) {
            return;
        }

        self.last_poll = Instant::now();

        if !self.player.is_running() {
            self.playing = false;
            return;
        }

        if let Ok(position) = self.player.position() {
            self.position = position;
        }

        if let Ok(duration) = self.player.duration() {
            self.duration = duration;
        }

        if let Ok(paused) = self.player.paused() {
            self.playing = !paused;
        }
    }

    fn toggle_playback(&mut self) {
        if !self.player.is_running() {
            return;
        }

        let new_playing = !self.playing;

        match self.player.set_paused(!new_playing) {
            Ok(()) => {
                self.playing = new_playing;
            }

            Err(error) => {
                self.status = format!("Playback control error: {error}");
            }
        }
    }
}

fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00".into();
    }

    let total = seconds.round() as u64;

    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

impl eframe::App for SimpleMkvPlayer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_player();

        // Keep refreshing while playback is active.
        if self.playing {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        // ----------------------------------------------------
        // TOP BAR
        // ----------------------------------------------------

        ui.horizontal(|ui| {
            if ui.button("Open MKV").clicked() {
                self.open_file();
            }

            ui.separator();

            if let Some(path) = &self.current_file {
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
            } else {
                ui.label("No file loaded");
            }
        });

        ui.separator();

        // ----------------------------------------------------
        // VIDEO PLACEHOLDER
        //
        // mpv is deliberately in its own window for this
        // milestone. Embedding comes next.
        // ----------------------------------------------------

        let available_width = ui.available_width();

        let video_height = (available_width * 9.0 / 16.0).min(ui.available_height() * 0.55);

        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available_width, video_height),
            egui::Sense::hover(),
        );

        ui.painter().rect_filled(rect, 4.0, egui::Color32::BLACK);

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if self.current_file.is_some() {
                "Video is currently rendered by mpv"
            } else {
                "Open an MKV file"
            },
            egui::FontId::proportional(20.0),
            egui::Color32::GRAY,
        );

        ui.add_space(8.0);

        // ----------------------------------------------------
        // REAL PLAYBACK CONTROLS
        // ----------------------------------------------------

        ui.horizontal(|ui| {
            let button_text = if self.playing { "Pause" } else { "Play" };

            if ui.button(button_text).clicked() {
                self.toggle_playback();
            }

            let max_duration = if self.duration > 0.0 {
                self.duration
            } else {
                1.0
            };

            let mut slider_position = self.position.clamp(0.0, max_duration);

            let slider_response = ui
                .add(egui::Slider::new(&mut slider_position, 0.0..=max_duration).show_value(false));

            if slider_response.changed() {
                self.position = slider_position;
            }

            if slider_response.drag_stopped() {
                match self.player.seek_absolute(slider_position) {
                    Ok(()) => {
                        self.position = slider_position;
                    }

                    Err(error) => {
                        self.status = format!("Seek error: {error}");
                    }
                }
            }

            ui.label(format!(
                "{} / {}",
                format_time(self.position),
                format_time(self.duration)
            ));
        });

        ui.separator();

        // ----------------------------------------------------
        // AUDIO TRACKS
        // ----------------------------------------------------

        ui.heading("Audio Tracks");

        ui.label(&self.status);

        ui.add_space(5.0);

        if self.tracks.is_empty() && self.current_file.is_some() {
            ui.label("No audio streams detected.");
        }

        for track in &mut self.tracks {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut track.enabled, "");

                    ui.strong(format!("Track {}", track.number));

                    ui.label(&track.name);

                    ui.add_space(10.0);

                    ui.add_enabled(
                        track.enabled,
                        egui::Slider::new(&mut track.volume, 0.0..=1.5).show_value(false),
                    );

                    ui.label(format!("{:.0}%", track.volume * 100.0));

                    if ui.button("Export WAV").clicked() {
                        println!("Export requested: audio stream {}", track.audio_index);
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    ui.add_space(25.0);

                    ui.label(format!("Codec: {}", track.codec.to_uppercase()));

                    ui.separator();

                    if let Some(rate) = track.sample_rate {
                        if rate % 1000 == 0 {
                            ui.label(format!("{} kHz", rate / 1000));
                        } else {
                            ui.label(format!("{} Hz", rate));
                        }

                        ui.separator();
                    }

                    if let Some(channels) = track.channels {
                        let channel_text = match channels {
                            1 => "Mono".to_string(),
                            2 => "Stereo".to_string(),
                            _ => format!("{} channels", channels),
                        };

                        ui.label(channel_text);
                        ui.separator();
                    }

                    if let Some(layout) = &track.channel_layout {
                        ui.label(format!("Layout: {}", layout));

                        ui.separator();
                    }

                    ui.label(format!("MKV stream #{}", track.stream_index));

                    if let Some(language) = &track.language {
                        ui.separator();

                        ui.label(format!("Language: {}", language));
                    }
                });
            });

            ui.add_space(4.0);
        }

        if !self.tracks.is_empty() {
            ui.add_space(5.0);

            if ui.button("Export All Tracks").clicked() {
                println!("Export all requested");
            }
        }
    }
}
