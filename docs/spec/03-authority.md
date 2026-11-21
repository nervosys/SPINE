# Layer 3: Authority

Status: **draft design, 2026-11-20. Nothing in this document is implemented.** It needs review before any code is written; the open questions at the end are real.

Layers 1, 2, 4 and 5 are described as built in the neighbouring files. This layer does not exist yet, so this file is a proposal.

## Purpose

Every action on SPINE must answer three questions from the request alone:

1. Who is asking?
2. On whose behalf, through which chain of delegation?
3. Is this exact action within what was delegated?

There is no ambient authority. Being connected, being on an allowlist, or holding a session grants nothing. A request with no valid grant chain is refused.

## Terms

| Term | Meaning |
| --- | --- |
| Party | Anything with a layer-2 identity name: a person's key, an organisation, an operator, an agent instance, a node. |
| Owner | The party whose identity is the authority of a resource name. Only an owner can issue a root grant for that resource. |
| Grant | A signed statement by an issuer that an audience may perform stated actions on stated resources within stated limits. |
| Chain | An ordered list of grants, root first, in which each grant's issuer is the previous grant's audience. |
| Invocation | A request signed by the last audience in a chain, citing that chain. |
| Receipt | A statement signed by the party that performed an invocation, recording what was done. |

Roles such as principal, operator and agent are labels on links in a chain, recorded for audit. They carry no power of their own.

## Grant

A grant is a map with deterministic encoding, signed by the issuer. The identifier of a grant is the hash of its signed bytes.

| Field | Type | Rule |
| --- | --- | --- |
| `v` | integer | Format version. Unknown versions are refused. |
| `iss` | identity name | Signer. |
| `aud` | identity name | The only party that may use or further delegate this grant. |
| `parent` | grant id or null | Null only in a root grant. |
| `role` | text | Audit label for `aud`: `principal`, `operator`, `agent`, `service`. |
| `res` | list of resource patterns | Exact names or a name plus path prefix. No other wildcards. |
| `act` | set | Subset of `read`, `invoke`, `publish`, `admin`. |
| `effect` | one of `read`, `reversible`, `irreversible` | Ceiling on the effect class (layer 5) of any operation invoked. |
| `delegable` | integer | How many further links may follow. Zero forbids delegation. |
| `nbf`, `exp` | time | Validity window. `exp` is mandatory. |
| `limits` | map | Optional ceilings: `calls`, `cost`, `bytes`, `rate`. |
| `caveats` | list | Optional further restrictions from a registered set. An unrecognised caveat causes refusal, never silent acceptance. |
| `nonce` | bytes | Makes otherwise identical grants distinct. |

### Attenuation

A grant with a parent is valid only if it is no broader than the parent in every field:

- every pattern in `res` is covered by a pattern in the parent;
- `act` is a subset;
- `effect` is no higher;
- `delegable` is strictly lower;
- the validity window lies inside the parent's;
- every limit is present and no higher wherever the parent sets one;
- every parent caveat is repeated.

Authority can therefore only shrink along a chain. A holder cannot grant what it does not hold.

## Invocation

An invocation carries:

- the chain, or grant ids for grants the recipient has already confirmed it holds;
- the operation and its arguments, or their hash;
- the recipient's identity, so it cannot be replayed to another party;
- a time and a nonce, so it cannot be replayed to the same party;
- a binding to the layer-1 session it was sent on;
- a signature by the audience of the last grant.

### Verification

The recipient refuses unless all of these hold, checked in this order so that cheap checks come first:

1. The chain is within length and size limits.
2. Every grant parses, has a known version, and has only recognised caveats.
3. The root grant's issuer is the owner of every resource the operation touches.
4. Each grant's issuer equals the previous grant's audience, and its `parent` equals the previous grant's id.
5. Every signature verifies against the issuer's current key under layer 2, including any key succession.
6. Every grant satisfies attenuation against its parent.
7. The current time is inside every validity window, allowing a stated clock skew.
8. No grant in the chain is revoked, to the recipient's stated staleness bound.
9. The invocation is signed by the last audience, names this recipient, is bound to this session, and its nonce is unseen.
10. The operation's resource, action and declared effect class fall within the last grant.
11. Stateful limits are not exhausted.

