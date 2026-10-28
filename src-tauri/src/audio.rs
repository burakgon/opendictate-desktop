//! Microphone capture, level metering, silence trimming, WAV encoding and UI sounds.

use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SizedSample,
};
use parking_lot::Mutex;

pub const TARGET_RATE: u32 = 16_000;

/// Raw mono audio as captured from the device.
pub struct Captured {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Captured {
    pub fn duration_secs(&self) -> f32 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.samples.len() as f32 / self.sample_rate as f32
        }
    }
}

struct Active {
    stop_tx: mpsc::Sender<()>,
    thread: thread::JoinHandle<Result<Captured>>,
}

/// Records from an input device on a dedicated thread (cpal streams are not `Send`).
pub struct Recorder {
    active: Mutex<Option<Active>>,
    /// Latest RMS level (f32 bits), polled by the UI.
    pub level: Arc<AtomicU32>,
}

impl Recorder {
    pub fn new() -> Self {
        Self { active: Mutex::new(None), level: Arc::new(AtomicU32::new(0)) }
    }

    pub fn is_recording(&self) -> bool {
        self.active.lock().is_some()
    }

    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Opens the device and starts capturing. Returns once the stream is running.
    pub fn start(&self, device_name: &str) -> Result<()> {
        let mut guard = self.active.lock();
        if guard.is_some() {
            return Ok(());
        }
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let level = self.level.clone();
        level.store(0f32.to_bits(), Ordering::Relaxed);
        let device_name = device_name.to_string();

        let thread = thread::Builder::new()
            .name("od-capture".into())
            .spawn(move || capture_thread(&device_name, level, stop_rx, ready_tx))
            .context("could not spawn the capture thread")?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                *guard = Some(Active { stop_tx, thread });
                Ok(())
            }
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = stop_tx.send(());
                Err(anyhow!("the microphone did not start in time"))
            }
        }
    }

    /// Stops capturing and returns the audio.
    pub fn stop(&self) -> Result<Captured> {
        let active = self.active.lock().take().ok_or_else(|| anyhow!("not recording"))?;
        let _ = active.stop_tx.send(());
        let captured = active.thread.join().map_err(|_| anyhow!("capture thread panicked"))??;
        self.level.store(0f32.to_bits(), Ordering::Relaxed);
        Ok(captured)
    }

    /// Stops capturing and throws the audio away.
    pub fn cancel(&self) {
        if let Some(active) = self.active.lock().take() {
            let _ = active.stop_tx.send(());
            let _ = active.thread.join();
        }
        self.level.store(0f32.to_bits(), Ordering::Relaxed);
    }
}

/// Prefix for PipeWire/PulseAudio sources, recorded through the `pulse` ALSA device.
const PULSE_PREFIX: &str = "pulse:";

fn find_input_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    let mut wanted = name.to_string();
    #[cfg(target_os = "linux")]
    {
        if let Some(source) = name.strip_prefix(PULSE_PREFIX) {
            // libpulse picks the source from PULSE_SOURCE when the ALSA `pulse` device opens.
            std::env::set_var("PULSE_SOURCE", source);
            wanted = "pulse".into();
        } else {
            std::env::remove_var("PULSE_SOURCE");
        }
    }
    if !wanted.is_empty() {
        if let Ok(devices) = host.input_devices() {
            for d in devices {
                if d.name().map(|n| n == wanted).unwrap_or(false) {
                    return Ok(d);
                }
            }
        }
        log::warn!("input device '{name}' not found, falling back to the default device");
    }
    host.default_input_device().ok_or_else(|| anyhow!("no microphone found"))
}

