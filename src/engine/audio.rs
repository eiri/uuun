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

        // Mix buffer shared across blocks.
        let mut mix_f64 = vec![0.0_f64; BLOCK_SIZE];

        let build_result = match supported.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                &config,
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
    mix_f64: &mut Vec<f64>,
    out_chans: usize,
    _sample_rate: f64,
) {
    let frames = output.len() / out_chans;

    while let Ok(msg) = rx.try_recv() {
        apply_message(channels, msg);
    }

    if mix_f64.len() < frames {
        mix_f64.resize(frames, 0.0);
    }
    mix_f64[..frames].fill(0.0);

    for ch in channels.iter_mut() {
        ch.process(&mut mix_f64[..frames], frames);
    }

    let mut frame_idx = 0;
    for sample in mix_f64.iter().take(frames) {
        for ch in 0..out_chans {
            output[frame_idx + ch] = sample.clamp(-1.0, 1.0) as f32;
        }
        frame_idx += out_chans;
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
        EngineMessage::AllNotesOff => {
            for ch in channels.iter_mut() {
                ch.all_notes_off();
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
