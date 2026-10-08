use crate::{
    export::ffmpeg::{
        ExportMixTrack, export_mix_wav_with_progress, export_track_wav_with_progress,
        export_video_mix_mkv_with_progress,
    },
    media::{
        audio_mixer::{AudioMixer, MixTrackConfig},
        player::MpvPlayer,
        tracks::probe_audio_tracks,
        video_surface::VideoSurface,
    },
};

use eframe::egui;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
    pub gain_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportMode {
    SeparateTracks,
    MixedAudio,
    VideoWithMix,
}

enum ExportEvent {
    Progress { label: String, fraction: f32 },
    Finished(Result<String, String>),
}

struct ExportProgressState {
    label: String,
    fraction: f32,
}

#[derive(Debug, Clone)]
enum ExportFeedback {
    Success(String),
    Error(String),
}

pub struct SimpleMkvPlayer {
    current_file: Option<PathBuf>,
    tracks: Vec<AudioTrack>,

    player: MpvPlayer,
    video_surface: Option<VideoSurface>,

    audio_mixer: Option<AudioMixer>,

    master_gain_db: f32,
    master_name: String,

    export_window_open: bool,
    export_mode: ExportMode,
    export_track_selection: Vec<bool>,

    export_receiver: Option<Receiver<ExportEvent>>,
    export_progress: Option<ExportProgressState>,
    export_feedback: Option<ExportFeedback>,

    playing: bool,
    position: f64,
    duration: f64,
    video_fullscreen: bool,
    last_video_interaction: Instant,
    video_height_override: Option<f32>,

    status: String,
    last_poll: Instant,
}

impl SimpleMkvPlayer {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut player = MpvPlayer::new();

        let mut status = "Open an MKV file.".to_string();

        let video_surface = match cc.get_proc_address.clone() {
            Some(get_proc_address) => {
                let repaint_context = cc.egui_ctx.clone();

                let request_repaint: Arc<dyn Fn() + Send + Sync + 'static> = Arc::new(move || {
                    repaint_context.request_repaint();
                });

