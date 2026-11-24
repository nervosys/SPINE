# Layer 3: Authority

Status: **draft design, 2026-11-20. Parts one and two are implemented as a pure library (`src/spine-authority`: grants, chains, attenuation, the eleven verification steps, signed revocation records and feed heads, signed receipts, an atomic in-memory usage ledger, vectors). A node-side enforcement component (`spine_core::authority`: a classification of the 19 commands, a gate that verifies, binds the invocation to the command and charges, and the single-user root grant) is implemented and unit-tested as a library module and is not connected: spine-wire v1 cannot carry a grant chain, so the daemon checks no grant and `authority.mode = "required"` is refused at startup. Nothing is wired into the mesh, SDK or CLI, and no revocation or receipt is distributed over any network.** The choices the code had to make are under "Provisional decisions" and need review; the open questions at the end are real and none is resolved.

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

An issuer revokes a grant by publishing a signed revocation record under its own name through layer 2, listing grant ids. Revoking a grant revokes everything below it. The record format, who may revoke and the feed head are specified under "Revocation records, feed heads, receipts and the ledger (part two)"; the draft text here is superseded where they differ (P19 to P23).

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

A receipt is the audit record. With the chain, it shows who delegated, who acted, what was done and what it cost, verifiable by a third party without trusting either side. The implemented receipt differs from this table (P24, P25); see part two.

## Revocation records, feed heads, receipts and the ledger (part two)

This section is normative for the library. It defines the objects and how each is checked; it does not define how they are distributed. Encodings follow the grant conventions: deterministic CBOR (`spine-cbor`), text keys, byte strings for keys, ids, digests and signatures, closed maps (an unknown or missing key is refused), each object a map `{body, sig}` whose signature is Ed25519 (strict verification) over a domain string followed by the CBOR of `body`. Every domain string ends with one NUL byte (`\x00`).

### Revocation record

Domain `spine-authority-revocation-v1`. Maximum encoded size 512 bytes.

| Key | CBOR type | Rule |
| --- | --- | --- |
| `v` | unsigned integer | Format version, 1. Unknown versions are refused. |
| `revoker` | byte string, 32 | The signer. |
| `grant` | byte string, 32 | Id of the one grant being revoked. |
| `revoked_at` | unsigned integer | When the statement was made, Unix seconds. |
| `nonce` | byte string, 16 | Makes otherwise identical records distinct. |

The id of a record is SHA-256 over `spine-authority-revocation-id-v1` (with its NUL) and the whole encoded record.

