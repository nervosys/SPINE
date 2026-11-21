# Neural Protocol

## Overview

The neural protocol enables learned message encoding using the Titans architecture. Messages are projected into a latent space for efficient transmission and optional cryptographic protection.

## Neural Transmission

The `transmit_neural` call, the latent-page command and the agentic-state
command are not part of the core protocol. They are recorded as extension
candidates in `research/spine-protocol-research/README.md`.

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
