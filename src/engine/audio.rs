use std::sync::{Arc, mpsc};

use cpal::{
    BufferSize, Device, Error, ErrorKind, FromSample, I24, SampleFormat, SizedSample, Stream,
    StreamConfig, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::{
    dsp::patch::Patch,
    engine::{
        control::Controls,
        message::EngineMessage,
        synth::{BLOCK_SIZE, Synth},
    },
};

pub struct Engine {
    sender: mpsc::SyncSender<EngineMessage>,
    controls: Arc<Controls>,
    _stream: Stream,
}

impl Engine {
    pub fn start(initial_patch: Patch) -> Result<Self, String> {
        initial_patch
            .validate()
            .map_err(|err| format!("invalid initial patch: {err}"))?;

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no output device available")?;
        let device_name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| "<unknown>".to_owned());

        let supported = device
            .default_output_config()
            .map_err(|err| format!("failed to read output config for {device_name}: {err}"))?;
        let sample_format = supported.sample_format();
        if sample_format.is_dsd() {
            return Err(format!(
                "unsupported output format {sample_format} on {device_name}"
            ));
        }

        let mut config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: BufferSize::Fixed(BLOCK_SIZE as u32),
        };

        let (sender, controls, stream) =
            match build_stream(&device, config, sample_format, initial_patch) {
                Ok(result) => result,
                Err(fixed_err) if fixed_err.kind() == ErrorKind::UnsupportedConfig => {
                    config.buffer_size = BufferSize::Default;
                    build_stream(&device, config, sample_format, initial_patch).map_err(
                        |default_err| {
                            format!(
                                "failed to open {device_name} with {sample_format}, fixed buffer: \
                         {fixed_err}; default buffer: {default_err}"
                            )
                        },
                    )?
                }
                Err(err) => {
                    return Err(format!(
                        "failed to open {device_name} with {sample_format} and {config:?}: {err}"
                    ));
                }
            };

        stream
            .play()
            .map_err(|err| format!("failed to start {device_name}: {err}"))?;

        Ok(Self {
            sender,
            controls,
            _stream: stream,
        })
    }

    pub fn sender(&self) -> mpsc::SyncSender<EngineMessage> {
        self.sender.clone()
    }

    pub fn controls(&self) -> Arc<Controls> {
        Arc::clone(&self.controls)
    }
}

fn build_stream(
    device: &Device,
    config: StreamConfig,
    format: SampleFormat,
    patch: Patch,
) -> Result<(mpsc::SyncSender<EngineMessage>, Arc<Controls>, Stream), Error> {
    match format {
        SampleFormat::I8 => build_typed::<i8>(device, config, patch),
        SampleFormat::I16 => build_typed::<i16>(device, config, patch),
        SampleFormat::I24 => build_typed::<I24>(device, config, patch),
        SampleFormat::I32 => build_typed::<i32>(device, config, patch),
        SampleFormat::I64 => build_typed::<i64>(device, config, patch),
        SampleFormat::U8 => build_typed::<u8>(device, config, patch),
        SampleFormat::U16 => build_typed::<u16>(device, config, patch),
        SampleFormat::U24 => build_typed::<U24>(device, config, patch),
        SampleFormat::U32 => build_typed::<u32>(device, config, patch),
        SampleFormat::U64 => build_typed::<u64>(device, config, patch),
        SampleFormat::F32 => build_typed::<f32>(device, config, patch),
        SampleFormat::F64 => build_typed::<f64>(device, config, patch),
        format => Err(Error::with_message(
            ErrorKind::UnsupportedConfig,
            format!("unsupported output format: {format}"),
        )),
    }
}

fn build_typed<T>(
    device: &Device,
    config: StreamConfig,
    patch: Patch,
) -> Result<(mpsc::SyncSender<EngineMessage>, Arc<Controls>, Stream), Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate as f64;
    let out_chans = config.channels as usize;
    let (tx, rx) = mpsc::sync_channel(256);
    let controls = Arc::new(Controls::new());
    let mut callback = AudioCallback::new(
        rx,
        Arc::clone(&controls),
        Synth::new(patch, sample_rate),
        out_chans,
    );

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _info| {
            callback.render(data);
        },
        |err| eprintln!("[uuun] audio stream error: {err}"),
        None,
    )?;

    Ok((tx, controls, stream))
}

/// Queue, mailboxes, synthesis, and PCM conversion state for one device.
pub struct AudioCallback {
    rx: mpsc::Receiver<EngineMessage>,
    controls: Arc<Controls>,
    synth: Synth,
    mono: [f64; BLOCK_SIZE],
    out_chans: usize,
}

impl AudioCallback {
    /// The device channel count must be nonzero.
    pub fn new(
        rx: mpsc::Receiver<EngineMessage>,
        controls: Arc<Controls>,
        synth: Synth,
        out_chans: usize,
    ) -> Self {
        assert!(out_chans > 0);
        Self {
            rx,
            controls,
            synth,
            mono: [0.0; BLOCK_SIZE],
            out_chans,
        }
    }

