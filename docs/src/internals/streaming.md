# Streaming

`spine-stream` provides reactive streaming with multiplexing and flow control.

It is a library with its own message types; it is not the native wire's streaming.
spine-wire v1 reserves frame types `STREAM_START`, `STREAM_TOKEN`, `STREAM_END` and
`STREAM_CANCEL` (0x20 to 0x23), but no command streams yet: every one of the 19
commands is a single `REQUEST` and a single `RESPONSE`. Its `LatentVector` and
`LatentBatch` stream items carry embeddings as data and are not an encryption or
encoding layer.

## Features

- **Reactive streams** — Backpressure-aware data flow
- **Multiplexing** — Multiple logical streams over one connection
- **Flow control** — Window-based credit system
- **Priority queuing** — Prioritized message delivery
- **Chunked transfer** — Large payload streaming
- **Batching** — `BatchingStream` with deadline-based partial emission

## Batch Processing

Collect items into batches with configurable size and timeout:

```rust
use spine_stream::BatchingStream;

let batched = BatchingStream::new(source, 100, Duration::from_millis(50));
// Emits when: 100 items collected OR 50ms elapsed (whichever first)
```

## Priority Levels

Messages can be assigned priority for ordered delivery under contention.
