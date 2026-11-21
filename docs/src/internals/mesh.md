# Mesh Networking

SPINE agents can form peer-to-peer mesh networks for decentralized communication. The code is the `spine-mesh` crate; the handshake it runs is `spine-session`. This is a separate protocol from the native client wire (spine-wire v1, which has no mesh handshake and takes its confidentiality from TLS); the daemon reaches the mesh through the `mesh_call` and `*_name` commands.

## Overview

The mesh networking layer provides:

- **Ed25519 signing identity**: Cryptographic agent identity with fingerprinting
- **Peer-to-peer connections**: Direct TCP, WebSocket and QUIC connections between agents, plaintext or with the encrypted `spine-session` handshake (ML-KEM-768 and Ed25519, AES-256-GCM frames; custom cryptography without external review)
- **Name records**: Signed name records and their lookup RPC (`spine-name`, `naming_mesh`)
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
| `MeshPayload` | Message variants: agent messages, peer gossip, ping/pong, route request/response, knowledge sync, swarm invite/response, name records and resolution, and versioned application RPC |

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

Every `MeshEnvelope` is signed by the sender using Ed25519. Recipients verify the signature against the sender's public key from the trusted key store, rejecting messages from unknown or banned peers. The signature (v3) covers the domain string `spine-mesh-envelope-v3` and the deterministic CBOR (`spine-cbor`) of the array `[id, from, to, timestamp_secs, timestamp_nanos, payload]`, so the signed bytes depend on the values and not on JSON text formatting; an envelope signed under an earlier version does not verify; `ttl` and `hops` are rewritten by forwarders and are not signed, so treat them as advisory.