use crate::settings::{Settings, SttEngine};
use anyhow::{anyhow, Context, Result};
use std::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Whisper contexts are expensive to create (model load + Metal init), so the
/// last one is cached and reused until the user switches models.
pub struct WhisperCache {
    inner: Mutex<Option<(String, WhisperContext)>>,
}

impl WhisperCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    fn transcribe(
        &self,
        model_key: &str,
        language: &str,
        bias: Option<&str>,
        samples: &[f32],
    ) -> Result<String> {
        let mut guard = self.inner.lock().unwrap();
        let needs_load = match guard.as_ref() {
            Some((key, _)) => key != model_key,
            None => true,
        };
        if needs_load {
            let path = crate::models::local_path(model_key)
                .ok_or_else(|| anyhow!("unknown model: {model_key}"))?;
            if !path.exists() {
                return Err(anyhow!(
                    "model '{model_key}' is not downloaded yet — open Flow settings and download it"
                ));
            }
            let ctx = WhisperContext::new_with_params(
                path.to_str().context("invalid model path")?,
                WhisperContextParameters::default(),
            )
            .context("failed to load whisper model")?;
            *guard = Some((model_key.to_string(), ctx));
        }

        let (_, ctx) = guard.as_ref().unwrap();
        let mut state = ctx.create_state().context("failed to create whisper state")?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_token_timestamps(false);
        // whisper.cpp defaults to English when no language is given; "auto"
        // must be passed explicitly to trigger language detection.
        params.set_language(Some(language));
        // Bias decoding toward the user's vocabulary. Zero terms => no prompt =>
        // identical behavior to an unbiased run.
        if let Some(prompt) = bias {
            params.set_initial_prompt(prompt);
        }
        let threads = std::thread::available_parallelism()
            .map(|n| (n.get() as i32 - 2).max(2))
            .unwrap_or(4);
        params.set_n_threads(threads);

        state.full(params, samples).context("whisper inference failed")?;

        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(segment) = state.get_segment(i) {
                text.push_str(&segment.to_str_lossy()?);
            }
        }
        Ok(text.trim().to_string())
    }
}

/// Encodes 16kHz mono f32 samples as an in-memory 16-bit WAV.
fn encode_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: crate::audio::WHISPER_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for s in samples {
            writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

/// Groq's free tier currently allows 2,000 requests/day on whisper-large-v3-turbo.
async fn transcribe_groq(
    api_key: &str,
    language: &str,
    bias: Option<&str>,
    samples: &[f32],
) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("Groq API key is not set — add it in Flow settings"));
    }
    let wav = encode_wav(samples)?;
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", "whisper-large-v3-turbo")
        .text("response_format", "json")
        .text("temperature", "0");
    if language != "auto" {
        form = form.text("language", language.to_string());
    }
    // Groq's transcription endpoint is OpenAI-compatible and accepts a `prompt`
    // field to bias vocabulary. Omitted entirely when there are no terms.
    if let Some(prompt) = bias {
        form = form.text("prompt", prompt.to_string());
    }

    let response = crate::http::client()
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .context("Groq request failed")?;

    let status = response.status();
    let body: serde_json::Value = response.json().await.context("invalid Groq response")?;
    if !status.is_success() {
        let msg = body["error"]["message"].as_str().unwrap_or("unknown error");
        return Err(anyhow!("Groq STT error ({status}): {msg}"));
    }
    Ok(body["text"].as_str().unwrap_or_default().trim().to_string())
}

pub async fn transcribe(
    cache: &WhisperCache,
    settings: &Settings,
    bias: Option<&str>,
    samples: Vec<f32>,
) -> Result<String> {
    match settings.stt_engine {
        SttEngine::Groq => {
            transcribe_groq(&settings.groq_api_key, &settings.language, bias, &samples).await
        }
        SttEngine::Local => {
            // whisper inference is CPU/GPU-bound; keep it off the async executor
            tokio::task::block_in_place(|| {
                cache.transcribe(&settings.whisper_model, &settings.language, bias, &samples)
            })
        }
    }
}
