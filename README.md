# uuun

Moog-style 8-voices polyphonic software synthesizer.

## Summary

Learning project. Extending on https://github.com/eiri/gregory

## Build & Run

```bash
$ make test
$ make lint
$ make run _or make build_
```

or to specify midi channel

```bash
$ cargo run -- 1
   Compiling uuun v0.3.0 (/Users/eiri/Code/github.com/eiri/uuun)
    Finished `dev` profile [optimized + debuginfo] target(s) in 0.67s
     Running `target/debug/uuun 1`
Available MIDI input ports:
  [0] MiniFuse 4
  [1] Bass Station II
Connecting to MIDI port [1]: Bass Station II
synthesizer running. Press Ctrl-C to quit.
```

## License

[MIT](https://github.com/eiri/uuun/blob/main/LICENSE)
