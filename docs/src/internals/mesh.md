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

> Only the name-resolution and application payloads are dispatched by any transport. `MeshNode` still has the routing table, TTL and gossip helpers, but no entry point forwards envelopes or handles gossip, and `MeshNode::process_envelope` (which verified signatures only for senders already in a trust map and let unknown senders through) was removed.

Messages are routed via shortest-path selection from the routing table. Routes are learned from incoming messages and gossip announcements. Stale routes are pruned periodically.

```text
Agent A → MeshNode → [hop] → MeshNode → Agent B
              ↓                   ↑
          Route Table          Route Table
```

## Gossip Discovery

Nodes periodically broadcast `PeerAnnouncement` messages containing their identity, capabilities, and known peers. Announcements propagate through the network with max-hop limits, allowing new nodes to discover the full topology.

## Identity Pinning and QUIC

Updated 2026-11-21. A node id is the peer's Ed25519 public key, so a hello, ack or referral that names a peer also names the key a dial to it must prove. Transports that run the mesh handshake record it (`NameTransport::learn_peer`; the first claim stands) and pass it as the expected peer, and a responder that proves another key fails the dial with `NameMeshError::PinMismatch`, which is logged and counted. A bare seed address is accepted on first contact, its key recorded and required on every later dial in this process; `namespace.seed_keys` in the configuration supplies a seed's key from the first dial. Pins are kept in memory only. See the trust model in the `spine-mesh` README for the full table.

Over QUIC, TLS 1.3 encrypts and authenticates the endpoint (certificates given to `trust_certificate`), and the mesh handshake on the first stream says which key is on the connection. That key is kept for the life of the connection and used for every later stream, so `Application` RPC works over an authenticated QUIC transport and a connection that fails the handshake is closed. The mesh session keys are not used to seal QUIC streams. The handshake is not bound to the TLS channel; the mitigations are trusting only given certificates and pinning the expected key.

## Signature Verification

Every `MeshEnvelope` is signed by the sender using Ed25519. A receiver does not take `from` at its word: handlers whose decision depends on who sent an envelope verify the signature against a key this node recorded for that agent id (and, on an authenticated connection, against the connection's own key) and drop what does not verify. The variants are listed with what each handler requires in the trust model section of the `spine-mesh` README. The signature (v3) covers the domain string `spine-mesh-envelope-v3` and the deterministic CBOR (`spine-cbor`) of the array `[id, from, to, timestamp_secs, timestamp_nanos, payload]`, so the signed bytes depend on the values and not on JSON text formatting; an envelope signed under an earlier version does not verify; `ttl` and `hops` are rewritten by forwarders and are not signed, so treat them as advisory.