    pub fn render<T>(&mut self, output: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        // Drain events before mailboxes, once per device callback.
        while let Ok(message) = self.rx.try_recv() {
            self.synth.handle(message);
        }
        self.controls
            .drain(|channel, control, value| self.synth.set_control(channel, control, value));

        for block in output.chunks_mut(BLOCK_SIZE * self.out_chans) {
            let frames = block.len().div_ceil(self.out_chans);
            self.synth.render(&mut self.mono[..frames]);
            for (frame, sample) in block.chunks_mut(self.out_chans).zip(&self.mono) {
                // All device formats retain the original f32 conversion.
                frame.fill(T::from_sample(*sample as f32));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render<T>(output: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        let patch = Patch::default();
        let synth = Synth::new(patch, 48_000.0);
        let (tx, rx) = mpsc::sync_channel(1);
        let controls = Arc::new(Controls::new());
        tx.send(EngineMessage::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        })
        .unwrap();

        AudioCallback::new(rx, controls, synth, 2).render(output);
    }

    #[test]
    fn callback_handles_empty_and_small_output() {
        for length in [0, 1, 2, 3, 64] {
            let mut output = vec![f32::NAN; length];
            render(&mut output);
            assert!(output.iter().all(|s| s.is_finite()));
            for frame in output.chunks(2) {
                assert!(frame.iter().all(|s| *s == frame[0]));
            }
        }
    }

    #[test]
    fn callback_signal_references() {
        let expected = [
            [
                0.0,
                -0.00534829031676054,
                -0.014621907845139503,
                0.0281984843313694,
                0.0281984843313694,
                0.012718249275118478,
            ],
            [
                0.0,
                -0.04239417985081673,
                -0.07011008262634277,
                0.20712819695472717,
                0.2725902199745178,
                0.12352681219251826,
            ],
            [
                0.0,
                -0.1695767194032669,
                -0.2804403305053711,
                0.8285127878189087,
                1.0,
                0.48679042827827923,
            ],
        ];
        for (voices, expected) in [1, 8, 32].into_iter().zip(expected) {
            let patch = Patch {
                filter_cutoff_hz: 10_000.0,
                ..Default::default()
            };
            let mut synth = Synth::new(patch, 48_000.0);
            for voice in 0..voices {
                synth.handle(EngineMessage::NoteOn {
                    channel: voice / 8,
                    note: 48 + (voice % 8) as u8,
                    velocity: 127,
                });
            }
            let (_tx, rx) = mpsc::sync_channel(1);
            let mut output = [0.0_f32; 512];
            AudioCallback::new(rx, Arc::new(Controls::new()), synth, 2).render(&mut output);
            let mono: Vec<f64> = output.iter().step_by(2).map(|s| f64::from(*s)).collect();
            for (actual, expected) in crate::dsp::reference::summary(&mono)
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (actual - expected).abs() <= 1e-6,
                    "{voices} voices: {actual} != {expected}"
                );
            }
        }
    }

    #[test]
    fn invalid_patch_fails_before_audio_setup() {
        let patch = Patch {
            filter_cutoff_hz: f64::NAN,
            ..Patch::default()
        };
        let error = Engine::start(patch).err().expect("invalid patch accepted");

        assert_eq!(
            error,
            "invalid initial patch: filter_cutoff_hz must be within 20..=20000"
        );
    }

    #[test]
    fn callback_handles_large_blocks() {
        let frames = BLOCK_SIZE * 3;
        let mut output = vec![0.0_f32; frames * 2];
        render(&mut output);

        assert!(output.iter().any(|sample| *sample != 0.0));
        let peak = output.iter().copied().map(f32::abs).fold(0.0, f32::max);
        assert!(peak <= 0.115);
    }

    #[test]
    fn callback_converts_integer_samples() {
        let mut signed = vec![0_i16; BLOCK_SIZE * 2];
        render(&mut signed);
        assert!(signed.iter().any(|sample| *sample != 0));

        let mut unsigned = vec![32_768_u16; BLOCK_SIZE * 2];
        render(&mut unsigned);
        assert!(unsigned.iter().any(|sample| *sample != 32_768));
    }

    #[test]
    fn core_matches_adapter() {
        for channels in [1, 2, 6] {
            for frames in [0, 1, 64, 256, 513, 1024] {
                let patch = Patch::default();
                let mut core = Synth::new(patch, 48_000.0);
                let message = EngineMessage::NoteOn {
                    channel: 0,
                    note: 60,
                    velocity: 100,
                };
                core.handle(message);
                let (tx, rx) = mpsc::sync_channel(256);
                tx.send(message).unwrap();
                let mut callback = AudioCallback::new(
                    rx,
                    Arc::new(Controls::new()),
                    Synth::new(patch, 48_000.0),
                    channels,
                );
                let mut mono = vec![f64::NAN; frames];
                let mut output = vec![f64::NAN; frames * channels];
                core.render(&mut mono);
                callback.render(&mut output);
                for (frame, sample) in output.chunks(channels).zip(mono) {
                    assert!(frame.iter().all(|s| *s == f64::from(sample as f32)));
                }
            }
        }
    }
}
