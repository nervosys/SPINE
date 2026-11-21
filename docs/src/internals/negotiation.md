# Protocol Negotiation

The native wire (spine-wire v1, specified in `docs/spec/00-wire.md`) opens with
one round trip: the client sends `HELLO`, the server answers `HELLO_ACK`.
The code is `spine_protocol::hello`. It is not yet used by the daemon or the
SDK; the frame codec and body encoding it builds on are `spine_protocol::frame`
and `spine_protocol::cbor`.

The earlier JSON `VersionOffer` / `VersionResponse` handshake and its 13
`ProtocolFeature` variants had no caller on any wire and were removed.

## Bodies

Bodies are deterministic CBOR maps with text keys. `HELLO` and `HELLO_ACK` are
open (unknown keys are ignored); `CLOSE` is closed.

| Frame | Keys |
|-------|------|
| `HELLO` | `versions` (strictly ascending, not empty), `max_frame` (1024 to 16,777,216), `extensions` (no duplicates), `agent` (optional, at most 128 bytes) |
| `HELLO_ACK` | `version`, `max_frame`, `max_inflight` (at least 1), `extensions`, `agent` (optional) |
| `CLOSE` | `code`, `message`, `supported` (optional, for `unsupported_version`) |

## Rules

- The server selects the highest version both sides support. If there is none
  it sends `CLOSE` with `unsupported_version` and its `supported` list.
- `max_frame` is the smaller of the client's and the server's limit.
- `extensions` in the acknowledgement are the offered extensions the server
  supports, in the client's offered order; the position is the extension index.

## Usage

```rust,ignore
use spine_protocol::hello::{negotiate, Hello, LocalConfig};

// Server side: frame is the first frame read from the peer.
let hello = Hello::from_frame(&frame)?;
let ack = negotiate(&hello, &LocalConfig::default())?;
let reply = ack.to_frame()?;
// Client side, after reading the acknowledgement:
// ack.validate_against(&hello)?;
```

An `Err(Close)` is the body to send in a `CLOSE` frame before closing.
