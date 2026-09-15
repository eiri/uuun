use std::sync::mpsc;

use cpal::{
    BufferSize, SampleFormat, Stream, StreamConfig,
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

        let supported = device.default_output_config().map_err(|e| e.to_string())?;

        let sample_rate = supported.sample_rate() as f64;
        let channels = supported.channels() as usize;

        let config = StreamConfig {
            channels: supported.channels(),
            sample_rate: supported.sample_rate(),
            buffer_size: BufferSize::Fixed(BLOCK_SIZE as u32),
        };

        // Bounded channel: 256 messages of headroom before the sender blocks.
        let (tx, rx) = mpsc::sync_channel::<EngineMessage>(256);

        // Build the four multitimbral channels on the audio thread's closure.
        let mut ch_array: [Channel; NUM_CHANNELS] =
            std::array::from_fn(|_| Channel::new(initial_patch, sample_rate));

        // Fixed scratch buffer keeps allocations off the audio thread.
        let mut mix_f64 = [0.0_f64; BLOCK_SIZE];

        let build_result = match supported.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _info| {
                    audio_callback(
                        data,
                        &rx,
                        &mut ch_array,
                        &mut mix_f64,
                        channels,
                        sample_rate,
                    );
                },
                |err| eprintln!("[uuun] audio stream error: {err}"),
                None,
            ),
            fmt => return Err(format!("unsupported sample format: {fmt}")),
        };

        let stream = build_result.map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;

        Ok(Self {
            sender: tx,
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

fn audio_callback(
    output: &mut [f32],
    rx: &mpsc::Receiver<EngineMessage>,
    channels: &mut [Channel; NUM_CHANNELS],
    mix_f64: &mut [f64; BLOCK_SIZE],
    out_chans: usize,
    _sample_rate: f64,
) {
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
            frame.fill(sample.clamp(-1.0, 1.0) as f32);
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

    #[test]
    fn callback_handles_large_blocks() {
        let patch = Patch::default();
        let mut channels = std::array::from_fn(|_| Channel::new(patch, 48_000.0));
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(EngineMessage::NoteOn {
            channel: 0,
            note: 60,
            velocity: 100,
        })
        .unwrap();

        let frames = BLOCK_SIZE * 3;
        let mut output = vec![0.0; frames * 2];
        let mut mix = [0.0; BLOCK_SIZE];
        audio_callback(&mut output, &rx, &mut channels, &mut mix, 2, 48_000.0);

        assert!(output.iter().any(|sample| *sample != 0.0));
    }
}
