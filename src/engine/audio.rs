use std::sync::mpsc;

use cpal::{
    BufferSize, Device, Error, ErrorKind, FromSample, I24, SampleFormat, SizedSample, Stream,
    StreamConfig, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::{
    dsp::patch::Patch,
    engine::{channel::Channel, message::EngineMessage},
};

pub const NUM_CHANNELS: usize = 4; // multitimbral
pub const BLOCK_SIZE: usize = 256; // preferred buffer size in frames

pub struct Engine {
    sender: mpsc::SyncSender<EngineMessage>,
    _stream: Stream,
}

impl Engine {
    pub fn start(initial_patch: Patch) -> Result<Self, String> {
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

        let (sender, stream) = match build_stream(&device, config, sample_format, initial_patch) {
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
            _stream: stream,
        })
    }

    #[allow(dead_code)]
    pub fn send(&self, msg: EngineMessage) -> bool {
        self.sender.try_send(msg).is_ok()
    }

    #[allow(dead_code)]
    pub fn sender(&self) -> mpsc::SyncSender<EngineMessage> {
        self.sender.clone()
    }
}

fn build_stream(
    device: &Device,
    config: StreamConfig,
    format: SampleFormat,
    patch: Patch,
) -> Result<(mpsc::SyncSender<EngineMessage>, Stream), Error> {
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
) -> Result<(mpsc::SyncSender<EngineMessage>, Stream), Error>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate as f64;
    let out_chans = config.channels as usize;
    let (tx, rx) = mpsc::sync_channel(256);
    let mut channels = std::array::from_fn(|_| Channel::new(patch, sample_rate));
    let mut mix = [0.0; BLOCK_SIZE];

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _info| {
            audio_callback(data, &rx, &mut channels, &mut mix, out_chans, sample_rate);
        },
        |err| eprintln!("[uuun] audio stream error: {err}"),
        None,
    )?;

    Ok((tx, stream))
}

fn audio_callback<T>(
    output: &mut [T],
    rx: &mpsc::Receiver<EngineMessage>,
    channels: &mut [Channel; NUM_CHANNELS],
    mix_f64: &mut [f64; BLOCK_SIZE],
    out_chans: usize,
    _sample_rate: f64,
) where
    T: SizedSample + FromSample<f32>,
{
    while let Ok(msg) = rx.try_recv() {
        apply_message(channels, msg);
    }

    // CPAL may provide more frames than requested. Render bounded chunks.
    for block in output.chunks_mut(BLOCK_SIZE * out_chans) {
        let frames = block.len().div_ceil(out_chans);
        mix_f64[..frames].fill(0.0);

        for channel in channels.iter_mut() {
            channel.process(&mut mix_f64[..frames], frames);
        }

        for (frame, sample) in block.chunks_mut(out_chans).zip(mix_f64.iter()) {
            frame.fill(T::from_sample(sample.clamp(-1.0, 1.0) as f32));
        }
    }
}

fn apply_message(channels: &mut [Channel; NUM_CHANNELS], msg: EngineMessage) {
    match msg {
        EngineMessage::NoteOn {
            channel,
            note,
            velocity,
        } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.note_on(note, velocity);
            }
        }
        EngineMessage::NoteOff { channel, note } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.note_off(note);
            }
        }
        EngineMessage::SetPatch { channel, patch } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.set_patch(*patch);
            }
        }
        EngineMessage::AllNotesOff { channel } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.all_notes_off();
            }
        }
        EngineMessage::AllSoundOff { channel } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.all_sound_off();
            }
        }
        EngineMessage::PitchBend { channel, semitones } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.pitch_bend(semitones);
            }
        }
        EngineMessage::ChannelPressure { channel, value } => {
            if let Some(ch) = channels.get_mut(channel) {
                ch.channel_pressure(value);
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
        let mut channels = std::array::from_fn(|_| Channel::new(patch, 48_000.0));
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(EngineMessage::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        })
        .unwrap();

        let mut mix = [0.0; BLOCK_SIZE];
        audio_callback(output, &rx, &mut channels, &mut mix, 2, 48_000.0);
    }

    #[test]
    fn callback_handles_large_blocks() {
        let frames = BLOCK_SIZE * 3;
        let mut output = vec![0.0_f32; frames * 2];
        render(&mut output);

        assert!(output.iter().any(|sample| *sample != 0.0));
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
}
