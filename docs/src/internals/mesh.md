# Mesh Networking

SPINE agents can form peer-to-peer mesh networks for decentralized communication.

## Overview

The mesh networking layer provides:

- **Ed25519 signing identity**: Cryptographic agent identity with fingerprinting
- **Peer-to-peer connections**: Direct TCP connections between agents
- **Multi-hop routing**: Shortest-path message forwarding with TTL
- **Gossip protocol**: Peer announcement propagation for network discovery
- **Message deduplication**: Ring buffer preventing routing loops

## Core Types

| Type | Purpose |
|------|---------|
| `Ed25519Keypair` | Signing keypair (ed25519-dalek) |
| `SigningIdentity` | Agent identity with signing and verification |
| `MeshNode` | Network node with peer management and routing |
| `MeshEnvelope` | Signed message container with source, target, hops, TTL |
| `MeshTarget` | Destination: Agent(id), Broadcast, Capability(name) |
| `MeshPayload` | 9 message variants (Data, Query, Discovery, Gossip, etc.) |

## Routing

Messages are routed via shortest-path selection from the routing table. Routes are learned from incoming messages and gossip announcements. Stale routes are pruned periodically.

```text
Agent A → MeshNode → [hop] → MeshNode → Agent B
              ↓                   ↑
          Route Table          Route Table
```

## Gossip Discovery

Nodes periodically broadcast `PeerAnnouncement` messages containing their identity, capabilities, and known peers. Announcements propagate through the network with max-hop limits, allowing new nodes to discover the full topology.

## Signature Verification

Every `MeshEnvelope` is signed by the sender using Ed25519. Recipients verify the signature against the sender's public key from the trusted key store, rejecting messages from unknown or banned peers.