                match player.initialize_render(get_proc_address, request_repaint) {
                    Ok(()) => match player.render_handle() {
                        Some(render) => Some(VideoSurface::new(render)),

                        None => {
                            status = "Video renderer initialized without a render handle.".into();

                            None
                        }
                    },

                    Err(error) => {
                        status = format!("Video initialization failed: {error}");

                        None
                    }
                }
            }

            None => {
                status =
                    "OpenGL renderer unavailable. Simple MKV Player now requires the eframe Glow backend."
                        .into();

                None
            }
        };

        let audio_mixer = match AudioMixer::new() {
            Ok(mixer) => Some(mixer),

            Err(error) => {
                status = format!("Audio mixer initialization failed: {error}");

                None
            }
        };

        Self {
            current_file: None,
            tracks: Vec::new(),

            player,
            video_surface,
            audio_mixer,

            master_gain_db: 0.0,
            master_name: "Master_Mix".to_string(),

            export_window_open: false,
            export_mode: ExportMode::SeparateTracks,
            export_track_selection: Vec::new(),

            export_receiver: None,
            export_progress: None,
            export_feedback: None,

            playing: false,
            position: 0.0,
            duration: 0.0,
            video_fullscreen: false,
            last_video_interaction: Instant::now(),
            video_height_override: None,

            status,
            last_poll: Instant::now(),
        }
    }

    fn set_video_fullscreen(&mut self, ctx: &egui::Context, fullscreen: bool) {
        self.video_fullscreen = fullscreen;

        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(fullscreen));

        ctx.request_repaint();
    }

    fn draw_video_controls(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        fullscreen: bool,
    ) -> bool {
        let pointer_pos = ui.input(|input| input.pointer.hover_pos());

        let pointer_delta = ui.input(|input| input.pointer.delta());

        let pointer_over_video = pointer_pos
            .map(|position| rect.contains(position))
            .unwrap_or(false);

        if pointer_over_video && pointer_delta.length_sq() > 0.0 {
            self.last_video_interaction = Instant::now();
        }

        let controls_visible = pointer_over_video
            && self.last_video_interaction.elapsed() < Duration::from_millis(1800);

        if !controls_visible {
            return false;
        }

        // A paused frame still needs repainting so the controls can
        // disappear after the inactivity timeout.
        ui.ctx().request_repaint_after(Duration::from_millis(100));

        let band_height = if fullscreen { 84.0 } else { 76.0 };

        let band_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), (rect.bottom() - band_height).max(rect.top())),
            rect.right_bottom(),
        );

        // Slight gradient-like effect using two translucent layers.
        ui.painter()
            .rect_filled(band_rect, 0.0, egui::Color32::from_black_alpha(125));

        let lower_band = egui::Rect::from_min_max(
            egui::pos2(band_rect.left(), band_rect.center().y),
            band_rect.right_bottom(),
        );

        ui.painter()
            .rect_filled(lower_band, 0.0, egui::Color32::from_black_alpha(65));

        // ----------------------------------------------------
        // SCRUB RAIL
        // ----------------------------------------------------

        let horizontal_margin = if fullscreen { 48.0 } else { 28.0 };

        let rail_y = band_rect.top() + 18.0;

        let scrub_rect = egui::Rect::from_min_max(
            egui::pos2(band_rect.left() + horizontal_margin, rail_y - 10.0),
            egui::pos2(band_rect.right() - horizontal_margin, rail_y + 10.0),
        );

        let scrub_response = ui.interact(
            scrub_rect,
            ui.make_persistent_id("video_scrub_bar"),
            egui::Sense::click_and_drag(),
        );

        let rail_rect =
            egui::Rect::from_center_size(scrub_rect.center(), egui::vec2(scrub_rect.width(), 4.0));

        ui.painter()
            .rect_filled(rail_rect, 2.0, egui::Color32::from_white_alpha(70));

        let maximum = self.duration.max(0.0);

        let fraction = if maximum > 0.0 {
            (self.position / maximum).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };

        let filled_width = rail_rect.width() * fraction;

        if filled_width > 0.0 {
            let filled_rect = egui::Rect::from_min_max(
                rail_rect.min,
                egui::pos2(rail_rect.left() + filled_width, rail_rect.bottom()),
            );

            ui.painter()
                .rect_filled(filled_rect, 2.0, egui::Color32::WHITE);
        }

        let knob_x = rail_rect.left() + filled_width;

        let knob_radius = if scrub_response.hovered() || scrub_response.dragged() {
            6.0
        } else {
            4.5
        };

        ui.painter().circle_filled(
            egui::pos2(knob_x, rail_rect.center().y),
            knob_radius,
            egui::Color32::WHITE,
        );

        let scrub_is_active = scrub_response.clicked() || scrub_response.dragged();

        if scrub_is_active && maximum > 0.0 {
            if let Some(position) = ui.input(|input| input.pointer.interact_pos()) {
                let fraction =
                    ((position.x - rail_rect.left()) / rail_rect.width()).clamp(0.0, 1.0);

                let target = maximum * fraction as f64;

                self.position = target;
                self.last_video_interaction = Instant::now();

                if scrub_response.clicked() {
                    self.seek_to(target);
                }
            }
        }

        if scrub_response.drag_stopped() && maximum > 0.0 {
            self.seek_to(self.position);
        }

        // ----------------------------------------------------
        // CENTER PLAY / PAUSE
        // ----------------------------------------------------

        let play_center = egui::pos2(band_rect.center().x, band_rect.bottom() - 25.0);

        let play_radius = if fullscreen { 18.0 } else { 16.0 };

        let play_rect = egui::Rect::from_center_size(
            play_center,
            egui::vec2(play_radius * 2.0, play_radius * 2.0),
        );

        let play_response = ui.interact(
            play_rect,
            ui.make_persistent_id("video_play_pause"),
            egui::Sense::click(),
        );

        let play_fill = if play_response.hovered() {
            egui::Color32::from_white_alpha(55)
        } else {
            egui::Color32::from_white_alpha(28)
        };

        ui.painter()
            .circle_filled(play_center, play_radius, play_fill);

        ui.painter().circle_stroke(
            play_center,
            play_radius,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(190)),
        );

        let play_text = if self.playing { "Ⅱ" } else { "▶" };

        ui.painter().text(
            play_center,
            egui::Align2::CENTER_CENTER,
            play_text,
            egui::FontId::proportional(16.0),
            egui::Color32::WHITE,
        );

        if play_response.clicked() {
            self.last_video_interaction = Instant::now();
            self.toggle_playback();
        }

        // ----------------------------------------------------
        // TIME — isolated in the bottom-right so long durations
        // can never collide with the scrub rail.
        // ----------------------------------------------------

        let time_text = format!(
            "{} / {}",
            format_time(self.position),
            format_time(self.duration),
        );

        let time_pos = egui::pos2(band_rect.right() - 16.0, band_rect.bottom() - 25.0);

        ui.painter().text(
            time_pos,
            egui::Align2::RIGHT_CENTER,
            time_text,
            egui::FontId::monospace(12.0),
            egui::Color32::WHITE,
        );

        // The whole bottom band owns pointer input while visible,
        // so video click/double-click gestures don't fire through it.
        pointer_pos
            .map(|position| band_rect.contains(position))
            .unwrap_or(false)
    }

    fn mix_configs(&self) -> Vec<MixTrackConfig> {
        self.tracks
            .iter()
            .map(|track| MixTrackConfig {
                audio_index: track.audio_index,

                gain_db: track.gain_db,

                enabled: track.enabled,
            })
            .collect()
    }

    fn start_mixer_at(&mut self, seconds: f64) -> Result<(), String> {
        let Some(path) = self.current_file.clone() else {
            return Err("No media file loaded.".into());
        };

        let configs = self.mix_configs();

        let Some(mixer) = self.audio_mixer.as_mut() else {
            return Err("Audio mixer unavailable.".into());
        };

        mixer.start_mix(&path, &configs, seconds)
    }

    fn export_mix(&mut self) {
        if self.export_receiver.is_some() {
            self.status = "An export is already running.".into();
            return;
        }

        let Some(input_path) = self.current_file.clone() else {
            self.status = "Open a media file before exporting.".into();
            return;
        };

        let enabled_tracks: Vec<ExportMixTrack> = self
            .tracks
            .iter()
            .filter(|track| track.enabled)
            .map(|track| ExportMixTrack {
                audio_index: track.audio_index,
                gain_db: track.gain_db,
            })
            .collect();

        if enabled_tracks.is_empty() {
            self.status = "Export Mix needs at least one enabled audio track.".into();
            return;
        }

        let stem = source_stem(&input_path);
        let mix_name = safe_name_or_fallback(&self.master_name, "Master_Mix");
        let default_name = format!("{stem}_{mix_name}.wav");

        let Some(output_path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Wave Audio", &["wav"])
            .save_file()
        else {
            return;
        };

        let master_gain_db = self.master_gain_db;
        let duration = self.duration;
        let (sender, receiver) = mpsc::channel();

        self.export_feedback = None;
        self.export_receiver = Some(receiver);
        self.export_progress = Some(ExportProgressState {
            label: "Exporting mix...".into(),
            fraction: 0.0,
        });
        self.status = "Exporting mix...".into();

        thread::spawn(move || {
            let result = export_mix_wav_with_progress(
                &input_path,
                &output_path,
                &enabled_tracks,
                master_gain_db,
                duration,
                |fraction| {
                    let _ = sender.send(ExportEvent::Progress {
                        label: "Exporting mix...".into(),
                        fraction,
                    });
                },
            )
            .map(|()| format!("Exported mix: {}", output_path.display()));

            let _ = sender.send(ExportEvent::Finished(result));
        });
    }

    fn export_video_with_mix(&mut self) {
        if self.export_receiver.is_some() {
            self.status = "An export is already running.".into();
            return;
        }

        let Some(input_path) = self.current_file.clone() else {
            self.status = "Open a media file before exporting.".into();
            return;
        };

        let enabled_tracks: Vec<ExportMixTrack> = self
            .tracks
            .iter()
            .filter(|track| track.enabled)
            .map(|track| ExportMixTrack {
                audio_index: track.audio_index,
                gain_db: track.gain_db,
            })
            .collect();

        if enabled_tracks.is_empty() {
            self.status = "Video export needs at least one enabled audio track.".into();
            return;
        }

        let stem = source_stem(&input_path);
        let mix_name = safe_name_or_fallback(&self.master_name, "Master_Mix");
        let default_name = format!("{stem}_{mix_name}.mkv");

        let Some(output_path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Matroska Video", &["mkv"])
            .save_file()
        else {
            return;
        };

        let master_gain_db = self.master_gain_db;
        let duration = self.duration;
        let audio_title = Some(mix_name);
        let (sender, receiver) = mpsc::channel();

        self.export_feedback = None;
        self.export_receiver = Some(receiver);
        self.export_progress = Some(ExportProgressState {
            label: "Exporting video + current mix...".into(),
            fraction: 0.0,
        });
        self.status = "Exporting video + current mix...".into();

        thread::spawn(move || {
            let result = export_video_mix_mkv_with_progress(
                &input_path,
                &output_path,
                &enabled_tracks,
                master_gain_db,
                duration,
                audio_title.as_deref(),
                |fraction| {
                    let _ = sender.send(ExportEvent::Progress {
                        label: "Exporting video + current mix...".into(),
                        fraction,
                    });
                },
            )
            .map(|()| format!("Exported video + current mix: {}", output_path.display()));

            let _ = sender.send(ExportEvent::Finished(result));
        });
    }

    fn export_track(&mut self, track_index: usize) {
        if self.export_receiver.is_some() {
            self.status = "An export is already running.".into();
            return;
        }

        let Some(input_path) = self.current_file.clone() else {
            self.status = "Open a media file before exporting.".into();
            return;
        };

        let Some(track) = self.tracks.get(track_index) else {
            self.status = "Could not find the requested audio track.".into();
            return;
        };

        let default_name = single_track_export_filename(&input_path, track);

        let Some(output_path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Wave Audio", &["wav"])
            .save_file()
        else {
            return;
        };

        let audio_index = track.audio_index;
        let gain_db = track.gain_db;
        let number = track.number;
        let title = nonempty_metadata(&track.name).map(str::to_owned);
        let duration = self.duration;
        let (sender, receiver) = mpsc::channel();

        self.export_feedback = None;
        self.export_receiver = Some(receiver);
        self.export_progress = Some(ExportProgressState {
            label: format!("Exporting Track {number}..."),
            fraction: 0.0,
        });
        self.status = format!("Exporting Track {number}...");

        thread::spawn(move || {
            let result = export_track_wav_with_progress(
                &input_path,
                &output_path,
                audio_index,
                gain_db,
                title.as_deref(),
                duration,
                |fraction| {
                    let _ = sender.send(ExportEvent::Progress {
                        label: format!("Exporting Track {number}..."),
                        fraction,
                    });
                },
            )
            .map(|()| format!("Exported Track {number}: {}", output_path.display()));

            let _ = sender.send(ExportEvent::Finished(result));
        });
    }

    fn export_selected_tracks(&mut self) {
        if self.export_receiver.is_some() {
            self.status = "An export is already running.".into();
            return;
        }

        let Some(input_path) = self.current_file.clone() else {
            self.status = "Open a media file before exporting.".into();
            return;
        };

        let selected: Vec<(usize, usize, f32, String)> = self
            .export_track_selection
            .iter()
            .enumerate()
            .filter_map(|(index, selected)| selected.then_some(index))
            .filter_map(|index| {
                let track = self.tracks.get(index)?;

                Some((
                    track.number,
                    track.audio_index,
                    track.gain_db,
                    track.name.clone(),
                ))
            })
            .collect();

        if selected.is_empty() {
            self.status = "Select at least one track to export.".into();
            return;
        }

        let Some(output_folder) = rfd::FileDialog::new().pick_folder() else {
            return;
        };

        let duration = self.duration;
        let count = selected.len();
        let source_name = source_stem(&input_path);
        let batch_timestamp = compact_utc_timestamp();
        let (sender, receiver) = mpsc::channel();

        self.export_feedback = None;
        self.export_receiver = Some(receiver);
        self.export_progress = Some(ExportProgressState {
            label: format!("Exporting Track 1 (1/{count})..."),
            fraction: 0.0,
        });
        self.status = format!("Exporting {count} separate WAV tracks...");

        thread::spawn(move || {
            for (position, (number, audio_index, gain_db, name)) in selected.into_iter().enumerate()
            {
                let component = nonempty_metadata(&name)
                    .map(sanitize_filename_component)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| format!("Track_{number}"));

                let filename = format!("{source_name}_{component}_{batch_timestamp}.wav");

                let output_path = unique_output_path(&output_folder.join(filename));

                let title = nonempty_metadata(&name);
                let base = position as f32 / count as f32;
                let span = 1.0 / count as f32;
                let label = format!("Exporting Track {number} ({}/{count})...", position + 1);

                let result = export_track_wav_with_progress(
                    &input_path,
                    &output_path,
                    audio_index,
                    gain_db,
                    title,
                    duration,
                    |track_fraction| {
                        let overall = base + track_fraction * span;

                        let _ = sender.send(ExportEvent::Progress {
                            label: label.clone(),
                            fraction: overall.clamp(0.0, 1.0),
                        });
                    },
                );

                if let Err(error) = result {
                    let _ = sender.send(ExportEvent::Finished(Err(format!(
                        "Batch export stopped on Track {number}: {error}"
                    ))));
                    return;
                }
            }

            let _ = sender.send(ExportEvent::Finished(Ok(format!(
                "Export complete - {count} WAV track{} saved to {}",
                if count == 1 { "" } else { "s" },
                output_folder.display()
            ))));
        });
    }

    fn poll_export(&mut self) {
        let events: Vec<ExportEvent> = self
            .export_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default();

        let mut finished = false;

        for event in events {
            match event {
                ExportEvent::Progress { label, fraction } => {
                    self.export_progress = Some(ExportProgressState {
                        label,
                        fraction: fraction.clamp(0.0, 1.0),
                    });
                }

                ExportEvent::Finished(result) => {
                    match result {
                        Ok(message) => {
                            self.status = message.clone();
                            self.export_feedback = Some(ExportFeedback::Success(message));
                        }

                        Err(error) => {
                            let message = format!("Export failed: {error}");
                            self.status = message.clone();
                            self.export_feedback = Some(ExportFeedback::Error(message));
                        }
                    }

                    self.export_progress = None;
                    finished = true;
                }
            }
        }

        if finished {
            self.export_receiver = None;
        }
    }

    fn stop_mixer(&mut self) {
        if let Some(mixer) = self.audio_mixer.as_mut() {
            mixer.stop_all();
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

        self.stop_mixer();

        self.export_feedback = None;
        self.current_file = Some(path.clone());

        self.tracks.clear();

        self.playing = false;
        self.position = 0.0;
        self.duration = 0.0;

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

                        enabled: i == 0,

                        gain_db: 0.0,
                    })
                    .collect();

                self.export_track_selection = vec![true; self.tracks.len()];
            }

            Err(error) => {
                self.status = format!("Audio probe failed: {error}");
            }
        }

        match self.player.load(&path) {
            Ok(()) => {
                self.status = format!(
                    "Loaded video with {} audio track{}.",
                    self.tracks.len(),
                    if self.tracks.len() == 1 { "" } else { "s" }
                );

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
        if self.last_poll.elapsed() < Duration::from_millis(100) {
            return;
        }

        self.last_poll = Instant::now();

        if !self.player.is_running() {
            if self.playing {
                self.stop_mixer();
            }

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

        if self.playing {
            match self.player.set_paused(true) {
                Ok(()) => {
                    self.stop_mixer();

                    self.playing = false;
                }

                Err(error) => {
                    self.status = format!("Playback control error: {error}");
                }
            }

            return;
        }

        let start = self.position;

        if let Err(error) = self.start_mixer_at(start) {
            self.status = format!("Audio playback error: {error}");

            return;
        }

        match self.player.set_paused(false) {
            Ok(()) => {
                self.playing = true;
            }

            Err(error) => {
                self.stop_mixer();

                self.status = format!("Playback control error: {error}");
            }
        }
    }

    fn seek_to(&mut self, seconds: f64) {
        let max_seek = if self.duration > 0.10 {
            self.duration - 0.05
        } else {
            self.duration
        };

        let seconds = seconds.clamp(0.0, max_seek.max(0.0));

        let was_playing = self.playing;

        if was_playing {
            let _ = self.player.set_paused(true);

            self.stop_mixer();
        }

        match self.player.seek_absolute(seconds) {
            Ok(()) => {
                self.position = seconds;
            }

            Err(error) => {
                self.status = format!("Seek error: {error}");

                if was_playing {
                    let _ = self.player.set_paused(false);
                }

                return;
            }
        }

        if was_playing {
            match self.start_mixer_at(seconds) {
                Ok(()) => {
                    if let Err(error) = self.player.set_paused(false) {
                        self.stop_mixer();

                        self.playing = false;

                        self.status = format!("Playback control error: {error}");
                    }
                }

                Err(error) => {
                    self.playing = false;

                    self.status = format!("Audio seek error: {error}");
                }
            }
        }
    }
}

fn nonempty_metadata(value: &str) -> Option<&str> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn sanitize_filename_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect();

    sanitized.trim().trim_end_matches(['.', ' ']).to_string()
}

