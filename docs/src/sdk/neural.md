# Neural Protocol

## Overview

The neural protocol enables learned message encoding using the Titans architecture. Messages are projected into a latent space for efficient transmission and optional cryptographic protection.

## Neural Transmission

```rust
use spine_agentic::ProtocolDomain;

let result = client.transmit_neural(
    b"important data payload",
    ProtocolDomain::Text,
).await?;
println!("Transmitted: {:?}", result);
```

## Protocol Domains

| Domain   | Description                     |
| -------- | ------------------------------- |
| `Text`   | Natural language content        |
| `Code`   | Source code and structured data |
| `Binary` | Binary data and files           |

## Latent Vector Subscription

Stream latent vectors as they're generated:

```rust
let mut rx = client.subscribe_latent().await?;
while let Some(vector) = rx.recv().await {
    println!("Latent dim={}: norm={:.3}",
        vector.len(),
        vector.iter().map(|x| x * x).sum::<f32>().sqrt()
    );
}
```

## Protocol Morphology

Trigger protocol shape-shifting:

```rust
client.morph().await?;
```

This evolves the protocol's frame structure using genetic algorithms for active defense.

## Speculation Stats

Check how well the predictor is performing:

```rust
let stats = client.get_speculation_stats();
println!("Hits: {}, Misses: {}", stats.hits, stats.misses);
```