A refusal names the failed step and nothing else about the chain.

## Limits that need state

`calls`, `cost`, `bytes` and `rate` cannot be checked from the chain alone. The party that performs the operation keeps a counter for every grant id in the chain and charges an invocation against all of them. A delegate therefore cannot exceed its delegator's limit by splitting it among sub-delegates.

Limits are enforced per performing party. A grant covering resources served by several independent parties is limited separately at each one unless they share state. Grants intended to cap total spending should name one performing party.

## Revocation

An issuer revokes a grant by publishing a signed revocation record under its own name through layer 2, listing grant ids. Revoking a grant revokes everything below it.

Verifiers cannot always reach the network, so revocation is bounded, not instant:

- every grant has a mandatory expiry;
- a verifier declares the maximum age of revocation data it will accept for each effect class;
- the default for `irreversible` is that current revocation data is required and the operation is refused without it;
- short lifetimes with renewal are preferred to long lifetimes with revocation.

## Receipts

The performing party returns a signed receipt for every invocation that had an effect:

| Field | Content |
| --- | --- |
| `inv` | Hash of the invocation |
| `chain` | Grant ids used |
| `op`, `effect` | Operation and the effect class it had |
| `result` | Hash of the result, or the error |
| `cost` | What was charged against limits |
| `time` | When it was performed |

A receipt is the audit record. With the chain, it shows who delegated, who acted, what was done and what it cost, verifiable by a third party without trusting either side.

## Data cannot grant

Grants and invocations are a distinct message class at layer 1. Layer-4 objects have no field that can hold one. Content fetched by an agent can describe a grant, but nothing the stack does treats content as an invocation. This is the structural half of the defence against instruction injection; the other half is provenance labels in layer 4.

## How existing behaviour maps onto this

| Today | Under this design |
| --- | --- |
| Any client connected to the native server has operator-level access | Single-user mode: at first start the node issues itself a root grant over its own resources. Other clients need a grant from it. |
| Coordinator allowlist for signed mesh tasks | The worker's owner issues each coordinator a grant with `invoke` on the task interface. The allowlist becomes a set of issued grants. |
| Owner-scoped tasks and knowledge | Ownership is the root issuer of the chain that created the task. |
| Publication signed by the name's key | Unchanged for the owner. Delegated publication becomes possible through a `publish` grant. |

## Prior art considered

This is closest to authorization capabilities as in UCAN and ZCAP-LD. Macaroons were rejected because verification needs the root secret, so a third party cannot audit a chain. Biscuit's logic language is more expressive than this fixed field set; it was set aside for now because a fixed set is easier to specify, review and implement on constrained devices. These are references for reviewers, not dependencies.

## Open questions

1. **Offline revocation.** Is there a staleness bound that is both safe for reversible effects and usable without a reachable authority? Unresolved.
2. **Confused deputy.** A service holding many chains must use the caller's chain, never its own, when acting for a caller. The format allows this; whether the API makes the wrong thing hard is a layer-5 question.
3. **Privacy.** A chain reveals the whole delegation path to the recipient. Whether the principal's identity should be hideable from the performing party, while staying auditable later, is undecided.
4. **Post-quantum signatures.** Chains multiply signature size. Ed25519 is 64 bytes per link; ML-DSA is kilobytes. The session layer is moving to hybrid key exchange, but signatures on long-lived grants are the more exposed asset.
5. **Cost units.** `cost` needs a unit that means the same thing to both sides. Undefined until layer 5 defines declared cost.
6. **Human approval.** Where approval of an irreversible action attaches: as a caveat requiring a fresh co-signature from the principal, or as a short-lived grant issued per action. The second is simpler; the first is more flexible.
7. **Clock trust.** Validity windows assume roughly agreeing clocks. The skew allowance and behaviour for devices without a clock are unspecified.