fn source_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(sanitize_filename_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Recording".to_string())
}

fn safe_name_or_fallback(value: &str, fallback: &str) -> String {
    nonempty_metadata(value)
        .map(sanitize_filename_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn single_track_export_filename(input_path: &Path, track: &AudioTrack) -> String {
    let source = source_stem(input_path);

    let component = nonempty_metadata(&track.name)
        .map(sanitize_filename_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("Track_{}", track.number));

    format!("{source}_{component}.wav")
}

fn unique_output_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("export");
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");

    for suffix in 2usize.. {
        let filename = if extension.is_empty() {
            format!("{stem}_{suffix}")
        } else {
            format!("{stem}_{suffix}.{extension}")
        };

        let candidate = parent.join(filename);

        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!()
}

fn compact_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);

    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;

    // Gregorian civil date conversion from days since Unix epoch.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };

    if month <= 2 {
        year += 1;
    }

    format!("{year:04}{month:02}{day:02}_{hour:02}{minute:02}{second:02}")
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

fn meter_fraction(db: f32) -> f32 {
    let clamped = db.clamp(-60.0, 0.0);

    (clamped + 60.0) / 60.0
}

fn meter_fill_color(db: f32, enabled: bool, is_post: bool) -> egui::Color32 {
    if !enabled {
        if is_post {
            return egui::Color32::from_rgb(85, 85, 85);
        }

        return egui::Color32::from_rgb(95, 95, 105);
    }

    if is_post {
        if db > -3.0 {
            egui::Color32::from_rgb(220, 70, 65)
        } else if db > -12.0 {
            egui::Color32::from_rgb(220, 185, 55)
        } else {
            egui::Color32::from_rgb(52, 190, 90)
        }
    } else {
        egui::Color32::from_rgb(170, 225, 255)
    }
}

fn draw_single_meter(
    painter: &egui::Painter,
    rect: egui::Rect,
    db: f32,
    enabled: bool,
    is_post: bool,
) {
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(15, 15, 15));

    let fraction = meter_fraction(db);

    let fill_top = rect.bottom() - rect.height() * fraction;

    let fill_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left(), fill_top), rect.right_bottom());

    painter.rect_filled(fill_rect, 2.0, meter_fill_color(db, enabled, is_post));

    painter.line_segment(
        [
            egui::pos2(rect.left(), fill_top),
            egui::pos2(rect.right(), fill_top),
        ],
        egui::Stroke::new(
            2.0,
            if enabled {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(150)
            },
        ),
    );
}