fn capture_thread(
    device_name: &str,
    level: Arc<AtomicU32>,
    stop_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::Sender<Result<()>>,
) -> Result<Captured> {
    type Running = (cpal::Stream, Arc<Mutex<Vec<f32>>>, u32);
    let setup = || -> Result<Running> {
        let device = find_input_device(device_name)?;
        let config = device.default_input_config().context("the microphone has no usable configuration")?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();
        let buffer = Arc::new(Mutex::new(Vec::<f32>::with_capacity(sample_rate as usize * 30)));
        log::info!(
            "recording from '{}' ({} Hz, {} ch, {:?})",
            device.name().unwrap_or_default(),
            sample_rate,
            channels,
            format
        );

        let stream = match format {
            SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            SampleFormat::I32 => build_stream::<i32>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            SampleFormat::I8 => build_stream::<i8>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            SampleFormat::U8 => build_stream::<u8>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            SampleFormat::F64 => build_stream::<f64>(&device, &stream_config, channels, buffer.clone(), level.clone()),
            other => Err(anyhow!("unsupported sample format {other:?}")),
        }?;
        stream.play().context("could not start the microphone stream")?;
        Ok((stream, buffer, sample_rate))
    };

    let (stream, buffer, sample_rate) = match setup() {
        Ok(v) => {
            let _ = ready_tx.send(Ok(()));
            v
        }
        Err(e) => {
            let _ = ready_tx.send(Err(anyhow!("{e:#}")));
            return Err(e);
        }
    };

    // Block until asked to stop (or the engine goes away).
    let _ = stop_rx.recv();
    drop(stream);
    let samples = std::mem::take(&mut *buffer.lock());
    Ok(Captured { samples, sample_rate })
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    buffer: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let meter_window = (config.sample_rate.0 / 30).max(1) as usize;
    let mut sum_sq = 0f32;
    let mut count = 0usize;
    let channels = channels.max(1);
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut buf = buffer.lock();
            for frame in data.chunks(channels) {
                let mut acc = 0f32;
                for s in frame {
                    acc += <f32 as cpal::FromSample<T>>::from_sample_(*s);
                }
                let v = acc / frame.len() as f32;
                buf.push(v);
                sum_sq += v * v;
                count += 1;
                if count >= meter_window {
                    let rms = (sum_sq / count as f32).sqrt();
                    level.store(rms.to_bits(), Ordering::Relaxed);
                    sum_sq = 0.0;
                    count = 0;
                }
            }
        },
        |err| log::error!("audio stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InputDevice {
    pub id: String,
    pub label: String,
}

