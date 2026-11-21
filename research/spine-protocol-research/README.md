# spine-protocol-research

Experimental protocol research for SPINE. **It is unreviewed, it is not on the
native wire, and nothing here is a security feature.** Do not use it to protect
data. Use TLS (or the session layer) for confidentiality and authentication.

The code was part of `spine-protocol` behind a `research` cargo feature. It moved
here when the native wire was cut down to the `SP` body, a compression flag byte
and a fixed 5-byte frame. `spine-protocol` no longer has a `research` feature and
no crate in the root workspace depends on this one except as an optional
dependency behind a `research` feature (`spine-gateway`).

## What is here

- `ChameleonKey`: a latent-space key that encodes a byte string into a
  `LatentVector` with a neural encoder and evolves per message. Its output is
  lossy and keyed by a 64-bit-derived seed in places; it is not an AEAD.
- `SpineConnection` and `connect_auto`: a second framing codec over TCP or QUIC
  with optional pre-shared-key AES-GCM and the chameleon layer. It does not
  speak the native wire.
- The protocol evolution framework (`ProtocolGenome`, `ProtocolPopulation`,
  `CoevolutionaryArena`, ...).
- `TitansLatentCodec`: a `spine_protocol::NeuralCodec` over `spine-neural`'s
  `NeuralLatentEncoder`. The gateway registers it behind its own `research`
  feature.
- Tests for `spine-nostd` and `spine-embedded` frame-header interop (they do not
  touch `spine-protocol`; the root crate may not depend on `research/`).

## Dependencies

It uses only public items of `spine-protocol`, plus `spine-neural` and
`spine-crypto`. Two small private helpers (`fresh_nonce`, `decode_zstd_limited`)
are copied here rather than made public there.

## Build

```
cd research
cargo test -p spine-protocol-research --lib --tests
cargo run -p spine-protocol-research --example chameleon_demo
cargo bench -p spine-protocol-research --bench neural_codec_bench
```

The crate has a crate-level `#![allow(dead_code)]`, carried over from
`spine-protocol`.
