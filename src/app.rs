use crate::media::tracks::probe_audio_tracks;
use eframe::egui;
use std::path::PathBuf;

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
    playing: bool,
    position: f32,
    status: String,
}

impl SimpleMkvPlayer {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            current_file: None,
            tracks: Vec::new(),
            playing: false,
            position: 0.0,
            status: "Open an MKV file.".into(),
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

                        // Start everything enabled for now.
                        // Once the real mixer exists, these become live controls.
                        enabled: true,
                        volume: 1.0,
                    })
                    .collect();

                self.status = format!(
                    "Detected {} audio track{}.",
                    self.tracks.len(),
                    if self.tracks.len() == 1 { "" } else { "s" }
                );
            }

            Err(error) => {
                self.status = error;
            }
        }
    }
}

impl eframe::App for SimpleMkvPlayer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // ----------------------------------------------------
        // TOP BAR
        // ----------------------------------------------------

        ui.horizontal(|ui| {
            if ui.button("Open MKV").clicked() {
                self.open_file();
            }

            ui.separator();

            if let Some(path) = &self.current_file {
                ui.label(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                );
            } else {
                ui.label("No file loaded");
            }
        });

        ui.separator();

        // ----------------------------------------------------
        // VIDEO PLACEHOLDER
        // ----------------------------------------------------

        let available_width = ui.available_width();

        let video_height = (available_width * 9.0 / 16.0)
            .min(ui.available_height() * 0.55);

        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(available_width, video_height),
            egui::Sense::hover(),
        );

        ui.painter().rect_filled(
            rect,
            4.0,
            egui::Color32::BLACK,
        );

        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if self.current_file.is_some() {
                "Video output"
            } else {
                "Open an MKV file"
            },
            egui::FontId::proportional(20.0),
            egui::Color32::GRAY,
        );

        ui.add_space(8.0);

        // ----------------------------------------------------
        // PLAYBACK PLACEHOLDER
        // ----------------------------------------------------

        ui.horizontal(|ui| {
            let button_text = if self.playing {
                "Pause"
            } else {
                "Play"
            };

            if ui.button(button_text).clicked() {
                self.playing = !self.playing;
            }

            ui.add(
                egui::Slider::new(
                    &mut self.position,
                    0.0..=100.0,
                )
                .show_value(false),
            );

            ui.label(format!("{:.0}%", self.position));
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

                    ui.strong(format!(
                        "Track {}",
                        track.number
                    ));

                    ui.label(&track.name);

                    ui.add_space(10.0);

                    ui.add_enabled(
                        track.enabled,
                        egui::Slider::new(
                            &mut track.volume,
                            0.0..=1.5,
                        )
                        .show_value(false),
                    );

                    ui.label(format!(
                        "{:.0}%",
                        track.volume * 100.0
                    ));

                    if ui.button("Export WAV").clicked() {
                        println!(
                            "Export requested: audio stream {}",
                            track.audio_index
                        );
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    ui.add_space(25.0);

                    ui.label(format!(
                        "Codec: {}",
                        track.codec.to_uppercase()
                    ));

                    ui.separator();

                    if let Some(rate) = track.sample_rate {
                        if rate % 1000 == 0 {
                            ui.label(format!(
                                "{} kHz",
                                rate / 1000
                            ));
                        } else {
                            ui.label(format!(
                                "{} Hz",
                                rate
                            ));
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
                        ui.label(format!(
                            "Layout: {}",
                            layout
                        ));

                        ui.separator();
                    }

                    ui.label(format!(
                        "MKV stream #{}",
                        track.stream_index
                    ));

                    if let Some(language) = &track.language {
                        ui.separator();

                        ui.label(format!(
                            "Language: {}",
                            language
                        ));
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
