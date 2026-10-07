use cpal::{
    SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct MixTrackConfig {
    pub audio_index: usize,
    pub volume: f32,
    pub enabled: bool,
}

struct OutputTrack {
    queue: Arc<Mutex<VecDeque<f32>>>,

    volume_bits: Arc<AtomicU32>,

    enabled: Arc<AtomicBool>,
}

struct DecoderRuntime {
    child: Child,

    thread: Option<JoinHandle<()>>,

    stop: Arc<AtomicBool>,

    queue: Arc<Mutex<VecDeque<f32>>>,

    volume_bits: Arc<AtomicU32>,

    enabled: Arc<AtomicBool>,
}

pub struct AudioMixer {
    _stream: Stream,

    output_tracks: Arc<Mutex<Vec<OutputTrack>>>,

    output_enabled: Arc<AtomicBool>,

    decoders: HashMap<usize, DecoderRuntime>,

    sample_rate: u32,
    channels: u16,
}

impl AudioMixer {
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .ok_or_else(|| "Windows has no default audio output device.".to_string())?;

        let supported = device
            .default_output_config()
            .map_err(|e| format!("Could not query default audio output configuration: {e}"))?;

        let sample_format = supported.sample_format();

        let config: cpal::StreamConfig = supported.into();

        let sample_rate = config.sample_rate.0;

        let channels = config.channels;

        let output_tracks = Arc::new(Mutex::new(Vec::<OutputTrack>::new()));

        let output_enabled = Arc::new(AtomicBool::new(false));

        let error_callback = |error| {
            eprintln!("Audio output error: {error}");
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                let tracks = Arc::clone(&output_tracks);

                let enabled = Arc::clone(&output_enabled);

                device.build_output_stream(
                    &config,
                    move |output: &mut [f32], _| {
                        write_f32(output, &tracks, &enabled);
                    },
                    error_callback,
                    None,
                )
            }

            SampleFormat::I16 => {
                let tracks = Arc::clone(&output_tracks);

                let enabled = Arc::clone(&output_enabled);

                device.build_output_stream(
                    &config,
                    move |output: &mut [i16], _| {
                        write_i16(output, &tracks, &enabled);
                    },
                    error_callback,
                    None,
                )
            }

            SampleFormat::U16 => {
                let tracks = Arc::clone(&output_tracks);

                let enabled = Arc::clone(&output_enabled);

                device.build_output_stream(
                    &config,
                    move |output: &mut [u16], _| {
                        write_u16(output, &tracks, &enabled);
                    },
                    error_callback,
                    None,
                )
            }

            other => {
                return Err(format!("Unsupported output format: {other:?}"));
            }
        }
        .map_err(|e| format!("Could not create Windows output stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Could not start Windows audio output: {e}"))?;

        Ok(Self {
            _stream: stream,

            output_tracks,
            output_enabled,

            decoders: HashMap::new(),

            sample_rate,
            channels,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn start_mix(
        &mut self,
        path: &Path,
        tracks: &[MixTrackConfig],
        start_seconds: f64,
    ) -> Result<(), String> {
        self.stop_all();

        self.output_enabled.store(false, Ordering::Relaxed);

        for config in tracks {
            self.start_decoder(path, config, start_seconds)?;
        }

        self.rebuild_output_tracks()?;

        // Let every decoder build a small buffer before the
        // output callback begins consuming samples.
        let minimum_samples = (self.sample_rate as usize * self.channels as usize) / 10;

        let deadline = Instant::now() + Duration::from_secs(3);

        loop {
            let mut ready = true;

            for decoder in self.decoders.values() {
                let len = decoder
                    .queue
                    .lock()
                    .map_err(|_| "Audio queue lock poisoned.".to_string())?
                    .len();

                if len < minimum_samples {
                    ready = false;
                    break;
                }
            }

            if ready || Instant::now() >= deadline {
                break;
            }

            thread::sleep(Duration::from_millis(10));
        }

        self.output_enabled.store(true, Ordering::Relaxed);

        Ok(())
    }

    fn start_decoder(
        &mut self,
        path: &Path,
        config: &MixTrackConfig,
        start_seconds: f64,
    ) -> Result<(), String> {
        let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));

        let stop = Arc::new(AtomicBool::new(false));

        let enabled = Arc::new(AtomicBool::new(config.enabled));

        let volume_bits = Arc::new(AtomicU32::new(config.volume.clamp(0.0, 2.0).to_bits()));

        let map = format!("0:a:{}", config.audio_index);

        let start = start_seconds.max(0.0).to_string();

        let mut child = Command::new("ffmpeg")
            .arg("-nostdin")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-ss")
            .arg(start)
            .arg("-i")
            .arg(path)
            .arg("-map")
            .arg(map)
            .arg("-vn")
            .arg("-sn")
            .arg("-dn")
            .arg("-f")
            .arg("f32le")
            .arg("-acodec")
            .arg("pcm_f32le")
            .arg("-ar")
            .arg(self.sample_rate.to_string())
            .arg("-ac")
            .arg(self.channels.to_string())
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                format!(
                    "Could not launch ffmpeg for audio track {}: {e}",
                    config.audio_index
                )
            })?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Could not capture ffmpeg PCM output.".to_string())?;

        let thread_queue = Arc::clone(&queue);

        let thread_stop = Arc::clone(&stop);

        let max_buffer_samples = self.sample_rate as usize * self.channels as usize * 2;

        let decoder_thread = thread::spawn(move || {
            let mut read_buffer = [0_u8; 16384];

            let mut carry: Vec<u8> = Vec::new();

            loop {
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }

                while thread_queue
                    .lock()
                    .map(|queue| queue.len() >= max_buffer_samples)
                    .unwrap_or(true)
                {
                    if thread_stop.load(Ordering::Relaxed) {
                        return;
                    }

                    thread::sleep(Duration::from_millis(5));
                }

                let read = match stdout.read(&mut read_buffer) {
                    Ok(0) => break,
                    Ok(read) => read,
                    Err(_) => break,
                };

                let mut bytes = Vec::with_capacity(carry.len() + read);

                bytes.extend_from_slice(&carry);

                bytes.extend_from_slice(&read_buffer[..read]);

                let complete = bytes.len() / 4 * 4;

                carry.clear();

                if complete < bytes.len() {
                    carry.extend_from_slice(&bytes[complete..]);
                }

                if let Ok(mut queue) = thread_queue.lock() {
                    for chunk in bytes[..complete].chunks_exact(4) {
                        queue.push_back(f32::from_le_bytes([
                            chunk[0], chunk[1], chunk[2], chunk[3],
                        ]));
                    }
                } else {
                    break;
                }
            }
        });

        self.decoders.insert(
            config.audio_index,
            DecoderRuntime {
                child,
                thread: Some(decoder_thread),
                stop,
                queue,
                volume_bits,
                enabled,
            },
        );

        Ok(())
    }

    fn rebuild_output_tracks(&self) -> Result<(), String> {
        let mut output = self
            .output_tracks
            .lock()
            .map_err(|_| "Output track lock poisoned.".to_string())?;

        output.clear();

        for decoder in self.decoders.values() {
            output.push(OutputTrack {
                queue: Arc::clone(&decoder.queue),

                volume_bits: Arc::clone(&decoder.volume_bits),

                enabled: Arc::clone(&decoder.enabled),
            });
        }

        Ok(())
    }

    pub fn set_track_volume(&self, audio_index: usize, volume: f32) {
        if let Some(decoder) = self.decoders.get(&audio_index) {
            decoder
                .volume_bits
                .store(volume.clamp(0.0, 2.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_track_enabled(&self, audio_index: usize, enabled: bool) {
        if let Some(decoder) = self.decoders.get(&audio_index) {
            decoder.enabled.store(enabled, Ordering::Relaxed);
        }
    }

    pub fn stop_all(&mut self) {
        self.output_enabled.store(false, Ordering::Relaxed);

        if let Ok(mut output) = self.output_tracks.lock() {
            output.clear();
        }

        for decoder in self.decoders.values() {
            decoder.stop.store(true, Ordering::Relaxed);
        }

        for decoder in self.decoders.values_mut() {
            let _ = decoder.child.kill();

            let _ = decoder.child.wait();

            if let Some(thread) = decoder.thread.take() {
                let _ = thread.join();
            }
        }

        self.decoders.clear();
    }
}

impl Drop for AudioMixer {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn mix_samples(
    count: usize,
    tracks: &Arc<Mutex<Vec<OutputTrack>>>,
    output_enabled: &Arc<AtomicBool>,
) -> Vec<f32> {
    if !output_enabled.load(Ordering::Relaxed) {
        return vec![0.0; count];
    }

    let Ok(tracks) = tracks.lock() else {
        return vec![0.0; count];
    };

    let mut mixed = vec![0.0_f32; count];

    for track in tracks.iter() {
        let enabled = track.enabled.load(Ordering::Relaxed);

        let volume = f32::from_bits(track.volume_bits.load(Ordering::Relaxed));

        let Ok(mut queue) = track.queue.lock() else {
            continue;
        };

        for sample in mixed.iter_mut() {
            // Always consume samples, even while muted,
            // otherwise re-enabling a track would play
            // old buffered audio.
            let value = queue.pop_front().unwrap_or(0.0);

            if enabled {
                *sample += value * volume;
            }
        }
    }

    for sample in mixed.iter_mut() {
        *sample = sample.clamp(-1.0, 1.0);
    }

    mixed
}

fn write_f32(output: &mut [f32], tracks: &Arc<Mutex<Vec<OutputTrack>>>, enabled: &Arc<AtomicBool>) {
    let mixed = mix_samples(output.len(), tracks, enabled);

    output.copy_from_slice(&mixed);
}

fn write_i16(output: &mut [i16], tracks: &Arc<Mutex<Vec<OutputTrack>>>, enabled: &Arc<AtomicBool>) {
    let mixed = mix_samples(output.len(), tracks, enabled);

    for (destination, sample) in output.iter_mut().zip(mixed.iter()) {
        *destination = (*sample * i16::MAX as f32) as i16;
    }
}

fn write_u16(output: &mut [u16], tracks: &Arc<Mutex<Vec<OutputTrack>>>, enabled: &Arc<AtomicBool>) {
    let mixed = mix_samples(output.len(), tracks, enabled);

    for (destination, sample) in output.iter_mut().zip(mixed.iter()) {
        *destination = ((*sample * 0.5 + 0.5) * u16::MAX as f32) as u16;
    }
}
