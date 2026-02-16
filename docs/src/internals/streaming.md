# Streaming

`spine-stream` provides reactive streaming with multiplexing and flow control.

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
