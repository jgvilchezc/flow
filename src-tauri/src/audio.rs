use crate::mic_mute;
use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Captures microphone input. Samples are accumulated as f32 mono at the
/// device's native rate and resampled to 16kHz when the recording stops.
pub struct Recorder {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    source_rate: u32,
}

// cpal::Stream is !Send on some platforms; the recorder is only ever touched
// from the dedicated audio thread (see lib.rs), so this is safe in practice.
unsafe impl Send for Recorder {}

impl Recorder {
    pub fn new() -> Self {
        Self {
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            source_rate: WHISPER_SAMPLE_RATE,
        }
    }

    /// Starts capturing from the input device named `preferred_device_name`
    /// when it exists, otherwise from the system default input.
    pub fn start(&mut self, preferred_device_name: Option<&str>) -> Result<()> {
        if self.stream.is_some() {
            return Ok(());
        }
        let host = cpal::default_host();
        let device = match preferred_device_name.and_then(|name| resolve_by_name(&host, name)) {
            Some(device) => device,
            None => host
                .default_input_device()
                .ok_or_else(|| anyhow!("no input device available"))?,
        };
        let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());

        // Something on macOS occasionally mutes the input device at the
        // CoreAudio level, which makes cpal deliver silence. Push-to-talk
        // means the user wants the mic live right now, so clear it. This is
        // best-effort: a failure here must never prevent the recording.
        match mic_mute::ensure_unmuted(&device_name) {
            Ok(true) => {
                log::warn!("input device {device_name:?} was muted by the system; unmuted it")
            }
            Ok(false) => {}
            Err(err) => {
                log::warn!("could not clear the mute on input device {device_name:?}: {err}")
            }
        }

        let config = device
            .default_input_config()
            .context("failed to get default input config")?;

        self.source_rate = config.sample_rate().0;
        log::info!("recording from {device_name:?} at {} Hz", self.source_rate);
        let channels = config.channels() as usize;

        let buffer = Arc::clone(&self.buffer);
        buffer.lock().unwrap().clear();

        let err_fn = |err| log::error!("audio stream error: {err}");

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| push_mono(&buffer, data, channels),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let floats: Vec<f32> =
                        data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                    push_mono(&buffer, &floats, channels);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    let floats: Vec<f32> = data
                        .iter()
                        .map(|s| (*s as f32 - 32_768.0) / 32_768.0)
                        .collect();
                    push_mono(&buffer, &floats, channels);
                },
                err_fn,
                None,
            )?,
            other => return Err(anyhow!("unsupported sample format: {other}")),
        };

        stream.play().context("failed to start audio stream")?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Stops capturing and returns the recording as 16kHz mono f32.
    pub fn stop(&mut self) -> Vec<f32> {
        self.stream = None; // dropping the stream stops capture
        let samples = std::mem::take(&mut *self.buffer.lock().unwrap());
        log::info!(
            "captured {:.2}s at {} Hz, peak amplitude {:.4}",
            samples.len() as f32 / self.source_rate as f32,
            self.source_rate,
            peak_amplitude(&samples)
        );
        resample_linear(&samples, self.source_rate, WHISPER_SAMPLE_RATE)
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }
}

/// Names of every input device cpal can see. Devices whose name cannot be
/// read are skipped.
pub fn list_input_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Looks up an input device by exact name, logging why the lookup failed so
/// the fallback to the system default is diagnosable from the log alone.
fn resolve_by_name(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    let devices = match host.input_devices() {
        Ok(devices) => devices,
        Err(err) => {
            log::warn!("cannot enumerate input devices ({err}); falling back to system default");
            return None;
        }
    };
    let found = pick_by_name(devices.filter_map(|d| d.name().ok().map(|n| (n, d))), name);
    if found.is_none() {
        log::warn!("input device {name:?} not found; falling back to system default");
    }
    found
}

/// Picks the first device whose name equals `wanted`. Duplicate names (two
/// identical USB mics) resolve to whichever cpal enumerates first.
fn pick_by_name<T>(mut devices: impl Iterator<Item = (String, T)>, wanted: &str) -> Option<T> {
    devices.find(|(name, _)| name == wanted).map(|(_, d)| d)
}

/// Peak below which a recording is treated as silence. macOS delivers exact
/// zeros when the Microphone permission is denied or stale; a quiet room on a
/// real microphone peaks orders of magnitude above this.
pub const SILENCE_PEAK: f32 = 1e-4;

/// True when the recording carries no usable signal (see [`SILENCE_PEAK`]).
pub fn is_silent(samples: &[f32]) -> bool {
    peak_amplitude(samples) < SILENCE_PEAK
}

/// Largest absolute sample value; 0.0 for an empty slice.
fn peak_amplitude(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

fn push_mono(buffer: &Arc<Mutex<Vec<f32>>>, data: &[f32], channels: usize) {
    let mut buf = buffer.lock().unwrap();
    if channels <= 1 {
        buf.extend_from_slice(data);
    } else {
        buf.extend(
            data.chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
    }
}

fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<(String, u8)> {
        vec![
            ("MacBook Pro Microphone".into(), 1),
            ("iPhone Microphone".into(), 2),
        ]
    }

    #[test]
    fn pick_by_name_returns_exact_match() {
        let picked = pick_by_name(candidates().into_iter(), "iPhone Microphone");
        assert_eq!(picked, Some(2));
    }

    #[test]
    fn pick_by_name_returns_none_when_no_device_matches() {
        let picked = pick_by_name(candidates().into_iter(), "USB Mic");
        assert_eq!(picked, None);
    }

    #[test]
    fn peak_amplitude_of_empty_slice_is_zero() {
        assert_eq!(peak_amplitude(&[]), 0.0);
    }

    #[test]
    fn is_silent_for_all_zero_samples() {
        assert!(is_silent(&[0.0; 16_000]));
    }

    #[test]
    fn is_not_silent_for_quiet_room_noise() {
        assert!(!is_silent(&[0.0, 0.002, -0.003, 0.001]));
    }

    #[test]
    fn peak_amplitude_uses_absolute_value() {
        assert_eq!(peak_amplitude(&[0.1, -0.8, 0.5]), 0.8);
    }
}
