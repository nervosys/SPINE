# Protocol Negotiation

Every native connection (TCP, TLS, WebSocket, and QUIC when built) opens with one
round trip of the spine-wire v1 handshake: the client sends `HELLO` and the
server answers `HELLO_ACK`. Nothing else may be sent first. The specification is
`docs/spec/00-wire.md` (sections C.5 to C.7 and C.10); the code is
`spine_protocol::hello` (bodies and `negotiate`) and
`spine_protocol::connection::Connection` (which sends and checks them).
`AgentClient::connect*` and the daemon both go through it.

The earlier JSON `VersionOffer` / `VersionResponse` handshake and its
`ProtocolFeature` list are gone. Nothing in the current code negotiates features
by name; the only optional capability slot is `extensions`.

## Bodies

Bodies are deterministic CBOR maps with text keys. `HELLO` and `HELLO_ACK` are
open (unknown keys are ignored, so later versions can add fields); `CLOSE` is
closed. Frames in the handshake are read under the 64 KiB pre-`HELLO` limit.

| Frame | Keys |
|-------|------|
| `HELLO` (client, first) | `versions` (strictly ascending, not empty), `max_frame` (1,024 to 16,777,216), `extensions` (no duplicates), `agent` (optional, at most 128 bytes) |
| `HELLO_ACK` (server) | `version`, `max_frame`, `max_inflight` (at least 1), `extensions`, `agent` (optional) |
| `CLOSE` | `code`, `message`, `supported` (optional, for `unsupported_version`) |

## Rules

- **Version.** The server selects the highest version offered that it supports.
  If there is none it sends `CLOSE` with `unsupported_version` and its
  `supported` list. Only version 1 exists.
- **Frame size.** `max_frame` in the acknowledgement is the smaller of the
  client's and the server's limit, and applies to both directions from then on.
- **In-flight requests.** `max_inflight` is the number of requests the client may
  have outstanding. The current daemon offers 1, so a client sends one `REQUEST`
  at a time; a second concurrent request is answered `overloaded`.
- **Extensions.** `extensions` in the acknowledgement are the offered extensions
  the server supports, in the order the client offered them; the position is the
  extension index used by `EXT` frames. The current daemon supports none, and the
  SDK offers none.
- **Client check.** The client rejects an acknowledgement that selects a version
  it did not offer, raises `max_frame` above its own, names an extension it did
  not offer, or carries `max_inflight` of 0, each as `protocol_violation`.
- **Timeout.** The daemon closes a connection that has not sent `HELLO` within
  `server.hello_timeout_secs` (default 10) with `CLOSE(timeout)`. The SDK waits
  ten seconds for `HELLO_ACK`.

The daemon's offer is `spine_protocol::native::local_config()`: version 1, 16 MiB
`max_frame`, `max_inflight` 1, no extensions. The SDK's `HELLO` is
`native::client_hello()`: version 1, 16 MiB `max_frame`, no extensions, and an
`agent` string of the form `spine-protocol/<version>`.

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

An `Err(Close)` is the body to send in a `CLOSE` frame before closing. Most code
does not call these directly: `AgentClient::connect` and
`Connection::client_handshake` / `Connection::server_handshake` run the whole
exchange.
