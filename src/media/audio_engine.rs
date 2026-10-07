use cpal::{
    SampleFormat, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use std::{
    collections::VecDeque,
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct AudioEngine {
    _stream: Stream,

    queue: Arc<Mutex<VecDeque<f32>>>,

    volume_bits: Arc<AtomicU32>,

    sample_rate: u32,
    channels: u16,

    decoder_child: Option<Child>,

    decoder_thread: Option<JoinHandle<()>>,

    decoder_stop: Option<Arc<AtomicBool>>,

    current_track: Option<usize>,
}

impl AudioEngine {
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

        let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));

        let volume_bits = Arc::new(AtomicU32::new(1.0_f32.to_bits()));

        let error_callback = |error| {
            eprintln!("Audio output error: {error}");
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                let queue = Arc::clone(&queue);

                let volume = Arc::clone(&volume_bits);

                device.build_output_stream(
                    &config,
                    move |output: &mut [f32], _| {
                        write_f32(output, &queue, &volume);
                    },
                    error_callback,
                    None,
                )
            }

            SampleFormat::I16 => {
                let queue = Arc::clone(&queue);

                let volume = Arc::clone(&volume_bits);

                device.build_output_stream(
                    &config,
                    move |output: &mut [i16], _| {
                        write_i16(output, &queue, &volume);
                    },
                    error_callback,
                    None,
                )
            }

            SampleFormat::U16 => {
                let queue = Arc::clone(&queue);

                let volume = Arc::clone(&volume_bits);

                device.build_output_stream(
                    &config,
                    move |output: &mut [u16], _| {
                        write_u16(output, &queue, &volume);
                    },
                    error_callback,
                    None,
                )
            }

            other => {
                return Err(format!(
                    "Unsupported Windows audio sample format: {other:?}"
                ));
            }
        }
        .map_err(|e| format!("Could not create audio output stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Could not start Windows audio output: {e}"))?;

        Ok(Self {
            _stream: stream,

            queue,
            volume_bits,

            sample_rate,
            channels,

            decoder_child: None,
            decoder_thread: None,
            decoder_stop: None,

            current_track: None,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn current_track(&self) -> Option<usize> {
        self.current_track
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 2.0);

        self.volume_bits.store(volume.to_bits(), Ordering::Relaxed);
    }

    pub fn start_track(
        &mut self,
        path: &Path,
        audio_index: usize,
        start_seconds: f64,
    ) -> Result<(), String> {
        self.stop();

        {
            let mut queue = self
                .queue
                .lock()
                .map_err(|_| "Audio queue lock was poisoned.".to_string())?;

            queue.clear();
        }

        let start = start_seconds.max(0.0).to_string();

        let sample_rate = self.sample_rate.to_string();

        let channels = self.channels.to_string();

        let map = format!("0:a:{audio_index}");

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
            .arg(sample_rate)
            .arg("-ac")
            .arg(channels)
            .arg("pipe:1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not launch ffmpeg audio decoder: {e}"))?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Could not capture ffmpeg PCM output.".to_string())?;

        let queue = Arc::clone(&self.queue);

        let stop = Arc::new(AtomicBool::new(false));

        let thread_stop = Arc::clone(&stop);

        let max_buffer_samples = self.sample_rate as usize * self.channels as usize * 2;

        let decoder_thread = thread::spawn(move || {
            let mut buffer = [0_u8; 16384];

            let mut carry: Vec<u8> = Vec::with_capacity(3);

            while !thread_stop.load(Ordering::Relaxed) {
                let read = match stdout.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => read,

                    Err(_) => break,
                };

                let mut bytes = Vec::with_capacity(carry.len() + read);

                bytes.extend_from_slice(&carry);

                bytes.extend_from_slice(&buffer[..read]);

                let whole_bytes = bytes.len() / 4 * 4;

                carry.clear();

                if whole_bytes < bytes.len() {
                    carry.extend_from_slice(&bytes[whole_bytes..]);
                }

                let samples = bytes[..whole_bytes]
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect::<Vec<_>>();

                loop {
                    if thread_stop.load(Ordering::Relaxed) {
                        return;
                    }

                    let queue_len = match queue.lock() {
                        Ok(queue) => queue.len(),

                        Err(_) => {
                            return;
                        }
                    };

                    if queue_len < max_buffer_samples {
                        break;
                    }

                    thread::sleep(Duration::from_millis(5));
                }

                match queue.lock() {
                    Ok(mut queue) => {
                        queue.extend(samples);
                    }

                    Err(_) => {
                        return;
                    }
                }
            }
        });

        self.decoder_child = Some(child);

        self.decoder_thread = Some(decoder_thread);

        self.decoder_stop = Some(stop);

        self.current_track = Some(audio_index);

        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.decoder_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }

        if let Some(child) = self.decoder_child.as_mut() {
            let _ = child.kill();

            let _ = child.wait();
        }

        self.decoder_child = None;

        if let Some(thread) = self.decoder_thread.take() {
            let _ = thread.join();
        }

        if let Ok(mut queue) = self.queue.lock() {
            queue.clear();
        }

        self.current_track = None;
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn volume_value(volume: &AtomicU32) -> f32 {
    f32::from_bits(volume.load(Ordering::Relaxed))
}

fn next_sample(queue: &Arc<Mutex<VecDeque<f32>>>) -> f32 {
    match queue.lock() {
        Ok(mut queue) => queue.pop_front().unwrap_or(0.0),

        Err(_) => 0.0,
    }
}

fn write_f32(output: &mut [f32], queue: &Arc<Mutex<VecDeque<f32>>>, volume: &Arc<AtomicU32>) {
    let volume = volume_value(volume);

    if let Ok(mut queue) = queue.lock() {
        for sample in output {
            let value = queue.pop_front().unwrap_or(0.0);

            *sample = value * volume;
        }
    } else {
        output.fill(0.0);
    }
}

fn write_i16(output: &mut [i16], queue: &Arc<Mutex<VecDeque<f32>>>, volume: &Arc<AtomicU32>) {
    let volume = volume_value(volume);

    if let Ok(mut queue) = queue.lock() {
        for sample in output {
            let value = queue.pop_front().unwrap_or(0.0) * volume;

            *sample = (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        }
    } else {
        output.fill(0);
    }
}

fn write_u16(output: &mut [u16], queue: &Arc<Mutex<VecDeque<f32>>>, volume: &Arc<AtomicU32>) {
    let volume = volume_value(volume);

    if let Ok(mut queue) = queue.lock() {
        for sample in output {
            let value = queue.pop_front().unwrap_or(0.0) * volume;

            let value = value.clamp(-1.0, 1.0);

            *sample = ((value * 0.5 + 0.5) * u16::MAX as f32) as u16;
        }
    } else {
        output.fill(u16::MAX / 2);
    }
}
