//! Microphone capture via cpal.
//!
//! Preferred capture format is 16 kHz mono f32 (the server's native appetite:
//! PCM WAV 16-bit / 16 kHz / mono, zero server-side resample). Devices that
//! cannot do 16k are handled by a fallback chain; conversion to the target
//! format happens at stop time so the audio callback stays allocation-free.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ChannelCount, StreamConfig};
use parking_lot::Mutex;
use serde::Serialize;

/// Target sample rate for uploads.
pub const TARGET_RATE: u32 = 16_000;
/// Hard stop well under the server's 25 MiB cap (~13 min @ 16 kHz/16-bit).
/// 413 is "stop recording" UX, so we self-cap at 12 min instead.
pub const MAX_SECONDS: u32 = 720;

/// State shared between the audio callback and the main thread.
struct CaptureState {
    /// Interleaved i16 samples at `rate`.
    samples: Mutex<Vec<i16>>,
    /// Set by the callback when MAX_SECONDS of audio is buffered.
    capped: AtomicBool,
}

pub struct Recorder {
    /// Dropping this stops capture and guarantees no further callbacks.
    _stream: cpal::Stream,
    state: Arc<CaptureState>,
    rate: u32,
    channels: ChannelCount,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInfo {
    pub sample_rate: u32,
    pub channels: u16,
}

fn map_cpal_error(e: cpal::Error) -> crate::error::ClientError {
    let msg = e.to_string();
    if msg.to_lowercase().contains("perm") || msg.to_lowercase().contains("authoriz") {
        crate::error::ClientError::audio(
            "microphone access denied — allow it in System Settings → Privacy & Security → Microphone",
        )
    } else {
        crate::error::ClientError::audio(format!("audio capture failed: {msg}"))
    }
}

impl Recorder {
    /// Open the default input device and start capturing immediately.
    pub fn start() -> Result<(Self, StartInfo), crate::error::ClientError> {
        let host = cpal::Host::default();
        let device = host
            .default_input_device()
            .ok_or_else(|| crate::error::ClientError::audio("no input device found"))?;

        let state = Arc::new(CaptureState {
            samples: Mutex::new(Vec::new()),
            capped: AtomicBool::new(false),
        });

        let mut last_err = crate::error::ClientError::audio("no usable input configuration");
        for cfg in candidate_configs(&device) {
            let built = build_stream(&device, cfg, Arc::clone(&state));
            match built {
                Ok(stream) => {
                    let info = StartInfo {
                        sample_rate: cfg.sample_rate,
                        channels: cfg.channels,
                    };
                    return Ok((
                        Recorder {
                            _stream: stream,
                            state,
                            rate: cfg.sample_rate,
                            channels: cfg.channels,
                        },
                        info,
                    ));
                }
                Err(e) => last_err = map_cpal_error(e),
            }
        }
        Err(last_err)
    }

    pub fn is_capped(&self) -> bool {
        self.state.capped.load(Ordering::Relaxed)
    }

    /// Stop capture, drain, downmix + resample to 16 kHz mono, encode WAV.
    pub fn finish_into_wav(self) -> Result<Vec<u8>, crate::error::ClientError> {
        drop(self._stream); // no callbacks can run past this point
        let raw = self.state.samples.lock().clone();
        if raw.is_empty() {
            return Err(crate::error::ClientError::audio(
                "no audio captured — check the input device",
            ));
        }
        let mono = downmix(&raw, self.channels as usize);
        let resampled = resample_linear(&mono, self.rate, TARGET_RATE);
        Ok(encode_wav16_mono(&resampled, TARGET_RATE))
    }
}

/// Build the ordered fallback chain of capture configs:
/// 1. exact 16 kHz mono (ideal)
/// 2. any supported mono config, closest max-rate to 16k first
/// 3. device default config (stereo/48k etc. — converted at stop time)
fn candidate_configs(device: &cpal::Device) -> Vec<StreamConfig> {
    let mut out: Vec<StreamConfig> = Vec::new();
    if let Ok(cfgs) = device.supported_input_configs() {
        let mut monos: Vec<_> = cfgs.filter(|c| c.channels() == 1).collect();
        monos.sort_by_key(|c| (c.max_sample_rate().abs_diff(TARGET_RATE), c.min_sample_rate()));
        for c in monos {
            let rate = if c.min_sample_rate() <= TARGET_RATE && TARGET_RATE <= c.max_sample_rate() {
                TARGET_RATE
            } else {
                c.max_sample_rate()
            };
            out.push(StreamConfig {
                channels: 1,
                sample_rate: rate,
                buffer_size: cpal::BufferSize::Default,
            });
        }
    }
    if let Ok(def) = device.default_input_config() {
        out.push(StreamConfig {
            channels: def.channels(),
            sample_rate: def.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        });
    }
    // De-duplicate while preserving order.
    out.dedup();
    out
}

fn build_stream(
    device: &cpal::Device,
    cfg: StreamConfig,
    state: Arc<CaptureState>,
) -> Result<cpal::Stream, cpal::Error> {
    let max_frames = (MAX_SECONDS as u64 * cfg.sample_rate as u64 * cfg.channels as u64) as usize;
    let data_cb = move |data: &[f32], _info: &cpal::InputCallbackInfo| {
        // try_lock keeps the audio thread non-blocking if stop() is draining.
        let Some(mut buf) = state.samples.try_lock() else { return };
        if buf.len() >= max_frames {
            state.capped.store(true, Ordering::Relaxed);
            return;
        }
        buf.reserve(data.len());
        for &s in data {
            // Clamp then scale f32 [-1, 1] → i16.
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            buf.push(v);
        }
    };
    let err_cb = |e: cpal::Error| eprintln!("[audio] stream error: {e}");
    let stream = device.build_input_stream(cfg, data_cb, err_cb, None)?;
    // cpal input streams start paused; capture only flows after play().
    stream.play()?;
    Ok(stream)
}

/// Average interleaved multi-channel frames down to mono.
fn downmix(interleaved: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

/// Linear-interpolation resampler. Cheap and plenty for speech; avoids a
/// heavyweight DSP dependency for the rare non-16k device.
fn resample_linear(input: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let idx = src.floor();
        let frac = src - idx;
        let i0 = idx as usize;
        let a = input[i0.min(input.len() - 1)] as f64;
        let b = input[(i0 + 1).min(input.len() - 1)] as f64;
        out.push((a + (b - a) * frac).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
    }
    out
}

/// Encode PCM16 mono samples into a WAV byte buffer (canonical 44-byte header).
fn encode_wav16_mono(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    let byte_rate = sample_rate * 2; // mono, 16-bit
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // channels = mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_canonical() {
        let wav = encode_wav16_mono(&[0, -1, i16::MAX], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 6);
    }

    #[test]
    fn downmix_stereo_to_mono() {
        // frames: (10,20) (-10,-20) → mono 15, -15
        assert_eq!(downmix(&[10, 20, -10, -20], 2), vec![15, -15]);
    }

    #[test]
    fn resample_halves_length() {
        let input = vec![100i16; 32_000]; // 2 s @ 16k
        let out = resample_linear(&input, 32_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|&s| s == 100));
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input = vec![7i16, 9, -3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }
}
