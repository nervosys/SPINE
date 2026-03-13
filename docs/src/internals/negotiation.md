# Protocol Version Negotiation

SPINE includes a version negotiation handshake to ensure client and server agree on protocol features.

## Overview

When a connection opens, the client sends a `VersionOffer` containing its supported protocol version and feature set. The server responds with a `VersionResponse` indicating acceptance (with the intersection of features) or rejection.

## Wire Format

```text
Client → Server:  [SPNE magic (4 bytes)] [length (4 bytes)] [JSON VersionOffer]
Server → Client:  [SPNE magic (4 bytes)] [length (4 bytes)] [JSON VersionResponse]
```

## Protocol Features

13 negotiable `ProtocolFeature` variants:

| Feature | Description |
|---------|-------------|
| Compression | zstd adaptive compression |
| Encryption | AES-256-GCM AEAD |
| Chameleon | Moving-target latent-space protocol |
| Speculation | Bidirectional message prediction |
| LatentBinary | Binary latent vector encoding |
| Streaming | Reactive stream multiplexing |
| ZeroCopy | Zero-copy frame I/O |
| BatchFrames | Frame batching/coalescing |
| WebSocket | WebSocket transport support |
| Quic | QUIC transport support |
| PostQuantum | Post-quantum (RLWE/ML-KEM) cryptography |
| NeuralProtocol | Neural protocol evolution |
| PluginPipeline | Transport plugin system |

## Usage

```rust,ignore
use spine_protocol::negotiation::{negotiate_client, negotiate_server};

// Client side
let accepted_features = negotiate_client(&mut stream, my_features).await?;

// Server side  
let client_features = negotiate_server(&mut stream, supported_features).await?;
```