**Who may revoke, and how a verifier checks it.** The issuer of the revoked grant, or the issuer of any grant above it in the chain (the root's issuer may revoke any grant below the root). The record carries no chain, so the right is checked at step 8 against the chain being verified: a record counts for that chain only if the revoked grant is in the chain and the revoker is the `iss` of that grant or of a grant earlier in the chain. A record whose revoker issued nothing at or above the revoked grant in the presented chain is ignored for that chain. Because step 8 asks about every grant of the chain, revoking a grant revokes everything delegated from it. A revocation is permanent; there is no record that undoes one.

### Feed head and feed

A feed is a set of revocation records with a head signed by its publisher. Domain of the head `spine-authority-feed-head-v1`; maximum encoded size 512 bytes.

| Key | CBOR type | Rule |
| --- | --- | --- |
| `v` | unsigned integer | Format version, 1. |
| `publisher` | byte string, 32 | The signer. A verifier accepts heads from one key it trusts. |
| `seq` | unsigned integer | Increases with every head the publisher issues, including heads that add no record. |
| `as_of` | unsigned integer | The set is complete as of this Unix time (seconds). |
| `count` | unsigned integer | Number of records. |
| `digest` | byte string, 32 | SHA-256 over `spine-authority-feed-digest-v1` (with its NUL) and the ids of all records, sorted ascending as byte strings, concatenated. |

A verifier loads a feed from the records, the head, the trusted publisher key, the highest `seq` it has already accepted (or none) and its clock. The load is refused, with the first of these that applies:

1. the head or a record does not decode as the canonical encoding;
2. the head's version is unknown, its publisher is not the trusted key, or its signature does not verify;
3. `seq` is not above the last accepted `seq` (rollback);
4. `as_of` is after the verifier's clock;
5. there are more than 16384 records, a record appears twice, the count differs from the head's, or the digest differs from the head's;
6. a record has an unknown version or a bad signature, or a `revoked_at` after the head's `as_of`.

A loaded feed is immutable. A feed that has not loaded is the same as no data: step 8 refuses with `RevocationDataStale` under any bound, because the default bounds are zero and no data has no age.

**Step 8 with a feed.** The feed answers `Revoked` for a chain if some record in it names a grant of the chain and its revoker satisfies the rule above. Its freshness is `now - as_of`; the existing per-effect-class bounds (P8) are compared with it.

**What the head does and does not do.** It lets a verifier tell that a set has not been truncated, padded or paired with a head from another time, and that the data is as fresh as the publisher says. It does not stop a publisher from leaving a record out of every head it signs, from showing different verifiers different feeds, or from signing a fresh head while cut off from the revokers; it does not help a verifier that has lost its last accepted `seq` (it can then still be shown an old head, limited only by the staleness bounds); and it says nothing about records the publisher has not yet received, which is what the staleness bound is for.

### Receipt

Domain `spine-authority-receipt-v1`. Maximum encoded size 512 bytes. Signed by the invocation's `recipient`.

| Key | CBOR type | Rule |
| --- | --- | --- |
| `v` | unsigned integer | Format version, 1. |
| `recipient` | byte string, 32 | The signer, the party that performed the invocation. |
| `invocation` | byte string, 32 | SHA-256 over `spine-authority-invocation-id-v1` (with its NUL) and the whole encoded signed invocation. |
| `leaf` | byte string, 32 | Id of the last grant, as the invocation cites it. |
| `allowed` | boolean | Whether the invocation was allowed and performed. |
| `effect` | text or null | The effect class performed; null when denied. |
| `step` | unsigned integer or null | The verification step that refused, 1 to 11; null when allowed. |
| `calls`, `cost`, `bytes` | unsigned integers | Amounts charged. When allowed, `calls` is 1. When denied, all three are 0. |
| `result` | byte string, 32 | Digest of the result, opaque to this layer. All zero when denied. |
| `time` | unsigned integer | When it was performed or refused, Unix seconds. |

A denial receipt carries the step number and nothing else about why: no denial variant, no amounts, no result. Two denials at the same step give the same receipt body. A receipt is refused as malformed if `allowed` is inconsistent with the other fields, as above.

To check a receipt a party needs the expected recipient key. `verify` checks the form, the recipient and the signature. `matches` checks that the receipt is for a given invocation (its id), names the invocation's last grant and recipient, and records exactly the outcome and amounts of a given verification result; it does not compare the result digest. With the chain, the invocation and the receipt, a third party can check who delegated, who acted, what was done and what it was charged.

### Usage ledger

Charging is one operation on the performing party's ledger. The `Decision` that verification returns carries, for each grant of the chain in order, its id, limits and expiry, and the nonce's replay horizon: the last time at which an invocation accepted now could still pass step 9's freshness check, `now + skew + maximum invocation age`. A charge:

1. is refused if the decision is malformed (lists of different lengths, or a grant twice);
2. forgets nonces whose horizon has passed;
3. is refused if the invoker's nonce is recorded;
4. is refused if, for any grant of the chain that has a limit, one more call and the declared cost and bytes would pass `calls`, `cost`, `bytes`, or `rate` (calls in the 60 seconds before `now`, counted per second);
5. is refused if the ledger is at a bound and holds nothing that may be forgotten;
6. otherwise records the nonce and charges every limited grant of the chain.

Steps 1 to 5 change nothing visible, so a refused charge changes nothing: either every grant is charged and the nonce is recorded, or none. A nonce is forgotten only after its horizon, so forgetting can never let through a replay that step 9 would have accepted; when the table is full of nonces still inside the window the ledger refuses (fail closed) rather than evict one. A grant's usage record may be dropped once the grant has expired, since step 7 refuses it from then on. The in-memory ledger bounds nonces (default 65536), grants with records (default 16384) and, per grant, one counter per second of the rate window. The ledger trusts the clock it is given and assumes it does not run backwards.

## Node enforcement (not yet on the wire)

This section describes a component that exists as a library module and is not connected to anything: `spine_core::authority` (`src/spine-core/src/authority.rs`). It is what a node will run on every command once a request can carry a grant chain. Today no request can. spine-wire v1 has no field for a chain or an invocation and states that adding one is a new version (`00-wire.md`, G8), and layer 1 defines no session value an invocation could be bound to. The daemon therefore checks no grant, and every connected client still has full control of the node, exactly as before.

### Resource model

Every command is an operation on the node's own resource tree. The owner is the node's public key; the path is `["node", <area>]`, with nine areas: `session`, `outbound`, `binaries`, `knowledge`, `names`, `resources`, `status`, `mesh`, `tools`. A grant with the prefix pattern `["node"]` covers the whole node; a grant on `["node", "names"]` covers the name service only. Because step 3 requires the root's issuer to own every resource, only a chain rooted at the node's key can authorise a command.

A grant on `["node", "names"]` is the node saying who may use its name service. It says nothing about who owns a name: `publish_name` and `publish_resource` carry a record signed by the name's own key, and layer 2 checks that signature as it does today.

### Command classification

`classify` is a total function from a command to an action, an effect class and a node resource. Its match has no wildcard arm, so a command cannot be added without being classified. The rule for the effect class:

- `read`: nothing leaves the node and nothing on it changes.
- `reversible`: only the node's own state changes, in a way a later command or an expiry undoes, or the node sends namespace lookups whose form the caller does not control.
- `irreversible`: something leaves the node that the caller shaped and the node cannot recall (an HTTP request to a URL the caller chose, a call or a publication signed with the node's key), or code runs whose effect the node cannot bound.

Anything that makes the node send traffic, sign, execute or store is not the action `read`. No command needs `admin` yet.

| Command | Action | Effect | Resource | Reason |
| --- | --- | --- | --- | --- |
| `navigate` | `invoke` | `irreversible` | `node/outbound` | The node sends an HTTP request to a URL the caller chose; a request that has been sent cannot be recalled. |
| `get_ur` | `read` | `read` | `node/session` | Returns the page already loaded in the session. |
| `get_raw_html` | `read` | `read` | `node/session` | Returns the HTML already loaded in the session. |
| `execute_binary` | `invoke` | `irreversible` | `node/binaries` | Runs a caller-supplied program that may navigate, search and store; the node does not bound its effect. |
| `store_knowledge` | `invoke` | `reversible` (`irreversible` in a `research` build) | `node/knowledge` | Writes an entry to the node's store; a `research` build also broadcasts it to the cluster. |
| `query_knowledge` | `read` | `read` | `node/knowledge` | Reads the node's store. |
| `get_binary_capabilities` | `read` | `read` | `node/session` | Returns the capability list of the session's binary. |
| `resolve_name` | `invoke` | `reversible` | `node/names` | May send a mesh lookup and caches the record; the cache entry expires. |
| `resolve_names` | `invoke` | `reversible` | `node/names` | As `resolve_name`, for up to 256 names. |
| `find_providers` | `invoke` | `reversible` | `node/names` | Sends a capability lookup to the mesh and caches what comes back. |
| `publish_name` | `publish` | `irreversible` | `node/names` | The node stores a signed record and announces it; peers keep it until it expires. |
| `fetch_name` | `invoke` | `reversible` | `node/names` | Resolves the name and returns its endpoints; it dials nothing. |
| `crawl_names` | `invoke` | `reversible` | `node/names` | Resolves a bounded set of linked names. |
| `publish_resource` | `publish` | `irreversible` | `node/resources` | The node stores bytes and announces their signed binding. |
| `read_resource` | `read` | `read` | `node/resources` | Reads bytes from the node's own store. |
| `namespace_status` | `read` | `read` | `node/status` | Returns the node's identity and its peer list. |
| `mesh_call` | `invoke` | `irreversible` | `node/mesh` | The node signs a call with its own key and sends it to a peer, which may do anything with it. |
| `tool_call` | `invoke` | `irreversible` | `node/tools` | Runs a tool. The two tools that exist are pure, but the class must hold for any tool registered later. |
| `capability_query` | `read` | `read` | `node/tools` | Lists what the node offers. |

### The gate

`AuthorityGate` holds the node's public key (the recipient), the trusted roots, the verification settings (skew, maximum invocation age, revocation staleness bounds), one usage ledger shared by all callers and one revocation set. `authorize` takes the encoded chain, the encoded invocation, the command about to run, the hash of the command body, the session value and the time. It refuses unless all of these hold, in this order:

1. The eleven verification steps pass (`verify_encoded_with_invocation`, which returns the decoded invocation so the gate decodes it once) against this node's key, the session value, the trusted roots, the revocation set and the ledger.
2. The invocation's `act` equals the command's action.
3. The invocation's declared `effect` is the command's class or a higher one, under the order `read` < `reversible` < `irreversible`. An invoker may declare more than the command needs, never less.
4. The invocation's `res` is exactly one resource, the command's node resource on this node. A wider resource, a deeper one, or an extra one is refused.
5. The invocation's `op` equals the hash of the command body.
6. The ledger records the nonce and charges every limited grant of the chain.

Checks 2 to 5 are the part of step 10 that the library cannot do, because it does not know what the operation is (P14). The ledger is locked from before check 1 until after check 6, so of two concurrent requests with one nonce, or two that would together pass a limit, exactly as many are accepted as the grants allow. A refusal at any point records no nonce and charges nothing.

The operation hash is the SHA-256 of the exact bytes of the command body as they were on the wire. The node computes it from what it received; it is never taken from the caller. How the bytes reach the gate is the carrier's business.

A refusal gives the caller a step number (as a stable name, `authority_step_1` to `authority_step_11`) and nothing else. A failure of checks 2 to 5 is reported as step 10; a replayed nonce as step 9; a full ledger as step 11. The full reason is kept for the node's own log.

`receipt` signs a receipt for an authorised invocation once the command has run. The gate issues no receipt for a refusal.

### Single-user mode

`issue_self_root` is a pure function: the node's key grants an audience key the whole node tree, all four actions, the effect ceiling `irreversible`, no limits, the role `principal`, a validity window and a nonce the caller supplies, and delegation through as many links as a chain allows (7). A gate in single-user mode trusts the node's own key as the only root. The holder of that grant delegates narrower grants to agents.

### Configuration

The `[authority]` section of the node's configuration is parsed and validated and has no effect:

| Key | Default | Rule |
| --- | --- | --- |
| `mode` | `"off"` | `"off"` or `"required"`. `"required"` is refused at startup in this build, with a message that the carrier does not exist, so that no operator believes enforcement is on when it is not. |
| `trusted_roots` | `[]` | Ed25519 public keys, 64 hex characters each, no key twice. Accepted as roots in addition to the node's own key. Under the resource model above they cannot authorise any command. |
| `skew_secs` | `0` | At most 300 (P7). |
| `max_invocation_age_secs` | `60` | 1 to 3600 (P11). |

An unknown key in the section is refused.

### What is not done

- **Carrier.** No frame, header or command carries a chain or an invocation. The daemon does not call the gate.
- **Session binding.** Layer 1 defines no session value. The gate takes 32 bytes as a parameter.
- **`did:`-rooted resources named by commands.** `publish_name` and `publish_resource` name a resource owned by the record's key. The gate does not extract it, so it cannot check a delegated `publish` grant from a name's owner. The record's own signature is still checked by layer 2.
- **Durable ledger.** The ledger is in memory. A restart forgets every nonce and every count, so a limit can be passed again and an invocation replayed inside its freshness window.
- **Revocation feeds.** The gate's revocation set is a local list of ids the operator vouches for, complete as of a time the caller states. It starts with no data and refuses everything until told otherwise. No signed record is checked and no feed is fetched.
- **Receipt distribution.** A receipt can be signed; nothing returns it to the caller or stores it.
- **`mode = "required"`.** Refused at startup.
- **Per-argument resources.** The resource does not depend on the command's arguments: a grant cannot say which URLs, which tools or which names.
- **Fixed: the invocation was decoded twice.** `verify_encoded` discarded the invocation it decoded and the gate decoded it again. `verify_encoded_with_invocation` returns it with the `Decision`, the gate uses it, and `verify_encoded` is a wrapper over it.

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

## Provisional decisions

The pure core of this layer is implemented in `src/spine-authority` (grant and invocation types, one canonical encoding, attenuation, the eleven verification steps, revocation records and feeds, receipts, the usage ledger, independent vectors in `03-authority.vectors.json`). The draft above left several points open or ambiguous. The code decided each one in the most conservative direction so that it could be written. **None of these is settled**; each row states the alternative and what would change. The crate README lists the encoding, the bounds and the scope rule in full.

| # | Decision | Alternative, and what changes |
| --- | --- | --- |
| P1 | A party (`iss`, `aud`, resource owner, invoker, recipient) is a 32-byte Ed25519 public key, the key of a `did:` authority, not a full name. | Full names allow a grant to follow a rotated key but need layer-2 key succession (see P9) and change the types of `iss`, `aud` and `owner`. |
| P2 | A resource is `(owner key, path segments)`. Names with a query, a fragment or a trailing slash are refused as resources, as are segments that are empty, `.`, `..`, contain `/` or a control character; `blob:`, `cap:` and `host:` names cannot be resources. | Accepting them needs a rule for whether `/a/` and `/a` are one resource (layer 2 keys them differently) and whether a query narrows a pattern. |
| P3 | A prefix pattern contains its own base path as well as everything below it, compared segment by segment: `/data` contains `/data` and `/data/x`, never `/database`. | Strictly-below needs a second pattern form to name the base; the attenuation tables change. |
| P4 | `role` is a closed set of four labels (`principal`, `operator`, `agent`, `service`); `nbf` is mandatory; `res` and `act` are non-empty; `act` is a strictly ascending set. | Open roles, an optional `nbf` or unordered sets give several encodings of one meaning, so the canonical-form check would need to normalise. |
| P5 | Step 3 also requires the root's issuer to be in a list of trusted roots the verifier supplies (empty trusts none), in addition to owning the resources. | For `did:` names ownership is derivable from the name, so trusted roots could become an optional narrowing. |
| P6 (Q6) | The caveat registry is empty. Any caveat fails step 2 (`UnknownCaveat`) and the builders refuse to issue one. The rule that a child repeats every parent caveat is implemented and tested. | Registering caveats (for example a fresh co-signature from the principal for irreversible actions) adds names with defined arguments and a check at step 10 or 11. Approval by short-lived per-action grants needs no caveat. |
| P7 (Q7) | Time is Unix seconds (`u64`) passed in by the caller. Skew defaults to 0, is clamped to 300 seconds, and softens only the start of a validity window and the future bound of an invocation, never an expiry. | Symmetric skew accepts a grant just after it expired when the verifier's clock runs slow. Behaviour for devices with no clock is not defined. |
| P8 (Q1) | Revocation staleness bounds default to 0 for every effect class (only data as of now is accepted; no data refuses). A verifier raises a bound per class. Revocation distribution and record format are not implemented. | The draft defaults only irreversible effects to current data. Looser defaults for read and reversible make offline use easier. The bounds are already per class, so only defaults change. |
| P9 | Step 5 verifies against `iss` itself; layer 2 has no key succession. | When it exists, a fail-closed key-view parameter is added to `verify`. |
| P10 | An invocation cites the id of the last grant (`leaf`) and the full chain is supplied. The draft's "ids of grants the recipient already holds" is not modelled. | A confirmed-grant cache keyed by id saves bytes and bears on the privacy question (Q3); it adds a lookup before step 2 and a rule for cached expiry. |
| P11 | An invocation is accepted only if its `time` is at most 60 seconds in the past (configurable) and not beyond now plus skew. Nonces are 16 bytes supplied by the caller (the crate has no RNG) and unique per invoker. | The window bounds how long a ledger must remember nonces. A different window changes only the context. |
| P12 (Q5) | `calls` counts one per invocation; `cost` and `bytes` are the amounts the invocation declares; `rate` is calls per rolling 60 seconds. All are charged against every grant of the chain. `Some(0)` is a real limit. A grant without limits does not consult the ledger. | `cost` has no unit until layer 5 defines declared cost. Charging only the leaf would let a delegate exceed its delegator by splitting. |
| P13 | Verification is read-only. The caller records the nonce and charges (`Decision`) in the same atomic step that performs the operation. | A ledger that charges inside `verify` hides the atomicity requirement but forces a mutable, I/O-capable parameter into a pure function. |
| P14 | The invocation's `effect` is the class the invoker declares; the verifier does not know the true class of an operation. | A layer-5 registry from operation to effect class would let step 10 compare a computed class. |
| P15 | A grant id is SHA-256 over a domain and the whole signed encoding including the signature. | The draft says "hash of its signed bytes" without saying whether the signature is included. With deterministic Ed25519 the two coincide in practice. |
| P16 | `session` is an opaque 32-byte value supplied by the verifier and signed in the invocation. | Layer 1 may define a handshake hash or channel id; the field type may change. |
| P17 | A refusal is one of several variants per step (`Denial::step` gives the step number alone, as the draft says). | Reporting only the step follows the draft sentence literally and loses information a caller may want. |
| P18 | `delegable` is a `u32`, strictly lower per link; a chain has at most 8 grants and an encoded grant at most 4096 bytes. | Different caps change constants only. Larger post-quantum signatures (Q4) would not fit the grant bound. |
| P19 (Q1) | Who may revoke: the issuer of the revoked grant, or of any grant above it in the chain. The right is checked at step 8 from the chain presented, so a record names only a grant id and a revoker key. | The draft says only "an issuer revokes a grant by publishing a record under its own name". Letting only the grant's own issuer revoke means a root cannot cut off a sub-delegation without the cooperation of the intermediate. Letting the audience also surrender a grant it holds adds `aud` to the authorised set; no format change. |
| P20 (Q1) | A record revokes exactly one grant id, carries an informational `revoked_at` (never after its head's `as_of`) and a nonce, and is permanent. Descendants are covered because step 8 checks every grant of the chain. | The draft says a record lists grant ids: one record for many ids saves signatures and makes partial publication possible. A reinstatement record would need a rule for revoke and reinstate in the same feed. |
| P21 (Q1) | Feeds: one trusted publisher key per feed, a head with a strictly increasing sequence, an `as_of` time, a count and a digest over the sorted record ids; a feed loads all or nothing, is immutable, and a feed that fails to load is no data. | The draft publishes each record "under its own name" through layer 2, which means one feed per issuer. A Merkle structure would allow inclusion proofs and incremental update; a transparency log would also expose a publisher that shows different feeds to different verifiers. The head here does neither. |
| P22 (Q1, Q7) | Freshness is the head's `as_of` against the existing per-effect-class bounds (P8, default 0). With no last-accepted sequence (first contact, or lost state) a rolled-back head cannot be detected; only the bounds limit its age. | Requiring a persisted last-accepted sequence before any use would close that, at the cost of a bootstrap rule. |
| P23 | `RevocationView` gains `chain_revoked(chain, ids)`, which step 8 calls instead of asking about each id; its default asks `is_revoked` for each id, so existing views behave as before. `RevocationFeed` overrides it to check each record's revoker against the chain. | Requiring `is_revoked` itself to be chain-aware is impossible, since it takes only an id. Filtering the feed against the chain before verification would move the same check out of the pure function. |
| P24 (Q3) | A receipt cites the invocation by hash and the last grant by id; it does not repeat the chain or the operation hash (both are reachable from the invocation). The invocation id is SHA-256 over its own domain and the signed encoding, as for grants (P15). | The draft lists the whole chain and the operation in the receipt: a receipt then stands alone and an auditor sees the delegation path without the invocation, at the cost of size and of disclosing more (Q3). |
| P25 | A receipt may record a denial (the step number only, zero amounts, zero result), not only an invocation that had an effect. Whether to issue one is the performing party's choice. | The draft has receipts only for invocations that had an effect. A signed refusal lets a requester prove it was refused; but anyone able to send an invocation can then demand a signature, so a performer under load may choose not to issue them. |
| P26 (Q5, Q7) | The ledger charges every limited grant of the chain and records the nonce atomically; a grant with no limit is not tracked; a nonce is kept until `now + skew + maximum invocation age` and is dropped only after that; a full table refuses to charge instead of evicting a live record; usage records go when the grant expires; `charge` returns an error and `Decision` carries the chain's limits, expiries and the nonce horizon. | Evicting the oldest nonce when full is simpler and bounds memory without refusing, but admits a replay inside the freshness window. A persistent or shared ledger needs the same atomicity from its store, which this crate cannot provide. |
| P27 | Size bounds: revocation, feed head and receipt at most 512 bytes each, a feed at most 16384 records, the in-memory ledger 65536 nonces and 16384 grant records by default. | Changing them changes constants only. A larger signature (Q4) would need larger object bounds. |
| P28 | A command is an operation on the node's own resource tree: owner the node's key, path `["node", <area>]`, nine areas, one resource per command, independent of the command's arguments. Only the node's key can root a chain that authorises a command. | One resource per command name gives finer grants but puts 19 names into grants and changes what a grant means whenever a command is added. Resources derived from arguments (the host of a `navigate` URL, the tool of a `tool_call`, the name being resolved or published) are finer still and need a canonical map from arguments to path segments; for `publish_name` and `publish_resource` that map is the `did:` name itself and brings in roots other than the node. |
| P29 | Classification is conservative: a command that makes the node send traffic, sign, execute or store is never the action `read`, so resolution is `invoke`; a request to a caller-chosen URL, a signed call, a publication and the running of a program or a tool are `irreversible`; `tool_call` has one class whatever the tool; `store_knowledge` is `irreversible` only in a `research` build. | HTTP defines `GET` as safe, so `navigate` could be `reversible`. A lookup could be `read`, since it changes nothing a caller can observe later. `tool_call` could take its class from the tool registry, where the two existing tools are pure. Each changes rows of the table, and with them which grants and which revocation staleness bound a command needs. |
| P30 | The gate requires the invocation's action to equal the command's, its declared effect to be the command's class or higher, its resources to be exactly the command's one resource, and its `op` to equal the hash of the command body. | Requiring the effect to be equal refuses an invoker that over-declares; over-declaring only costs the invoker a higher ceiling and a stricter staleness bound. Allowing a superset of resources accepts an invocation that claims to touch more than the command does. |
| P31 | A failure of the command binding is reported as step 10, a charge refused for a replayed nonce as step 9, any other charge failure as step 11. The caller is given `authority_step_N` and nothing else. | A twelfth step or a separate error for "this invocation is for another command" tells an honest client that its grant is wide enough and its request was built wrongly. That is more than "names the failed step and nothing else" allows. |
| P32 | `op` is the SHA-256 of the exact command body bytes as received, with no domain string, computed by the node. | A hash of a canonical re-encoding gives the same value for deterministic CBOR and is one more thing to get wrong. A domain-separated hash that also covers the request id binds the invocation to one request, not to one body; the nonce already makes an invocation single-use. |
| P33 | One lock is held across verification, the command binding and the charge, so all authorisations on a node run one at a time, signature checks included. | Verifying outside the lock and relying on the charge to re-check the nonce and the limits (it does both) runs verifications in parallel and gives the same outcomes and step numbers. Only throughput changes. |
| P34 | The single-user root grant is from the node's key to an audience key over the whole node tree: four actions, ceiling `irreversible`, no limits, delegable through 7 links, window and nonce supplied by the caller. It is not written to disk by this layer. | A narrower default (no `admin`, ceiling `reversible`) is safer and makes the first delegation of an irreversible command impossible without a second root. Issuing the grant to the node's own key keeps one key but needs the node's key online to sign every delegation. |
| P35 | The gate's revocation data is a local set of grant ids the operator vouches for, with a completeness time the caller states and that only moves forward. A new gate has no data and refuses everything at step 8. | Holding a signed feed (P21) needs something to fetch it. Treating a single-user node's own set as always current removes the need to state a time, but an intermediate issuer may revoke (P19) and the node would not learn of it. |
| P36 | Configuration: `[authority]` with `mode` `off` (default) or `required`; `required` is refused at startup until a carrier exists; unknown keys in the section are refused; `skew_secs` at most 300; `max_invocation_age_secs` 1 to 3600. | Accepting `required` and refusing every command fails closed and gives an operator a way to lock a node, but looks like working enforcement. A log-only mode needs the carrier as well. |

Open questions touched (none is resolved): 1 (P8, P19 to P23, P35), 4 (P18 and P27, bounds assume 64-byte signatures), 5 (P12, P26), 6 (P6), 7 (P7, P22, P26, P36). Question 3 (privacy) is touched by P10 and P24. Question 2 (confused deputy) is not touched. P28 to P36 are the choices of the node enforcement component and are no more settled than the rest.

## Open questions

1. **Offline revocation.** Is there a staleness bound that is both safe for reversible effects and usable without a reachable authority? Unresolved.
2. **Confused deputy.** A service holding many chains must use the caller's chain, never its own, when acting for a caller. The format allows this; whether the API makes the wrong thing hard is a layer-5 question.
3. **Privacy.** A chain reveals the whole delegation path to the recipient. Whether the principal's identity should be hideable from the performing party, while staying auditable later, is undecided.
4. **Post-quantum signatures.** Chains multiply signature size. Ed25519 is 64 bytes per link; ML-DSA is kilobytes. The session layer is moving to hybrid key exchange, but signatures on long-lived grants are the more exposed asset.
5. **Cost units.** `cost` needs a unit that means the same thing to both sides. Undefined until layer 5 defines declared cost.
6. **Human approval.** Where approval of an irreversible action attaches: as a caveat requiring a fresh co-signature from the principal, or as a short-lived grant issued per action. The second is simpler; the first is more flexible.
7. **Clock trust.** Validity windows assume roughly agreeing clocks. The skew allowance and behaviour for devices without a clock are unspecified.
