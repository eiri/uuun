# uuun

`uuun` is a small Moog-style subtractive synth controlled by MIDI with four MIDI channels with eight voices each. It's a learning project.

## Run

You need Rust, a MIDI input, and an audio output device.

```sh
cargo run
cargo run -- 1
```

The optional number selects a MIDI input port and available ports are printed at startup. MIDI channels 1-4 are accepted; the rest are ignored.

```sh
cargo run
   Compiling uuun v0.3.0 (/Users/eiri/Code/github.com/eiri/uuun)
    Finished `dev` profile [optimized + debuginfo] target(s) in 0.69s
     Running `target/debug/uuun`
Available MIDI input ports:
  [0] KeyStep Pro
  [1] MiniFuse 4
Connecting to MIDI port [0]: KeyStep Pro
synthesizer running. Press Ctrl-C to quit.
```

Tested on macOS with CoreAudio and CoreMIDI.

## MIDI controls

|             CC | Control                                |
| -------------: | -------------------------------------- |
|              5 | Glide time                             |
|             64 | Sustain                                |
|         71, 74 | Filter resonance, cutoff               |
| 72, 73, 75, 79 | Amp release, attack, decay, sustain    |
|         76, 77 | LFO rate, depth                        |
|         85, 86 | Filter key tracking, envelope amount   |
|        102-105 | Filter attack, decay, sustain, release |
|        106-109 | Oscillator 1-3 and noise levels        |
|            120 | Stop sound immediately                 |
|            121 | Reset controllers                      |
|            123 | Release all notes                      |

Each channel has eight voices. Glide begins when a voice is reused; its first note starts immediately.

If notes get stuck, send CC 120 or restart the synth.

## Audio

The synth uses the default output device. It asks for a 256-frame buffer and falls back to the device default. Signed, unsigned, and floating-point PCM formats are supported; DSD is not.

## Development

```sh
make test
make lint
make benchmark
make build
```

Based on [gregory](https://github.com/eiri/gregory).

## License

[MIT](LICENSE)