fn draw_dual_db_meters(ui: &mut egui::Ui, pre_peak_db: f32, post_peak_db: f32, enabled: bool) {
    let size = egui::vec2(34.0, 112.0);

    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());

    let painter = ui.painter();

    let gap = 4.0;
    let bar_width = (rect.width() - gap) / 2.0;

    let pre_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + bar_width, rect.bottom()));

    let post_rect =
        egui::Rect::from_min_max(egui::pos2(pre_rect.right() + gap, rect.top()), rect.max);

    draw_single_meter(painter, pre_rect, pre_peak_db, enabled, false);

    draw_single_meter(painter, post_rect, post_peak_db, enabled, true);
}

impl eframe::App for SimpleMkvPlayer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_player();
        self.poll_export();

        if self.playing || self.export_receiver.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }

        let escape_pressed = ui.input(|input| input.key_pressed(egui::Key::Escape));

        if self.video_fullscreen && escape_pressed {
            self.set_video_fullscreen(ui.ctx(), false);
        }

        let f_pressed = ui.input(|input| input.key_pressed(egui::Key::F));

        let keyboard_is_typing = ui.ctx().egui_wants_keyboard_input();

        if f_pressed && !keyboard_is_typing {
            self.set_video_fullscreen(ui.ctx(), !self.video_fullscreen);
        }

        let space_pressed = ui.input(|input| input.key_pressed(egui::Key::Space));

        if space_pressed && !keyboard_is_typing && self.current_file.is_some() {
            self.toggle_playback();
        }

        if self.video_fullscreen {
            let available = ui.available_size();

            let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click());

            ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);

            if self.current_file.is_some() {
                if let Some(surface) = &self.video_surface {
                    surface.paint(ui, rect);
                }
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Open an MKV file",
                    egui::FontId::proportional(20.0),
                    egui::Color32::GRAY,
                );
            }

            let controls_consumed_pointer = self.draw_video_controls(ui, rect, true);

            if !controls_consumed_pointer {
                if response.double_clicked() {
                    self.last_video_interaction = Instant::now();

                    self.set_video_fullscreen(ui.ctx(), false);
                } else if response.clicked() && self.current_file.is_some() {
                    self.last_video_interaction = Instant::now();

                    self.toggle_playback();
                }
            }

            return;
        }

        // ====================================================
        // TOP BAR
        // ====================================================

        ui.horizontal(|ui| {
            if ui.button("Open MKV").clicked() {
                self.open_file();
            }

            let export_button = ui.add_enabled(
                self.current_file.is_some() && self.export_receiver.is_none(),
                egui::Button::new("Export..."),
            );

            if export_button.clicked() {
                if self.export_track_selection.len() != self.tracks.len() {
                    self.export_track_selection = vec![true; self.tracks.len()];
                }

                self.export_window_open = true;
            }

            ui.separator();

            if let Some(path) = &self.current_file {
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
            } else {
                ui.label("No file loaded");
            }
        });

        ui.separator();

        if let Some(progress) = &self.export_progress {
            ui.horizontal(|ui| {
                ui.label(&progress.label);
                let width = (ui.available_width() - 12.0).max(120.0);
                ui.add_sized(
                    [width, 20.0],
                    egui::ProgressBar::new(progress.fraction).show_percentage(),
                );
            });
            ui.add_space(4.0);
        }

        // ====================================================
        // VIDEO
        // ====================================================

        let available_width = ui.available_width();

        let default_video_height = (available_width * 9.0 / 16.0).min(ui.available_height() * 0.40);

        let max_video_height = (ui.available_height() * 0.75).max(180.0);

        let video_height = self
            .video_height_override
            .unwrap_or(default_video_height)
            .clamp(180.0, max_video_height);

        let (rect, video_response) = ui.allocate_exact_size(
            egui::vec2(available_width, video_height),
            egui::Sense::click(),
        );

        ui.painter().rect_filled(rect, 4.0, egui::Color32::BLACK);

        if self.current_file.is_none() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Open an MKV file",
                egui::FontId::proportional(20.0),
                egui::Color32::GRAY,
            );
        }

        if self.current_file.is_some() {
            if let Some(surface) = &self.video_surface {
                surface.paint(ui, rect);
            }

            let controls_consumed_pointer = self.draw_video_controls(ui, rect, false);

            if !controls_consumed_pointer {
                if video_response.double_clicked() {
                    self.last_video_interaction = Instant::now();

                    self.set_video_fullscreen(ui.ctx(), true);
                } else if video_response.clicked() {
                    self.last_video_interaction = Instant::now();

                    self.toggle_playback();
                }
            }
        }

        let (resize_rect, resize_response) =
            ui.allocate_exact_size(egui::vec2(available_width, 9.0), egui::Sense::drag());

        let resize_response = resize_response
            .on_hover_cursor(egui::CursorIcon::ResizeVertical)
            .on_hover_text("Drag to resize the video pane. Double-click to reset.");

        let grip_width = 42.0;
        let grip_y = resize_rect.center().y;

        ui.painter().line_segment(
            [
                egui::pos2(resize_rect.center().x - grip_width / 2.0, grip_y),
                egui::pos2(resize_rect.center().x + grip_width / 2.0, grip_y),
            ],
            egui::Stroke::new(
                2.0,
                if resize_response.hovered() || resize_response.dragged() {
                    egui::Color32::from_gray(145)
                } else {
                    egui::Color32::from_gray(70)
                },
            ),
        );

        if resize_response.double_clicked() {
            self.video_height_override = None;
            ui.ctx().request_repaint();
        } else if resize_response.dragged() {
            let delta_y = ui.input(|input| input.pointer.delta().y);

            let current_height = self.video_height_override.unwrap_or(video_height);

            self.video_height_override =
                Some((current_height + delta_y).clamp(180.0, max_video_height));

            ui.ctx().request_repaint();
        }

        ui.add_space(2.0);

        ui.separator();

        // ====================================================
        // MIXER
        // ====================================================

        let mut export_mix_requested = false;
        let mut export_track_requested: Option<usize> = None;

        let mixer_available_height = ui.available_height().max(80.0);

        egui::ScrollArea::vertical()
            .id_salt("mixer_vertical_scroll")
            .max_height(mixer_available_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Mixer");

                    ui.separator();

                    ui.label(&self.status);
                });

                ui.add_space(4.0);

                let mixer = self.audio_mixer.as_ref();

                let (
                    master_pre_peak_db,
                    _master_pre_rms_db,
                    master_post_peak_db,
                    _master_post_rms_db,
                ) = mixer
                    .map(|mixer| mixer.master_levels_db())
                    .unwrap_or((-60.0, -60.0, -60.0, -60.0));

                egui::ScrollArea::horizontal()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (track_index, track) in self.tracks.iter_mut().enumerate() {
                                let (pre_peak_db, _pre_rms_db, post_peak_db, _post_rms_db) = mixer
                                    .and_then(|mixer| mixer.track_levels_db(track.audio_index))
                                    .unwrap_or((-60.0, -60.0, -60.0, -60.0));

                                ui.group(|ui| {
                                    ui.set_min_width(150.0);
                                    ui.set_max_width(150.0);

                                    ui.vertical_centered(|ui| {
                                        ui.strong(format!("Track {}", track.number));

                                        let name_response = ui.add(
                                            egui::TextEdit::singleline(&mut track.name)
                                                .hint_text("(Track name)")
                                                .desired_width(128.0),
                                        );

                                        let name_chars = track.name.chars().count();

                                        if name_response.has_focus() || name_chars > 64 {
                                            let count_text = format!("{name_chars}/64");

                                            if name_chars > 64 {
                                                ui.colored_label(egui::Color32::YELLOW, count_text);
                                            } else {
                                                ui.small(count_text);
                                            }
                                        } else {
                                            ui.add_space(14.0);
                                        }

                                        let enabled = ui.checkbox(&mut track.enabled, "Enabled");

                                        if enabled.changed() {
                                            if let Some(mixer) = mixer {
                                                mixer.set_track_enabled(
                                                    track.audio_index,
                                                    track.enabled,
                                                );
                                            }
                                        }

                                        ui.add_space(4.0);
                                        ui.small("Fader     Pre  Post");

                                        ui.horizontal(|ui| {
                                            let gain = ui.add_sized(
                                                [28.0, 112.0],
                                                egui::Slider::new(&mut track.gain_db, -60.0..=40.0)
                                                    .vertical()
                                                    .show_value(false),
                                            );

                                            if gain.changed() {
                                                if let Some(mixer) = mixer {
                                                    mixer.set_track_gain_db(
                                                        track.audio_index,
                                                        track.gain_db,
                                                    );
                                                }
                                            }

                                            ui.add_space(6.0);

                                            draw_dual_db_meters(
                                                ui,
                                                pre_peak_db,
                                                post_peak_db,
                                                track.enabled,
                                            );
                                        });

                                        ui.label(format!("{:+.1} dB", track.gain_db));
                                        ui.small(format!("Pre  {:>5.1} dB", pre_peak_db));
                                        ui.small(format!("Post {:>5.1} dB", post_peak_db));

                                        ui.add_space(3.0);

                                        if ui
                                            .add_enabled(
                                                self.export_receiver.is_none(),
                                                egui::Button::new("Export WAV"),
                                            )
                                            .clicked()
                                        {
                                            export_track_requested = Some(track_index);
                                        }

                                        ui.small(format!(
                                            "{} / {} Hz / #{}",
                                            track.codec.to_uppercase(),
                                            track.sample_rate.unwrap_or(0),
                                            track.stream_index
                                        ));
                                    });
                                });

                                ui.add_space(5.0);
                            }

                            // MASTER uses the same visual language as a normal track.
                            ui.group(|ui| {
                                ui.set_min_width(150.0);
                                ui.set_max_width(150.0);

                                ui.vertical_centered(|ui| {
                                    ui.strong("MASTER");

                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.master_name)
                                            .hint_text("Master_Mix")
                                            .desired_width(128.0),
                                    );

                                    ui.add_space(14.0);

                                    ui.add_sized(
                                        [128.0, ui.spacing().interact_size.y],
                                        egui::Label::new("Output Bus"),
                                    );

                                    ui.add_space(4.0);
                                    ui.small("Fader     Pre  Post");

                                    ui.horizontal(|ui| {
                                        let gain = ui.add_sized(
                                            [28.0, 112.0],
                                            egui::Slider::new(
                                                &mut self.master_gain_db,
                                                -60.0..=40.0,
                                            )
                                            .vertical()
                                            .show_value(false),
                                        );

                                        if gain.changed() {
                                            if let Some(mixer) = mixer {
                                                mixer.set_master_gain_db(self.master_gain_db);
                                            }
                                        }

                                        ui.add_space(6.0);

                                        draw_dual_db_meters(
                                            ui,
                                            master_pre_peak_db,
                                            master_post_peak_db,
                                            true,
                                        );
                                    });

                                    ui.label(format!("{:+.1} dB", self.master_gain_db));
                                    ui.small(format!("Pre  {:>5.1} dB", master_pre_peak_db));
                                    ui.small(format!("Post {:>5.1} dB", master_post_peak_db));

                                    ui.add_space(3.0);

                                    if ui
                                        .add_enabled(
                                            self.export_receiver.is_none(),
                                            egui::Button::new("Export Mix"),
                                        )
                                        .clicked()
                                    {
                                        export_mix_requested = true;
                                    }

                                    ui.small("Final output bus");
                                });
                            });
                        });
                    });
            });

        if let Some(track_index) = export_track_requested {
            self.export_track(track_index);
        }

        if export_mix_requested {
            self.export_mix();
        }

        // ====================================================
        // EXPORT WINDOW
        // ====================================================

        let mut export_window_open = self.export_window_open;
        let mut export_separate_requested = false;
        let mut export_dialog_mix_requested = false;
        let mut export_video_mix_requested = false;
        let mut close_export_window_requested = false;

        if export_window_open {
            egui::Window::new("Export")
                .open(&mut export_window_open)
                .resizable(false)
                .collapsible(false)
                .default_width(430.0)
                .show(ui.ctx(), |ui| {
                    ui.label("Choose what you want to export.");

                    ui.add_space(8.0);

                    ui.radio_value(
                        &mut self.export_mode,
                        ExportMode::SeparateTracks,
                        "Separate audio tracks",
                    );

                    ui.radio_value(
                        &mut self.export_mode,
                        ExportMode::MixedAudio,
                        "Mixed audio",
                    );

                    ui.radio_value(
                        &mut self.export_mode,
                        ExportMode::VideoWithMix,
                        "Video + current mix",
                    );

                    ui.separator();

                    match self.export_mode {
                        ExportMode::SeparateTracks => {
                            ui.label(
                                "Exports each selected source as its own 24-bit WAV."
                            );

                            ui.small(
                                "Track selection here is independent of the playback Enabled switches."
                            );

                            ui.add_space(6.0);

                            for (index, track) in self.tracks.iter().enumerate() {
                                if index >= self.export_track_selection.len() {
                                    break;
                                }

                                let title = nonempty_metadata(&track.name)
                                    .unwrap_or("(Track name)");

                                ui.checkbox(
                                    &mut self.export_track_selection[index],
                                    format!("Track {}  -  {title}", track.number),
                                );
                            }
                        }

                        ExportMode::MixedAudio => {
                            ui.label("Exports the current post-master mix as a 24-bit WAV.");

                            ui.small(
                                "What you hear is what you get: enabled tracks, track gains, and master gain."
                            );

                            ui.add_space(6.0);

                            let enabled_count =
                                self.tracks.iter().filter(|track| track.enabled).count();

                            ui.label(format!(
                                "{enabled_count} enabled track{}",
                                if enabled_count == 1 { "" } else { "s" }
                            ));

                            ui.label(format!(
                                "Master gain: {:+.1} dB",
                                self.master_gain_db
                            ));
                        }

                        ExportMode::VideoWithMix => {
                            ui.label(
                                "Exports the original video with the current post-master mix."
                            );

                            ui.add_space(6.0);

                            ui.label("Video:");
                            ui.small("Copy source stream (no video re-encode)");
                            ui.small(
                                "Resolution, frame rate, and video quality are preserved."
                            );

                            ui.add_space(6.0);

                            ui.label("Audio:");
                            ui.small("AAC 320 kbps stereo");
                            ui.small(
                                "Enabled tracks, track gains, and Master gain are applied."
                            );

                            ui.add_space(6.0);

                            let enabled_count =
                                self.tracks.iter().filter(|track| track.enabled).count();

                            ui.label(format!(
                                "{enabled_count} enabled track{}",
                                if enabled_count == 1 { "" } else { "s" }
                            ));

                            ui.label(format!(
                                "Master gain: {:+.1} dB",
                                self.master_gain_db
                            ));
                        }
                    }

                    ui.add_space(10.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close_export_window_requested = true;
                        }

                        let export_label = match self.export_mode {
                            ExportMode::SeparateTracks => "Export WAVs",
                            ExportMode::MixedAudio => "Export Mix",
                            ExportMode::VideoWithMix => "Export Video",
                        };

                        if ui
                            .add_enabled(
                                self.export_receiver.is_none(),
                                egui::Button::new(export_label),
                            )
                            .clicked()
                        {
                            match self.export_mode {
                                ExportMode::SeparateTracks => {
                                    export_separate_requested = true;
                                }

                                ExportMode::MixedAudio => {
                                    export_dialog_mix_requested = true;
                                }

                                ExportMode::VideoWithMix => {
                                    export_video_mix_requested = true;
                                }
                            }
                        }
                    });

                    if let Some(feedback) = &self.export_feedback {
                        ui.add_space(8.0);

                        match feedback {
                            ExportFeedback::Success(message) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(70, 200, 100),
                                    format!("✓ {message}"),
                                );
                            }

                            ExportFeedback::Error(message) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 80, 75),
                                    format!("✕ {message}"),
                                );
                            }
                        }
                    }
                });
        }

        if close_export_window_requested {
            export_window_open = false;
        }

        self.export_window_open = export_window_open;

        if export_separate_requested {
            self.export_selected_tracks();
        }

        if export_dialog_mix_requested {
            self.export_mix();
        }

        if export_video_mix_requested {
            self.export_video_with_mix();
        }
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let (Some(surface), Some(gl)) = (&self.video_surface, gl) {
            surface.destroy(gl);
        }
    }
}
