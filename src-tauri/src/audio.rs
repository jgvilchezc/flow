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

    pub fn start(&mut self) -> Result<()> {
        if self.stream.is_some() {
            return Ok(());
        }
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("no input device available"))?;
        let config = device
            .default_input_config()
            .context("failed to get default input config")?;

        self.source_rate = config.sample_rate().0;
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
        resample_linear(&samples, self.source_rate, WHISPER_SAMPLE_RATE)
    }

    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }
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
