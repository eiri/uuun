use std::{
    hint::black_box,
    sync::{Arc, mpsc},
    time::Instant,
};

use uuun::{
    dsp::patch::{OscParams, Patch},
    engine::{
        allocator::MAX_VOICES,
        audio::AudioCallback,
        control::Controls,
        message::{EngineEvent, EngineMessage},
        synth::{BLOCK_SIZE, Synth},
    },
};

const RUNS: usize = 2_000;
const WARMUP_RUNS: usize = 100;
const OUT_CHANNELS: usize = 2;

struct Callback {
    _tx: mpsc::SyncSender<EngineMessage>,
    callback: AudioCallback,
    output: [f32; BLOCK_SIZE * OUT_CHANNELS],
}

impl Callback {
    fn new(sample_rate: f64, voices: usize) -> Self {
        let patch = Patch {
            oscillators: [OscParams {
                level: 1.0,
                ..Default::default()
            }; 3],
            noise_level: 1.0,
            filter_resonance: 0.9,
            filter_env_amount: 1.0,
            lfo_depth: 1.0,
            ..Patch::default()
        };
        let (tx, rx) = mpsc::sync_channel(256);
        let mut synth = Synth::new(patch, sample_rate);

        for voice in 0..voices {
            synth.handle(EngineMessage {
                channel: voice / MAX_VOICES,
                event: EngineEvent::NoteOn {
                    note: 48 + (voice % MAX_VOICES) as u8,
                    velocity: 127,
                },
            });
        }

        Self {
            _tx: tx,
            callback: AudioCallback::new(rx, Arc::new(Controls::new()), synth, OUT_CHANNELS),
            output: [0.0; BLOCK_SIZE * OUT_CHANNELS],
        }
    }

    fn run(&mut self) {
        self.callback.render(&mut self.output);
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