/// Lists microphones. On Linux with PipeWire/PulseAudio these are the real sources (not ALSA
/// plugin names); elsewhere the audio host's input devices.
pub fn list_input_devices() -> Vec<InputDevice> {
    #[cfg(target_os = "linux")]
    {
        if let Some(sources) = pulse_sources() {
            if !sources.is_empty() {
                return sources;
            }
        }
    }
    let host = cpal::default_host();
    let mut out: Vec<InputDevice> = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for d in devices {
            let Ok(n) = d.name() else { continue };
            #[cfg(target_os = "linux")]
            {
                // Hide ALSA plugin pseudo-devices nobody wants to pick.
                let useful = n == "pipewire" || n == "pulse" || ["sysdefault", "hw:", "plughw:", "front:"].iter().any(|p| n.starts_with(p));
                if !useful {
                    continue;
                }
            }
            if n != "default" && !out.iter().any(|x| x.id == n) {
                out.push(InputDevice { label: n.clone(), id: n });
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn pulse_sources() -> Option<Vec<InputDevice>> {
    let output = std::process::Command::new("pactl").args(["-f", "json", "list", "sources"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let list: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).ok()?;
    Some(
        list.iter()
            .filter(|s| {
                s.get("monitor_source").and_then(|m| m.as_str()).map(|m| m.is_empty()).unwrap_or(true)
                    && s.pointer("/properties/device.class").and_then(|c| c.as_str()) != Some("monitor")
            })
            .filter_map(|s| {
                let name = s.get("name")?.as_str()?;
                let label = s.get("description").and_then(|d| d.as_str()).unwrap_or(name);
                Some(InputDevice { id: format!("{PULSE_PREFIX}{name}"), label: label.to_string() })
            })
            .collect(),
    )
}

/// Low-pass + resample to 16 kHz using a box filter matched to the decimation ratio.
pub fn resample_to_16k(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate == TARGET_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = rate as f64 / TARGET_RATE as f64;
    let out_len = (samples.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    let half = (ratio / 2.0).max(0.5);
    for i in 0..out_len {
        let center = i as f64 * ratio;
        if ratio > 1.0 {
            let start = (center - half).max(0.0) as usize;
            let end = ((center + half).ceil() as usize).min(samples.len());
            let slice = &samples[start..end.max(start + 1).min(samples.len())];
            out.push(slice.iter().sum::<f32>() / slice.len().max(1) as f32);
        } else {
            // Upsampling: linear interpolation.
            let idx = center.floor() as usize;
            let frac = (center - idx as f64) as f32;
            let a = samples[idx.min(samples.len() - 1)];
            let b = samples[(idx + 1).min(samples.len() - 1)];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Result of analysing a 16 kHz recording.
pub struct Analysis {
    pub has_speech: bool,
    pub start: usize,
    pub end: usize,
}

/// Finds the region containing speech using frame energy relative to the noise floor.
pub fn analyse(samples: &[f32]) -> Analysis {
    const FRAME: usize = 320; // 20 ms at 16 kHz
    if samples.len() < FRAME * 5 {
        return Analysis { has_speech: false, start: 0, end: samples.len() };
    }
    let rms: Vec<f32> = samples
        .chunks(FRAME)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[sorted.len() / 7];
    let peak = *sorted.last().unwrap_or(&0.0);
    let threshold = (floor * 3.0).max(0.0025);
    let loud: Vec<usize> = rms.iter().enumerate().filter(|(_, v)| **v > threshold).map(|(i, _)| i).collect();
    // Require ~100 ms of energy above the floor before we call it speech.
    if peak < 0.002 || loud.len() < 5 {
        return Analysis { has_speech: false, start: 0, end: samples.len() };
    }
    let pad = 15; // 300 ms
    let first = loud[0].saturating_sub(pad);
    let last = (loud[loud.len() - 1] + pad + 1).min(rms.len());
    Analysis { has_speech: true, start: first * FRAME, end: (last * FRAME).min(samples.len()) }
}

/// Encodes mono f32 samples as 16-bit PCM WAV, normalising quiet recordings.
pub fn encode_wav(samples: &[f32], rate: u32) -> Result<Vec<u8>> {
    let peak = samples.iter().fold(0f32, |m, v| m.max(v.abs()));
    // Gentle normalisation: bring the peak to ~-1 dBFS, at most 8x gain.
    let gain = if peak > 0.0 { (0.89 / peak).clamp(1.0, 8.0) } else { 1.0 };
    let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut cursor = Cursor::new(Vec::with_capacity(samples.len() * 2 + 44));
    {
        let mut w = hound::WavWriter::new(&mut cursor, spec)?;
        for s in samples {
            let v = (s * gain).clamp(-1.0, 1.0);
            w.write_sample((v * i16::MAX as f32) as i16)?;
        }
        w.finalize()?;
    }
    Ok(cursor.into_inner())
}

/// Decodes a WAV file into mono f32 samples (used for tests and the debug audio override).
pub fn decode_wav(bytes: &[u8]) -> Result<Captured> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes))?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().filter_map(Result::ok).map(|v| v as f32 / max).collect()
        }
    };
    let samples = raw.chunks(ch).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect();
    Ok(Captured { samples, sample_rate: spec.sample_rate })
}

// ---------------------------------------------------------------------------------------------
// UI sounds
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum Sound {
    Start,
    CommandStart,
    Stop,
    Done,
    Error,
    Cancel,
}

/// (frequency Hz, duration ms, gap after ms)
fn notes(sound: Sound) -> &'static [(f32, u32, u32)] {
    match sound {
        Sound::Start => &[(784.0, 70, 10), (1175.0, 110, 0)],
        Sound::CommandStart => &[(659.0, 60, 10), (880.0, 60, 10), (1319.0, 110, 0)],
        Sound::Stop => &[(1047.0, 60, 10), (784.0, 90, 0)],
        Sound::Done => &[(1319.0, 60, 0)],
        Sound::Error => &[(311.0, 120, 50), (233.0, 180, 0)],
        Sound::Cancel => &[(587.0, 70, 10), (440.0, 90, 0)],
    }
}

fn synth(sound: Sound, rate: u32, volume: f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &(freq, ms, gap) in notes(sound) {
        let n = (rate as f32 * ms as f32 / 1000.0) as usize;
        let attack = (rate as f32 * 0.006) as usize;
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let env_a = (i as f32 / attack.max(1) as f32).min(1.0);
            let env_d = (-(i as f32) / n as f32 * 4.0).exp();
            // Soft bell: fundamental plus a quiet octave partial.
            let v = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.8
                + (2.0 * std::f32::consts::PI * freq * 2.0 * t).sin() * 0.12 * env_d;
            out.push(v * env_a * env_d * volume * 0.35);
        }
        out.extend(std::iter::repeat_n(0.0, (rate as f32 * gap as f32 / 1000.0) as usize));
    }
    // Short fade tail so the device doesn't click.
    out.extend(std::iter::repeat_n(0.0, (rate as f32 * 0.03) as usize));
    out
}

/// Plays a UI sound without blocking.
pub fn play(sound: Sound, volume: f32) {
    if volume <= 0.0 {
        return;
    }
    let _ = thread::Builder::new().name("od-sound".into()).spawn(move || {
        if let Err(e) = play_blocking(sound, volume) {
            log::debug!("could not play sound: {e:#}");
        }
    });
}

fn play_blocking(sound: Sound, volume: f32) -> Result<()> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| anyhow!("no output device"))?;
    let config = device.default_output_config()?;
    let rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let format = config.sample_format();
    let samples = Arc::new(synth(sound, rate, volume.clamp(0.0, 1.0)));
    let total = samples.len();
    let pos = Arc::new(AtomicU32::new(0));
    let stream_config: cpal::StreamConfig = config.into();

    macro_rules! out_stream {
        ($t:ty) => {{
            let samples = samples.clone();
            let pos = pos.clone();
            device.build_output_stream(
                &stream_config,
                move |data: &mut [$t], _: &cpal::OutputCallbackInfo| {
                    for frame in data.chunks_mut(channels) {
                        let p = pos.fetch_add(1, Ordering::Relaxed) as usize;
                        let v = samples.get(p).copied().unwrap_or(0.0);
                        for s in frame.iter_mut() {
                            *s = <$t as cpal::FromSample<f32>>::from_sample_(v);
                        }
                    }
                },
                |e| log::debug!("sound stream error: {e}"),
                None,
            )?
        }};
    }
    let stream = match format {
        SampleFormat::F32 => out_stream!(f32),
        SampleFormat::I16 => out_stream!(i16),
        SampleFormat::U16 => out_stream!(u16),
        SampleFormat::I32 => out_stream!(i32),
        other => return Err(anyhow!("unsupported output format {other:?}")),
    };
    stream.play()?;
    let ms = (total as f32 / rate as f32 * 1000.0) as u64 + 80;
    thread::sleep(Duration::from_millis(ms));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_halves_length() {
        let s: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.01).sin()).collect();
        let r = resample_to_16k(&s, 48_000);
        assert_eq!(r.len(), 16_000);
    }

    #[test]
    fn silence_has_no_speech() {
        let s = vec![0.0005f32; 16_000];
        assert!(!analyse(&s).has_speech);
    }

    #[test]
    fn tone_is_detected_and_trimmed() {
        let mut s = vec![0.0f32; 16_000];
        s.extend((0..16_000).map(|i| (i as f32 * 0.1).sin() * 0.3));
        s.extend(vec![0.0f32; 16_000]);
        let a = analyse(&s);
        assert!(a.has_speech);
        assert!(a.start > 8_000 && a.start < 16_000);
        assert!(a.end > 32_000 && a.end < 40_000);
    }

    #[test]
    fn wav_roundtrip() {
        let s: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let wav = encode_wav(&s, 16_000).unwrap();
        let back = decode_wav(&wav).unwrap();
        assert_eq!(back.sample_rate, 16_000);
        assert_eq!(back.samples.len(), 1600);
    }
}
