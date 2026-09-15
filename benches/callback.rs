#![allow(dead_code, unused_imports)]

#[path = "../src/dsp/mod.rs"]
mod dsp;
#[path = "../src/engine/mod.rs"]
mod engine;

use std::{
    hint::black_box,
    sync::{Arc, mpsc},
    time::Instant,
};

use dsp::patch::Patch;
use engine::{
    audio::{BLOCK_SIZE, NUM_CHANNELS, audio_callback},
    channel::Channel,
    control::Controls,
    message::EngineMessage,
};

const RUNS: usize = 2_000;
const WARMUP_RUNS: usize = 100;
const OUT_CHANNELS: usize = 2;

struct Callback {
    _tx: mpsc::SyncSender<EngineMessage>,
    rx: mpsc::Receiver<EngineMessage>,
    controls: Arc<Controls>,
    channels: [Channel; NUM_CHANNELS],
    output: [f32; BLOCK_SIZE * OUT_CHANNELS],
    mix: [f64; BLOCK_SIZE],
    channel_mix: [f64; BLOCK_SIZE],
}

impl Callback {
    fn new(sample_rate: f64, voices: usize) -> Self {
        let patch = Patch {
            osc2_level: 1.0,
            osc3_level: 1.0,
            noise_level: 1.0,
            filter_resonance: 0.9,
            filter_env_amount: 1.0,
            lfo_depth: 1.0,
            ..Patch::default()
        };
        let (tx, rx) = mpsc::sync_channel(256);
        let mut channels = std::array::from_fn(|_| Channel::new(patch, sample_rate));

        for voice in 0..voices {
            channels[voice / 8].note_on(48 + (voice % 8) as u8, 127);
        }

        Self {
            _tx: tx,
            rx,
            controls: Arc::new(Controls::new()),
            channels,
            output: [0.0; BLOCK_SIZE * OUT_CHANNELS],
            mix: [0.0; BLOCK_SIZE],
            channel_mix: [0.0; BLOCK_SIZE],
        }
    }

    fn run(&mut self) {
        audio_callback(
            &mut self.output,
            &self.rx,
            &self.controls,
            &mut self.channels,
            &mut self.mix,
            &mut self.channel_mix,
            OUT_CHANNELS,
        );
        black_box(&self.output);
    }
}

fn percentile(samples: &[f64], percentile: f64) -> f64 {
    let index = ((samples.len() - 1) as f64 * percentile).round() as usize;
    samples[index]
}

fn main() {
    println!("callback: {BLOCK_SIZE} frames, stereo, {RUNS} samples");
    println!("rate    voices   mean      p95       p99       deadline   p99 load");

    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        for voices in [1, 8, 32] {
            let mut callback = Callback::new(sample_rate, voices);
            for _ in 0..WARMUP_RUNS {
                callback.run();
            }

            let mut samples = Vec::with_capacity(RUNS);
            for _ in 0..RUNS {
                let start = Instant::now();
                callback.run();
                samples.push(start.elapsed().as_secs_f64());
            }
            samples.sort_by(f64::total_cmp);

            let mean = samples.iter().sum::<f64>() / samples.len() as f64;
            let p95 = percentile(&samples, 0.95);
            let p99 = percentile(&samples, 0.99);
            let deadline = BLOCK_SIZE as f64 / sample_rate;

            println!(
                "{sample_rate:>6.0}  {voices:>6}   {:>6.2} ms  {:>6.2} ms  {:>6.2} ms  {:>6.2} ms  {:>7.1}%",
                mean * 1_000.0,
                p95 * 1_000.0,
                p99 * 1_000.0,
                deadline * 1_000.0,
                p99 / deadline * 100.0,
            );
        }
    }
}
