//! Node-side enforcement of layer 3 (`docs/spec/03-authority.md`, "Node
//! enforcement (not yet on the wire)").
//!
//! `spine-authority` decides whether an invocation is inside a chain of
//! grants. This module adds what only the node knows: which of its commands
//! the invocation is for. It has three parts.
//!
//! * [`classify`] maps every [`BrowserCommand`] to the action, the effect
//!   class and the node resource a caller must hold a grant for.
//! * [`AuthorityGate`] verifies an encoded chain and invocation, checks that
//!   the invocation covers the command actually being run, and charges the
//!   usage ledger, all under one lock.
//! * [`AuthorityGate::issue_self_root`] issues the root grant of single-user
//!   mode: the node's key grants an audience its whole resource tree.
//!
//! # Not connected
//!
//! Nothing in the daemon calls this module. spine-wire v1 has no field that
//! carries a grant chain or an invocation, and layer 1 defines no session
//! value to bind one to, so there is nothing for the daemon to pass in. The
//! carrier is a later step; until then `authority.mode = "required"` is
//! refused at startup ([`crate::config::AuthorityConfig`]).
//!
//! # What the carrier must supply
//!
//! * `chain_bytes` and `invocation_bytes`: the canonical encodings, as sent.
//! * `op_hash`: the SHA-256 of the exact bytes of the command body on the
//!   wire ([`operation_hash`]), computed by the node from what it received,
//!   never taken from the caller.
//! * `session`: the 32-byte value layer 1 will define for the connection.
//! * `now`: the node's clock, Unix seconds. It must not run backwards.
//!
//! # The resource model
//!
//! Every command is an operation on the node's own resource tree. The owner
//! is the node's public key and the path is `["node", <area>]`, where the
//! area is one of the nine in [`NodeArea`]. Only the node's key can root a
//! chain over that tree (verification step 3), so a grant over
//! `["node", "names"]` is the node saying who may use its name service; it
//! says nothing about who owns a name. `publish_name` and `publish_resource`
//! also name a `did:`-rooted resource owned by the record's key; that
//! resource is not extracted or checked here.

use std::sync::{Arc, Mutex, PoisonError, RwLock};

use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use spine_authority::{
    verify_encoded_with_invocation, Action, ChargeError, Decision, Denial, Effect, Grant, GrantId,
    GrantSpec, Invocation, IssueError, LedgerConfig, Limits, MemoryLedger, PublicKey, Receipt,
    ReceiptError, ResourceName, ResourcePattern, RevocationBounds, RevocationSet, Spent,
    UsageLedger, VerifyContext, MAX_CHAIN_DEPTH, MAX_CLOCK_SKEW_SECS,
};
use spine_protocol::BrowserCommand;
use spine_storage::StorageBackend;

use crate::authority_ledger::{DurableChargeError, DurableLedger, LedgerOpenError};
use crate::config::AuthorityConfig;

/// First path segment of every node resource.
pub const NODE_ROOT: &str = "node";

/// The closed set of areas of a node's resource tree. A command belongs to
/// exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeArea {
    /// The caller's session: the loaded page and binary.
    Session,
    /// HTTP requests the node makes to the human web.
    Outbound,
    /// Running HLB programs.
    Binaries,
    /// The knowledge store.
    Knowledge,
    /// The name service: resolution, discovery and publication.
    Names,
    /// The content-addressed resource store.
    Resources,
    /// The node's own identity and connectivity.
    Status,
    /// Signed application calls to mesh peers.
    Mesh,
    /// The node's tools.
    Tools,
}

impl NodeArea {
    /// Every area.
    pub const ALL: [NodeArea; 9] = [
        NodeArea::Session,
        NodeArea::Outbound,
        NodeArea::Binaries,
        NodeArea::Knowledge,
        NodeArea::Names,
        NodeArea::Resources,
        NodeArea::Status,
        NodeArea::Mesh,
        NodeArea::Tools,
    ];

    /// The path segment of this area.
    pub const fn as_str(self) -> &'static str {
        match self {
            NodeArea::Session => "session",
            NodeArea::Outbound => "outbound",
            NodeArea::Binaries => "binaries",
            NodeArea::Knowledge => "knowledge",
            NodeArea::Names => "names",
            NodeArea::Resources => "resources",
            NodeArea::Status => "status",
            NodeArea::Mesh => "mesh",
            NodeArea::Tools => "tools",
        }
    }

    /// The resource path of this area: `["node", <area>]`.
    pub fn path(self) -> Vec<String> {
        vec![NODE_ROOT.to_string(), self.as_str().to_string()]
    }
}

/// What a caller must be granted to run a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// The action the invocation must name.
    pub action: Action,
    /// The effect class of the command. The invocation must declare this
    /// class or a higher one (`Read < Reversible < Irreversible`).
    pub effect: Effect,
    /// Path of the node resource, below the node's own key.
    pub node_path: Vec<String>,
}

impl Requirement {
    fn of(action: Action, effect: Effect, area: NodeArea) -> Requirement {
        Requirement {
            action,
            effect,
            node_path: area.path(),
        }
    }

    /// The resource an invocation must name, on the node with this key.
    pub fn resource(&self, node: PublicKey) -> ResourceName {
        ResourceName {
            owner: node,
            path: self.node_path.clone(),
        }
    }
}

/// The effect class of `store_knowledge`. A default build keeps the entry in
/// the node's memory; a `research` build also broadcasts it to the cluster,
/// and what has been sent cannot be recalled.
fn store_knowledge_effect() -> Effect {
    if cfg!(feature = "research") {
        Effect::Irreversible
    } else {
        Effect::Reversible
    }
}

/// Classifies a command: the action, effect class and node resource a caller
/// needs a grant for.
///
/// The rule for the effect class:
///
/// * `Read`: nothing leaves the node and nothing on it changes.
/// * `Reversible`: only the node's own state changes, in a way a later
///   command or an expiry undoes, or the node sends namespace lookups whose
///   form the caller does not control.
/// * `Irreversible`: something leaves the node that the caller shaped and the
///   node cannot recall (an HTTP request to a URL the caller chose, a call or
///   a publication signed with the node's key), or code runs whose effect the
///   node cannot bound.
///
/// Anything that makes the node send traffic, sign, execute or store is not
/// `Action::Read`. The table with one reason per command is in the spec.
///
/// The match has no wildcard arm: a command added to [`BrowserCommand`] does
/// not compile until it is classified here.
pub fn classify(command: &BrowserCommand) -> Requirement {
    let (action, effect, area) = match command {
        BrowserCommand::Navigate { .. } => {
            (Action::Invoke, Effect::Irreversible, NodeArea::Outbound)
        }
        BrowserCommand::GetUR => (Action::Read, Effect::Read, NodeArea::Session),
        BrowserCommand::GetRawHTML => (Action::Read, Effect::Read, NodeArea::Session),
        BrowserCommand::ExecuteBinary(_) => {
            (Action::Invoke, Effect::Irreversible, NodeArea::Binaries)
        }
        BrowserCommand::StoreKnowledge { .. } => (
            Action::Invoke,
            store_knowledge_effect(),
            NodeArea::Knowledge,
        ),
        BrowserCommand::QueryKnowledge { .. } => (Action::Read, Effect::Read, NodeArea::Knowledge),
        BrowserCommand::GetBinaryCapabilities => (Action::Read, Effect::Read, NodeArea::Session),
        BrowserCommand::ResolveName { .. } => (Action::Invoke, Effect::Reversible, NodeArea::Names),
        BrowserCommand::ResolveNames { .. } => {
            (Action::Invoke, Effect::Reversible, NodeArea::Names)
        }
        BrowserCommand::FindProviders { .. } => {
            (Action::Invoke, Effect::Reversible, NodeArea::Names)
        }
        BrowserCommand::PublishName { .. } => {
            (Action::Publish, Effect::Irreversible, NodeArea::Names)
        }
        BrowserCommand::FetchName { .. } => (Action::Invoke, Effect::Reversible, NodeArea::Names),
        BrowserCommand::CrawlNames { .. } => (Action::Invoke, Effect::Reversible, NodeArea::Names),
        BrowserCommand::PublishResource { .. } => {
            (Action::Publish, Effect::Irreversible, NodeArea::Resources)
        }
        BrowserCommand::ReadResource { .. } => (Action::Read, Effect::Read, NodeArea::Resources),
        BrowserCommand::NamespaceStatus => (Action::Read, Effect::Read, NodeArea::Status),
        BrowserCommand::MeshCall { .. } => (Action::Invoke, Effect::Irreversible, NodeArea::Mesh),
        BrowserCommand::ToolCall(_) => (Action::Invoke, Effect::Irreversible, NodeArea::Tools),
        BrowserCommand::CapabilityQuery(_) => (Action::Read, Effect::Read, NodeArea::Tools),
    };
    Requirement::of(action, effect, area)
}

/// The pattern that covers the whole resource tree of the node with this key:
/// `["node"]` and everything below it.
pub fn node_tree(node: PublicKey) -> ResourcePattern {
    ResourcePattern {
        owner: node,
        path: vec![NODE_ROOT.to_string()],
        prefix: true,
    }
}

/// The operation hash of a command: SHA-256 of the exact bytes of the command
/// body as they were on the wire. An invocation's `op` must equal it.
///
/// The node computes it from the bytes it received. Re-encoding the decoded
/// command and hashing that would also work for deterministic CBOR, but the
/// received bytes are what the invoker signed for.
pub fn operation_hash(command_body: &[u8]) -> [u8; 32] {
    Sha256::digest(command_body).into()
}

/// Verification policy of an [`AuthorityGate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateSettings {
    /// Clock skew tolerated, in seconds; reduced to
    /// [`MAX_CLOCK_SKEW_SECS`] if larger. Default 0.
    pub skew_secs: u64,
    /// Oldest an invocation may be, in seconds. Default 60.
    pub max_invocation_age_secs: u64,
    /// How old revocation data may be for each effect class. Default: 0 for
    /// every class, so the data must be current as of `now`.
    pub revocation_bounds: RevocationBounds,
    /// Bounds of the usage ledger.
    pub ledger: LedgerConfig,
}

impl Default for GateSettings {
    fn default() -> GateSettings {
        GateSettings {
            skew_secs: 0,
            max_invocation_age_secs: 60,
            revocation_bounds: RevocationBounds::default(),
            ledger: LedgerConfig::default(),
        }
    }
}

/// Why the gate refused, in full. For the node's own log; never sent to the
/// caller, who is told the step alone ([`Refusal::step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalCause {
    /// One of the eleven verification steps refused.
    Verification(Denial),
    /// The invocation names another action than the command's.
    ActionMismatch,
    /// The invocation declares a weaker effect class than the command's.
    EffectTooWeak,
    /// The invocation does not name exactly the command's node resource.
    ResourceMismatch,
    /// The invocation's `op` is not the hash of the command body.
    OperationMismatch,
    /// The ledger refused the charge.
    Charge(ChargeError),
    /// The revocation set could not be read (its lock is poisoned).
    RevocationsUnavailable,
    /// The usage ledger could not be used: its lock is poisoned, or a durable
    /// ledger could not write the charge to its backend.
    LedgerUnavailable,
}

impl RefusalCause {
    /// The verification step this cause is reported as, 1 to 11.
    ///
    /// The four command-binding causes are step 10: they are the part of "the
    /// operation falls within the last grant" that needs to know the
    /// operation. A charge refused for a replayed nonce is step 9; any other
    /// charge failure is step 11.
    pub fn step(&self) -> u8 {
        match self {
            RefusalCause::Verification(denial) => denial.step(),
            RefusalCause::ActionMismatch
            | RefusalCause::EffectTooWeak
            | RefusalCause::ResourceMismatch
            | RefusalCause::OperationMismatch => 10,
            RefusalCause::Charge(ChargeError::NonceReplayed) => 9,
            RefusalCause::Charge(_) => 11,
            RefusalCause::RevocationsUnavailable => 8,
            RefusalCause::LedgerUnavailable => 11,
        }
    }
}

impl std::fmt::Display for RefusalCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefusalCause::Verification(denial) => write!(f, "{denial}"),
            RefusalCause::ActionMismatch => {
                f.write_str("step 10: the invocation names another action than the command's")
            }
            RefusalCause::EffectTooWeak => f.write_str(
                "step 10: the invocation declares a weaker effect class than the command's",
            ),
            RefusalCause::ResourceMismatch => f.write_str(
                "step 10: the invocation does not name exactly the command's node resource",
            ),
            RefusalCause::OperationMismatch => {
                f.write_str("step 10: the invocation is for another operation")
            }
            RefusalCause::Charge(error) => write!(f, "step {}: {error}", self.step()),
            RefusalCause::RevocationsUnavailable => {
                f.write_str("step 8: the revocation set is unavailable")
            }
            RefusalCause::LedgerUnavailable => {
                f.write_str("step 11: the usage ledger is unavailable")
            }
        }
    }
}

/// Stable names of the eleven steps, for a wire error later.
const STEP_NAMES: [&str; 11] = [
    "authority_step_1",
    "authority_step_2",
    "authority_step_3",
    "authority_step_4",
    "authority_step_5",
    "authority_step_6",
    "authority_step_7",
    "authority_step_8",
    "authority_step_9",
    "authority_step_10",
    "authority_step_11",
];

/// A refusal by the gate.
///
/// What may be told to the caller is the step number ([`Refusal::step`]) or
/// its stable name ([`Refusal::name`]) and nothing else, as the spec
/// requires. `Display` prints only that. [`Refusal::cause`] and `Debug` carry
/// the reason and are for the node's own log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    cause: RefusalCause,
}

impl Refusal {
    /// The number of the step that refused, 1 to 11.
    pub fn step(&self) -> u8 {
        self.cause.step()
    }

    /// A short stable name for the step: `authority_step_1` to
    /// `authority_step_11`.
    pub fn name(&self) -> &'static str {
        let index = usize::from(self.step()).saturating_sub(1);
        STEP_NAMES
            .get(index)
            .copied()
            .unwrap_or("authority_refused")
    }

    /// The full reason. Do not send it to the caller.
    pub fn cause(&self) -> RefusalCause {
        self.cause
    }
}

impl From<RefusalCause> for Refusal {
    fn from(cause: RefusalCause) -> Refusal {
        Refusal { cause }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authority refused at step {}", self.step())
    }
}

impl std::error::Error for Refusal {}

/// The ledger a gate holds: in memory only, or mirrored in a backend. Private
/// so that the gate's checks and their order do not depend on which.
#[derive(Debug)]
enum GateLedger {
    Memory(MemoryLedger),
    Durable(Box<DurableLedger>),
}

impl GateLedger {
    fn charge(&mut self, decision: &Decision) -> Result<(), RefusalCause> {
        match self {
            GateLedger::Memory(ledger) => ledger.charge(decision).map_err(RefusalCause::Charge),
            GateLedger::Durable(ledger) => ledger.charge(decision).map_err(|error| match error {
                DurableChargeError::Charge(error) => RefusalCause::Charge(error),
                DurableChargeError::Storage => RefusalCause::LedgerUnavailable,
            }),
        }
    }
}

impl UsageLedger for GateLedger {
    fn nonce_seen(&self, invoker: &PublicKey, nonce: &[u8; 16]) -> bool {
        match self {
            GateLedger::Memory(ledger) => ledger.nonce_seen(invoker, nonce),
            GateLedger::Durable(ledger) => ledger.nonce_seen(invoker, nonce),
        }
    }

    fn spent(&self, id: &GrantId, now: u64) -> Option<Spent> {
        match self {
            GateLedger::Memory(ledger) => ledger.spent(id, now),
            GateLedger::Durable(ledger) => ledger.spent(id, now),
        }
    }
}

/// An invocation the gate accepted and charged. The command may now run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorized {
    /// The decoded invocation.
    pub invocation: Invocation,
    /// What verification decided and the ledger was charged.
    pub decision: Decision,
    /// What the command required.
    pub requirement: Requirement,
}

/// The node's enforcement point for layer 3.
///
/// One gate serves every connection of a node: the ledger and the revocation
/// set are shared, so limits and nonces hold across callers.
#[derive(Debug)]
pub struct AuthorityGate {
    recipient: PublicKey,
    trusted_roots: Vec<PublicKey>,
    settings: GateSettings,
    revocations: RwLock<RevocationSet>,
    ledger: Mutex<GateLedger>,
}

impl AuthorityGate {
    /// A gate for the node with key `recipient` that accepts chains rooted at
    /// `trusted_roots` (empty accepts none).
    ///
    /// The gate starts with no revocation data, so it refuses everything at
    /// step 8 until [`AuthorityGate::refresh_revocations`] is called.
    pub fn new(
        recipient: PublicKey,
        trusted_roots: Vec<PublicKey>,
        settings: GateSettings,
    ) -> AuthorityGate {
        let settings = GateSettings {
            skew_secs: settings.skew_secs.min(MAX_CLOCK_SKEW_SECS),
            ..settings
        };
        AuthorityGate::with_ledger(
            recipient,
            trusted_roots,
            settings,
            GateLedger::Memory(MemoryLedger::with_config(settings.ledger)),
        )
    }

    /// Like [`AuthorityGate::new`], with a usage ledger that survives a
    /// restart: charges are written to `backend` before they count, and the
    /// ledger found there is restored (`now` is the node's clock, Unix
    /// seconds). A charge whose write fails is refused at step 11. It refuses
    /// to open on a row that cannot be tied to a nonce or a grant
    /// ([`LedgerOpenError`]). See [`crate::authority_ledger`].
    pub fn new_durable(
        recipient: PublicKey,
        trusted_roots: Vec<PublicKey>,
        settings: GateSettings,
        backend: Arc<dyn StorageBackend>,
        now: u64,
    ) -> Result<AuthorityGate, LedgerOpenError> {
        let settings = GateSettings {
            skew_secs: settings.skew_secs.min(MAX_CLOCK_SKEW_SECS),
            ..settings
        };
        let ledger = DurableLedger::open(backend, settings.ledger, now)?;
        Ok(AuthorityGate::with_ledger(
            recipient,
            trusted_roots,
            settings,
            GateLedger::Durable(Box::new(ledger)),
        ))
    }

    fn with_ledger(
        recipient: PublicKey,
        trusted_roots: Vec<PublicKey>,
        settings: GateSettings,
        ledger: GateLedger,
    ) -> AuthorityGate {
        AuthorityGate {
            recipient,
            trusted_roots,
            settings,
            revocations: RwLock::new(RevocationSet::default()),
            ledger: Mutex::new(ledger),
        }
    }

    /// A gate for single-user mode: the only trusted root is the node itself.
    pub fn single_user(recipient: PublicKey, settings: GateSettings) -> AuthorityGate {
        AuthorityGate::new(recipient, vec![recipient], settings)
    }

    /// A gate with the policy of an `[authority]` section: the node's own key
    /// and the configured `trusted_roots` as roots, and the configured skew
    /// and invocation age. `mode` is not looked at; whether to consult the
    /// gate is the caller's decision.
    pub fn from_config(
        recipient: PublicKey,
        config: &AuthorityConfig,
    ) -> anyhow::Result<AuthorityGate> {
        let mut roots: Vec<PublicKey> = vec![recipient];
        for key in config.trusted_root_keys()? {
            if !roots.contains(&key) {
                roots.push(key);
            }
        }
        let settings = GateSettings {
            skew_secs: config.skew_secs,
            max_invocation_age_secs: config.max_invocation_age_secs,
            ..GateSettings::default()
        };
        Ok(AuthorityGate::new(recipient, roots, settings))
    }

    /// The node's key: the recipient every invocation must name.
    pub fn recipient(&self) -> PublicKey {
        self.recipient
    }

    /// The keys accepted as the root of a chain.
    pub fn trusted_roots(&self) -> &[PublicKey] {
        self.trusted_roots.as_slice()
    }

    /// The verification policy, with the skew already clamped.
    pub fn settings(&self) -> GateSettings {
        self.settings
    }

    /// Issues the root grant of single-user mode. Pure: no clock, no random
    /// source, no file.
    ///
    /// `signer` is the node's key. The grant gives `audience` the whole node
    /// tree ([`node_tree`]) with all four actions, effect ceiling
    /// `irreversible`, no limits, role `principal`, valid from `now` for
    /// `ttl_secs`, and delegable through as many links as a chain allows.
    /// `nonce` distinguishes it from another grant with the same fields; use
    /// 16 random bytes. A `ttl_secs` of 0 is refused (empty window).
    pub fn issue_self_root(
        signer: &SigningKey,
        audience: PublicKey,
        now: u64,
        ttl_secs: u64,
        nonce: [u8; 16],
    ) -> Result<Grant, IssueError> {
        let node: PublicKey = signer.verifying_key().to_bytes();
        let spec = GrantSpec {
            aud: audience,
            role: "principal".to_string(),
            res: vec![node_tree(node)],
            act: vec![Action::Read, Action::Invoke, Action::Publish, Action::Admin],
            effect: Effect::Irreversible,
            delegable: (MAX_CHAIN_DEPTH - 1) as u32,
            nbf: now,
            exp: now.saturating_add(ttl_secs),
            limits: Limits::default(),
            caveats: Vec::new(),
            nonce,
        };
        Grant::issue_root(signer, spec)
    }

    /// Marks a grant as revoked. Permanent for the life of the gate. The
    /// caller vouches for the id: no signed record is checked here.
    pub fn revoke(&self, id: GrantId) {
        let mut set = self
            .revocations
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        set.revoke(id);
    }

    /// States that the gate's revocation set is complete as of `as_of` (Unix
    /// seconds). The time only moves forward; an earlier value is ignored.
    pub fn refresh_revocations(&self, as_of: u64) {
        let mut set = self
            .revocations
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let newest = set.as_of.map_or(as_of, |current| current.max(as_of));
        set.as_of = Some(newest);
    }

    /// The time as of which the revocation set was last stated complete.
    /// `None`: never, so every invocation is refused at step 8.
    pub fn revocations_as_of(&self) -> Option<u64> {
        self.revocations
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .as_of
    }

    /// Decides whether `command` may run, and charges for it.
    ///
    /// In order:
    ///
    /// 1. The eleven verification steps ([`verify_encoded_with_invocation`]) against this
    ///    node's key, `session`, the trusted roots, the revocation set and the
    ///    ledger.
    /// 2. The invocation's action equals the command's ([`classify`]).
    /// 3. The invocation's declared effect class is the command's or higher.
    /// 4. The invocation names exactly one resource: the command's node
    ///    resource on this node.
    /// 5. The invocation's `op` equals `op_hash` ([`operation_hash`]).
    /// 6. The ledger records the nonce and charges every limited grant of the
    ///    chain.
    ///
    /// The ledger is locked from before 1 until after 6, so of two concurrent
    /// calls with one nonce, or two that would together pass a limit, only
    /// the first is accepted. A refusal at any point changes nothing: no
    /// nonce is recorded and nothing is charged.
    pub fn authorize(
        &self,
        chain_bytes: &[Vec<u8>],
        invocation_bytes: &[u8],
        command: &BrowserCommand,
        op_hash: [u8; 32],
        session: [u8; 32],
        now: u64,
    ) -> Result<Authorized, Refusal> {
        let requirement = classify(command);

        // Lock order: revocations, then ledger. `revoke` and
        // `refresh_revocations` take only the first.
        let revocations = self
            .revocations
            .read()
            .map_err(|_| Refusal::from(RefusalCause::RevocationsUnavailable))?;
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| Refusal::from(RefusalCause::LedgerUnavailable))?;

        let mut ctx =
            VerifyContext::new(now, self.recipient, session, self.trusted_roots.as_slice());
        ctx.skew_secs = self.settings.skew_secs;
        ctx.max_invocation_age_secs = self.settings.max_invocation_age_secs;
        ctx.revocation_bounds = self.settings.revocation_bounds;

        let (decision, invocation) = verify_encoded_with_invocation(
            chain_bytes,
            invocation_bytes,
            &ctx,
            &*revocations,
            &*ledger,
        )
        .map_err(|denial| Refusal::from(RefusalCause::Verification(denial)))?;

        if invocation.body.act != requirement.action {
            return Err(RefusalCause::ActionMismatch.into());
        }
        if invocation.body.effect < requirement.effect {
            return Err(RefusalCause::EffectTooWeak.into());
        }
        let expected = requirement.resource(self.recipient);
        if invocation.body.res.as_slice() != std::slice::from_ref(&expected) {
            return Err(RefusalCause::ResourceMismatch.into());
        }
        if invocation.body.op != op_hash {
            return Err(RefusalCause::OperationMismatch.into());
        }

        ledger.charge(&decision).map_err(Refusal::from)?;

        Ok(Authorized {
            invocation,
            decision,
            requirement,
        })
    }

    /// The signed receipt for an invocation this gate authorized, once the
    /// command has run. `signer` must be the node's key, `result_hash` is a
    /// digest of the result (opaque to layer 3) and `time` is when it ran.
    ///
    /// There is no receipt for a refusal here: the gate can refuse for a
    /// reason that is not a [`Denial`], and whether to sign refusals at all
    /// is undecided (P25).
    pub fn receipt(
        &self,
        signer: &SigningKey,
        authorized: &Authorized,
        result_hash: [u8; 32],
        time: u64,
    ) -> Result<Receipt, ReceiptError> {
        if authorized.invocation.body.recipient != self.recipient {
            return Err(ReceiptError::NotRecipient);
        }
        let verdict: Result<Decision, Denial> = Ok(authorized.decision.clone());
        Receipt::issue(signer, &authorized.invocation, &verdict, result_hash, time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_authority::{InvocationBody, INVOCATION_VERSION};
    use spine_protocol::{
        CapabilityQuery, CapabilitySelector, SpineBinary, ToolCall, COMMAND_NAMES,
    };
    use std::sync::Barrier;

    const NOW: u64 = 5_000;
    const SESSION: [u8; 32] = [0x55; 32];
    const NAME: &str = "spine://cap:web.search/";

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn public(k: &SigningKey) -> PublicKey {
        k.verifying_key().to_bytes()
    }

    fn all_actions() -> Vec<Action> {
        vec![Action::Read, Action::Invoke, Action::Publish, Action::Admin]
    }

    /// One of each command, in declaration order.
    fn every_command() -> Vec<BrowserCommand> {
        let null = serde_json::Value::Null;
        vec![
            BrowserCommand::Navigate {
                url: "https://example.org/".to_string(),
            },
            BrowserCommand::GetUR,
            BrowserCommand::GetRawHTML,
            BrowserCommand::ExecuteBinary(SpineBinary {
                instructions: Vec::new(),
                data: Vec::new(),
                render_start: 0,
                exported_functions: std::collections::BTreeMap::new(),
                capabilities: Vec::new(),
            }),
            BrowserCommand::StoreKnowledge {
                key: "k".to_string(),
                value: null.clone(),
                tags: Vec::new(),
            },
            BrowserCommand::QueryKnowledge {
                query: String::new(),
                tags: Vec::new(),
                limit: 1,
            },
            BrowserCommand::GetBinaryCapabilities,
            BrowserCommand::ResolveName {
                name: NAME.to_string(),
                if_none_match: None,
            },
            BrowserCommand::ResolveNames {
                names: vec![NAME.to_string()],
            },
            BrowserCommand::FindProviders {
                capability: "web.search".to_string(),
                limit: None,
            },
            BrowserCommand::PublishName {
                record: null.clone(),
            },
            BrowserCommand::FetchName {
                name: NAME.to_string(),
                if_none_match: None,
            },
            BrowserCommand::CrawlNames {
                seed: NAME.to_string(),
                max_depth: None,
                max_visits: None,
            },
            BrowserCommand::PublishResource {
                record: null.clone(),
                content_type: "text/plain".to_string(),
                data: Vec::new(),
            },
            BrowserCommand::ReadResource {
                hash: [0u8; 32],
                offset: 0,
                max_bytes: 1,
            },
            BrowserCommand::NamespaceStatus,
            BrowserCommand::MeshCall {
                peer_id: String::new(),
                peer_key: [0u8; 32],
                payload: null.clone(),
            },
            BrowserCommand::ToolCall(ToolCall {
                name: "spine.content/hash".to_string(),
                args: null,
                trace: None,
            }),
            BrowserCommand::CapabilityQuery(CapabilityQuery {
                selector: CapabilitySelector::All,
            }),
        ]
    }

    /// The wire name of a command. Exhaustive, like `classify`.
    fn wire_name(command: &BrowserCommand) -> &'static str {
        match command {
            BrowserCommand::Navigate { .. } => "navigate",
            BrowserCommand::GetUR => "get_ur",
            BrowserCommand::GetRawHTML => "get_raw_html",
            BrowserCommand::ExecuteBinary(_) => "execute_binary",
            BrowserCommand::StoreKnowledge { .. } => "store_knowledge",
            BrowserCommand::QueryKnowledge { .. } => "query_knowledge",
            BrowserCommand::GetBinaryCapabilities => "get_binary_capabilities",
            BrowserCommand::ResolveName { .. } => "resolve_name",
            BrowserCommand::ResolveNames { .. } => "resolve_names",
            BrowserCommand::FindProviders { .. } => "find_providers",
            BrowserCommand::PublishName { .. } => "publish_name",
            BrowserCommand::FetchName { .. } => "fetch_name",
            BrowserCommand::CrawlNames { .. } => "crawl_names",
            BrowserCommand::PublishResource { .. } => "publish_resource",
            BrowserCommand::ReadResource { .. } => "read_resource",
            BrowserCommand::NamespaceStatus => "namespace_status",
            BrowserCommand::MeshCall { .. } => "mesh_call",
            BrowserCommand::ToolCall(_) => "tool_call",
            BrowserCommand::CapabilityQuery(_) => "capability_query",
        }
    }

    /// Stand-in for the hash of the command body: the carrier will hash the
    /// bytes it received; here any value both sides agree on will do.
    fn op_of(command: &BrowserCommand) -> [u8; 32] {
        operation_hash(wire_name(command).as_bytes())
    }

    /// The node (key 9) in single-user mode. It issued itself a root grant
    /// for an operator (key 2), who delegated the whole tree to an agent
    /// (key 3).
    struct World {
        node: SigningKey,
        operator: SigningKey,
        agent: SigningKey,
        gate: AuthorityGate,
        root: Grant,
        leaf: Grant,
    }

    impl World {
        fn new() -> World {
            World::with_leaf_limits(Limits::default())
        }

        fn with_leaf_limits(limits: Limits) -> World {
            let node = key(9);
            let operator = key(2);
            let agent = key(3);
            let gate = AuthorityGate::single_user(public(&node), GateSettings::default());
            gate.refresh_revocations(NOW);
            let root = AuthorityGate::issue_self_root(
                &node,
                public(&operator),
                NOW - 100,
                3_600,
                [1u8; 16],
            )
            .unwrap();
            let leaf = Grant::delegate(
                &operator,
                &root,
                GrantSpec {
                    aud: public(&agent),
                    role: "agent".to_string(),
                    res: vec![node_tree(public(&node))],
                    act: all_actions(),
                    effect: Effect::Irreversible,
                    delegable: 0,
                    nbf: NOW - 100,
                    exp: NOW + 1_000,
                    limits,
                    caveats: Vec::new(),
                    nonce: [2u8; 16],
                },
            )
            .unwrap();
            World {
                node,
                operator,
                agent,
                gate,
                root,
                leaf,
            }
        }

        fn chain(&self) -> Vec<Vec<u8>> {
            vec![self.root.encode().unwrap(), self.leaf.encode().unwrap()]
        }

        /// The invocation body that exactly covers `command`.
        fn body(&self, command: &BrowserCommand, nonce: u8) -> InvocationBody {
            let requirement = classify(command);
            InvocationBody {
                version: INVOCATION_VERSION,
                invoker: public(&self.agent),
                recipient: public(&self.node),
                session: SESSION,
                nonce: [nonce; 16],
                time: NOW,
                res: vec![requirement.resource(public(&self.node))],
                act: requirement.action,
                effect: requirement.effect,
                op: op_of(command),
                leaf: self.leaf.id().unwrap(),
                cost: 0,
                bytes: 0,
            }
        }

        fn sign(&self, body: InvocationBody) -> Vec<u8> {
            Invocation::sign(&self.agent, body)
                .unwrap()
                .encode()
                .unwrap()
        }

        fn attempt_on(
            &self,
            gate: &AuthorityGate,
            command: &BrowserCommand,
            body: InvocationBody,
        ) -> Result<Authorized, Refusal> {
            let chain = self.chain();
            let invocation = self.sign(body);
            gate.authorize(
                chain.as_slice(),
                invocation.as_slice(),
                command,
                op_of(command),
                SESSION,
                NOW,
            )
        }

        fn attempt(
            &self,
            command: &BrowserCommand,
            body: InvocationBody,
        ) -> Result<Authorized, Refusal> {
            self.attempt_on(&self.gate, command, body)
        }
    }

    fn navigate() -> BrowserCommand {
        BrowserCommand::Navigate {
            url: "https://example.org/".to_string(),
        }
    }

    fn resolve() -> BrowserCommand {
        BrowserCommand::ResolveName {
            name: NAME.to_string(),
            if_none_match: None,
        }
    }

    #[test]
    fn every_command_is_classified_and_no_mutating_command_is_a_read() {
        let commands = every_command();
        let names: Vec<&str> = commands.iter().map(wire_name).collect();
        assert_eq!(names, COMMAND_NAMES.to_vec());

        // The commands that send nothing, sign nothing, run nothing and store
        // nothing. Every other command must not be classified as a read.
        let read_only = [
            "get_ur",
            "get_raw_html",
            "query_knowledge",
            "get_binary_capabilities",
            "read_resource",
            "namespace_status",
            "capability_query",
        ];
        for command in &commands {
            let name = wire_name(command);
            let requirement = classify(command);
            assert_eq!(requirement.node_path.len(), 2, "{name}");
            assert_eq!(requirement.node_path[0], NODE_ROOT, "{name}");
            assert!(
                NodeArea::ALL
                    .iter()
                    .any(|area| area.as_str() == requirement.node_path[1]),
                "{name}"
            );
            assert!(requirement.resource([7u8; 32]).validate().is_ok(), "{name}");
            assert!(node_tree([7u8; 32]).contains(&requirement.resource([7u8; 32])));
            if read_only.contains(&name) {
                assert_eq!(requirement.action, Action::Read, "{name}");
                assert_eq!(requirement.effect, Effect::Read, "{name}");
            } else {
                assert_ne!(requirement.action, Action::Read, "{name}");
                assert!(requirement.effect > Effect::Read, "{name}");
            }
        }
    }

    #[test]
    fn commands_that_reach_outside_the_node_are_irreversible_invocations_or_publications() {
        for command in every_command() {
            let name = wire_name(&command);
            let requirement = classify(&command);
            match name {
                "navigate" | "execute_binary" | "mesh_call" | "tool_call" => {
                    assert_eq!(requirement.action, Action::Invoke, "{name}");
                    assert_eq!(requirement.effect, Effect::Irreversible, "{name}");
                }
                "publish_name" | "publish_resource" => {
                    assert_eq!(requirement.action, Action::Publish, "{name}");
                    assert_eq!(requirement.effect, Effect::Irreversible, "{name}");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn area_names_are_distinct_valid_segments() {
        for (index, area) in NodeArea::ALL.iter().enumerate() {
            assert!(!area.as_str().is_empty());
            assert!(NodeArea::ALL[index + 1..]
                .iter()
                .all(|other| other.as_str() != area.as_str()));
            assert_eq!(
                area.path(),
                vec![NODE_ROOT.to_string(), area.as_str().to_string()]
            );
        }
    }

    #[test]
    fn a_matching_invocation_is_authorized_for_every_command() {
        let world = World::new();
        for (index, command) in every_command().iter().enumerate() {
            let nonce = u8::try_from(index).unwrap() + 1;
            let authorized = world
                .attempt(command, world.body(command, nonce))
                .unwrap_or_else(|refusal| panic!("{}: {refusal:?}", wire_name(command)));
            assert_eq!(authorized.requirement, classify(command));
            assert_eq!(authorized.decision.chain_ids.len(), 2);
            assert_eq!(authorized.invocation.body.op, op_of(command));
        }
    }

    #[test]
    fn a_self_issued_root_authorizes_its_audience_directly() {
        let world = World::new();
        let command = BrowserCommand::NamespaceStatus;
        let root =
            AuthorityGate::issue_self_root(&world.node, public(&world.agent), NOW, 60, [3u8; 16])
                .unwrap();
        assert_eq!(root.body.iss, public(&world.node));
        assert_eq!(root.body.parent, None);
        assert_eq!(root.body.act, all_actions());
        assert_eq!(root.body.res, vec![node_tree(public(&world.node))]);
        assert_eq!((root.body.nbf, root.body.exp), (NOW, NOW + 60));

        let mut body = world.body(&command, 1);
        body.leaf = root.id().unwrap();
        let invocation = world.sign(body);
        let chain = [root.encode().unwrap()];
        let authorized = world
            .gate
            .authorize(
                &chain,
                invocation.as_slice(),
                &command,
                op_of(&command),
                SESSION,
                NOW,
            )
            .unwrap();
        assert_eq!(authorized.decision.chain_ids, vec![root.id().unwrap()]);
    }

    #[test]
    fn a_self_issued_root_needs_a_window() {
        let node = key(9);
        let refused = AuthorityGate::issue_self_root(&node, [4u8; 32], NOW, 0, [0u8; 16]);
        assert!(refused.is_err());
        let end_of_time =
            AuthorityGate::issue_self_root(&node, [4u8; 32], NOW, u64::MAX, [0u8; 16]).unwrap();
        assert_eq!(end_of_time.body.exp, u64::MAX);
    }

    #[test]
    fn a_delegate_cannot_be_given_more_than_the_node_tree() {
        let world = World::new();
        let outside = ResourcePattern {
            owner: public(&world.node),
            path: vec!["elsewhere".to_string()],
            prefix: true,
        };
        let refused = Grant::delegate(
            &world.operator,
            &world.root,
            GrantSpec {
                aud: public(&world.agent),
                role: "agent".to_string(),
                res: vec![outside],
                act: vec![Action::Read],
                effect: Effect::Read,
                delegable: 0,
                nbf: NOW,
                exp: NOW + 10,
                limits: Limits::default(),
                caveats: Vec::new(),
                nonce: [5u8; 16],
            },
        );
        assert!(refused.is_err());
    }

    #[test]
    fn a_stronger_declared_effect_is_accepted() {
        let world = World::new();
        let command = resolve();
        assert_eq!(classify(&command).effect, Effect::Reversible);
        let mut body = world.body(&command, 1);
        body.effect = Effect::Irreversible;
        let authorized = world.attempt(&command, body).unwrap();
        assert_eq!(authorized.decision.effect, Effect::Irreversible);
    }

    #[test]
    fn a_wrong_action_is_refused_at_step_10() {
        let world = World::new();
        let command = navigate();
        let mut body = world.body(&command, 1);
        body.act = Action::Read;
        let refusal = world.attempt(&command, body).unwrap_err();
        assert_eq!(refusal.cause(), RefusalCause::ActionMismatch);
        assert_eq!(refusal.step(), 10);
    }

    #[test]
    fn a_weaker_declared_effect_is_refused_at_step_10() {
        let world = World::new();
        let command = navigate();
        for weaker in [Effect::Read, Effect::Reversible] {
            let mut body = world.body(&command, 1);
            body.effect = weaker;
            let refusal = world.attempt(&command, body).unwrap_err();
            assert_eq!(refusal.cause(), RefusalCause::EffectTooWeak);
            assert_eq!(refusal.step(), 10);
        }
    }

    #[test]
    fn a_wrong_resource_is_refused_at_step_10() {
        let world = World::new();
        let command = navigate();
        let node = public(&world.node);
        let right = classify(&command).resource(node);
        let other = classify(&resolve()).resource(node);
        let whole_tree = ResourceName {
            owner: node,
            path: vec![NODE_ROOT.to_string()],
        };
        let below = ResourceName {
            owner: node,
            path: vec![
                NODE_ROOT.to_string(),
                "outbound".to_string(),
                "x".to_string(),
            ],
        };
        for res in [
            vec![other.clone()],
            vec![whole_tree],
            vec![below],
            vec![right.clone(), other],
            vec![right.clone(), right],
        ] {
            let mut body = world.body(&command, 1);
            body.res = res;
            let refusal = world.attempt(&command, body).unwrap_err();
            assert_eq!(refusal.cause(), RefusalCause::ResourceMismatch);
            assert_eq!(refusal.step(), 10);
        }
    }

    #[test]
    fn a_wrong_operation_hash_is_refused_at_step_10_and_charges_nothing() {
        let world = World::new();
        let command = navigate();
        let chain = world.chain();
        let invocation = world.sign(world.body(&command, 1));

        // The invocation was signed for `navigate`; the node is running
        // another body.
        let refusal = world
            .gate
            .authorize(
                chain.as_slice(),
                invocation.as_slice(),
                &command,
                operation_hash(b"another body"),
                SESSION,
                NOW,
            )
            .unwrap_err();
        assert_eq!(refusal.cause(), RefusalCause::OperationMismatch);
        assert_eq!(refusal.step(), 10);

        // The refusal did not spend the nonce: the same invocation is
        // accepted for the command it was signed for.
        assert!(world
            .gate
            .authorize(
                chain.as_slice(),
                invocation.as_slice(),
                &command,
                op_of(&command),
                SESSION,
                NOW,
            )
            .is_ok());
    }

    #[test]
    fn an_untrusted_root_is_refused_at_step_3() {
        let world = World::new();
        let command = BrowserCommand::NamespaceStatus;

        // A gate that trusts no root refuses even the node's own.
        let trusts_none = AuthorityGate::new(public(&world.node), Vec::new(), Default::default());
        trusts_none.refresh_revocations(NOW);
        let refusal = world
            .attempt_on(&trusts_none, &command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::RootNotAuthoritative)
        );
        assert_eq!(refusal.step(), 3);

        // A stranger roots a chain over its own tree.
        let stranger = key(7);
        let root = Grant::issue_root(
            &stranger,
            GrantSpec {
                aud: public(&world.agent),
                role: "principal".to_string(),
                res: vec![node_tree(public(&stranger))],
                act: all_actions(),
                effect: Effect::Irreversible,
                delegable: 0,
                nbf: NOW - 100,
                exp: NOW + 1_000,
                limits: Limits::default(),
                caveats: Vec::new(),
                nonce: [6u8; 16],
            },
        )
        .unwrap();
        let chain = [root.encode().unwrap()];
        let strangers_resource = classify(&command).resource(public(&stranger));
        let attempt = |gate: &AuthorityGate, res: Vec<ResourceName>, nonce: u8| {
            let mut body = world.body(&command, nonce);
            body.leaf = root.id().unwrap();
            body.res = res;
            let invocation = world.sign(body);
            gate.authorize(
                &chain,
                invocation.as_slice(),
                &command,
                op_of(&command),
                SESSION,
                NOW,
            )
        };

        // Not trusted: step 3.
        let refusal = attempt(&world.gate, vec![strangers_resource.clone()], 2).unwrap_err();
        assert_eq!(refusal.step(), 3);

        // Trusted as a root, it still cannot reach the node's tree (step 3:
        // it does not own it), and its own tree is not what the command
        // needs (step 10).
        let trusts_stranger = AuthorityGate::new(
            public(&world.node),
            vec![public(&world.node), public(&stranger)],
            Default::default(),
        );
        trusts_stranger.refresh_revocations(NOW);
        let nodes_resource = classify(&command).resource(public(&world.node));
        let refusal = attempt(&trusts_stranger, vec![nodes_resource], 3).unwrap_err();
        assert_eq!(refusal.step(), 3);
        let refusal = attempt(&trusts_stranger, vec![strangers_resource], 4).unwrap_err();
        assert_eq!(refusal.cause(), RefusalCause::ResourceMismatch);
    }

    #[test]
    fn a_revoked_grant_is_refused_at_step_8() {
        let command = BrowserCommand::NamespaceStatus;

        let world = World::new();
        assert!(world.attempt(&command, world.body(&command, 1)).is_ok());
        world.gate.revoke(world.leaf.id().unwrap());
        let refusal = world
            .attempt(&command, world.body(&command, 2))
            .unwrap_err();
        assert_eq!(refusal.cause(), RefusalCause::Verification(Denial::Revoked));
        assert_eq!(refusal.step(), 8);

        // Revoking the root revokes what was delegated from it.
        let world = World::new();
        world.gate.revoke(world.root.id().unwrap());
        let refusal = world
            .attempt(&command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(refusal.step(), 8);
    }

    #[test]
    fn missing_or_stale_revocation_data_is_refused_at_step_8() {
        let world = World::new();
        let command = BrowserCommand::NamespaceStatus;

        let fresh_gate = AuthorityGate::single_user(public(&world.node), Default::default());
        assert_eq!(fresh_gate.revocations_as_of(), None);
        let refusal = world
            .attempt_on(&fresh_gate, &command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::RevocationDataStale)
        );
        assert_eq!(refusal.step(), 8);

        // One second old is too old under the default bound of 0.
        fresh_gate.refresh_revocations(NOW - 1);
        let refusal = world
            .attempt_on(&fresh_gate, &command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(refusal.step(), 8);

        fresh_gate.refresh_revocations(NOW);
        assert!(world
            .attempt_on(&fresh_gate, &command, world.body(&command, 1))
            .is_ok());

        // The time never moves back.
        fresh_gate.refresh_revocations(NOW - 50);
        assert_eq!(fresh_gate.revocations_as_of(), Some(NOW));
    }

    #[test]
    fn a_replayed_nonce_is_refused_at_step_9() {
        let world = World::new();
        let command = resolve();
        assert!(world.attempt(&command, world.body(&command, 1)).is_ok());
        let refusal = world
            .attempt(&command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::NonceReplayed)
        );
        assert_eq!(refusal.step(), 9);
        assert!(world.attempt(&command, world.body(&command, 2)).is_ok());
    }

    #[test]
    fn another_session_is_refused_at_step_9() {
        let world = World::new();
        let command = resolve();
        let chain = world.chain();
        let invocation = world.sign(world.body(&command, 1));
        let refusal = world
            .gate
            .authorize(
                chain.as_slice(),
                invocation.as_slice(),
                &command,
                op_of(&command),
                [0x66; 32],
                NOW,
            )
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::WrongSession)
        );
        assert_eq!(refusal.step(), 9);
    }

    #[test]
    fn an_exhausted_call_limit_is_refused_at_step_11() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(2),
            ..Limits::default()
        });
        let command = resolve();
        assert!(world.attempt(&command, world.body(&command, 1)).is_ok());
        assert!(world.attempt(&command, world.body(&command, 2)).is_ok());
        let refusal = world
            .attempt(&command, world.body(&command, 3))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::LimitExhausted)
        );
        assert_eq!(refusal.step(), 11);
    }

    #[test]
    fn a_refused_invocation_does_not_count_against_a_limit() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(1),
            ..Limits::default()
        });
        let command = navigate();
        let mut weak = world.body(&command, 1);
        weak.effect = Effect::Read;
        assert!(world.attempt(&command, weak).is_err());
        assert!(world.attempt(&command, world.body(&command, 2)).is_ok());
        assert!(world.attempt(&command, world.body(&command, 3)).is_err());
    }

    #[test]
    fn of_threads_racing_one_invocation_exactly_one_wins() {
        let world = World::new();
        let command = BrowserCommand::NamespaceStatus;
        let chain = world.chain();
        let op = op_of(&command);
        let gate = &world.gate;
        const THREADS: usize = 8;

        for round in 1..=4u8 {
            let invocation = world.sign(world.body(&command, round));
            let barrier = Barrier::new(THREADS);
            let results: Vec<Result<Authorized, Refusal>> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..THREADS)
                    .map(|_| {
                        let barrier = &barrier;
                        let chain = &chain;
                        let invocation = &invocation;
                        let command = &command;
                        scope.spawn(move || {
                            barrier.wait();
                            gate.authorize(
                                chain.as_slice(),
                                invocation.as_slice(),
                                command,
                                op,
                                SESSION,
                                NOW,
                            )
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect()
            });
            let winners = results.iter().filter(|result| result.is_ok()).count();
            assert_eq!(winners, 1, "round {round}");
            for refusal in results.iter().filter_map(|result| result.as_ref().err()) {
                assert_eq!(refusal.step(), 9);
            }
        }
    }

    #[test]
    fn of_threads_racing_a_call_limit_only_the_limit_passes() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(3),
            ..Limits::default()
        });
        let command = BrowserCommand::NamespaceStatus;
        let chain = world.chain();
        let op = op_of(&command);
        let gate = &world.gate;
        const THREADS: u8 = 8;
        let invocations: Vec<Vec<u8>> = (1..=THREADS)
            .map(|nonce| world.sign(world.body(&command, nonce)))
            .collect();
        let barrier = Barrier::new(usize::from(THREADS));
        let results: Vec<Result<Authorized, Refusal>> = std::thread::scope(|scope| {
            let handles: Vec<_> = invocations
                .iter()
                .map(|invocation| {
                    let barrier = &barrier;
                    let chain = &chain;
                    let command = &command;
                    scope.spawn(move || {
                        barrier.wait();
                        gate.authorize(
                            chain.as_slice(),
                            invocation.as_slice(),
                            command,
                            op,
                            SESSION,
                            NOW,
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let winners = results.iter().filter(|result| result.is_ok()).count();
        assert_eq!(winners, 3);
        for refusal in results.iter().filter_map(|result| result.as_ref().err()) {
            assert_eq!(refusal.step(), 11);
        }
    }

    #[test]
    fn unreadable_input_is_refused_at_the_step_that_reads_it() {
        let world = World::new();
        let command = BrowserCommand::NamespaceStatus;
        let chain = world.chain();
        let invocation = world.sign(world.body(&command, 1));
        let authorize = |chain: &[Vec<u8>], invocation: &[u8]| {
            world
                .gate
                .authorize(chain, invocation, &command, op_of(&command), SESSION, NOW)
        };
        assert_eq!(authorize(&[], invocation.as_slice()).unwrap_err().step(), 1);
        assert_eq!(
            authorize(&[vec![0xffu8; 8]], invocation.as_slice())
                .unwrap_err()
                .step(),
            2
        );
        assert_eq!(
            authorize(chain.as_slice(), &[0xffu8; 8])
                .unwrap_err()
                .step(),
            9
        );
        // The chain without its root: the leaf is not a root grant.
        assert!(authorize(&chain[1..], invocation.as_slice()).is_err());
    }

    #[test]
    fn a_refusal_shows_the_caller_the_step_and_nothing_else() {
        let world = World::new();
        let command = navigate();
        let mut body = world.body(&command, 1);
        body.act = Action::Read;
        let refusal = world.attempt(&command, body).unwrap_err();
        assert_eq!(refusal.to_string(), "authority refused at step 10");
        assert_eq!(refusal.name(), "authority_step_10");
        // The log line says more.
        assert!(refusal.cause().to_string().contains("another action"));

        for (index, name) in STEP_NAMES.iter().enumerate() {
            assert_eq!(*name, format!("authority_step_{}", index + 1));
        }
        let causes = [
            (RefusalCause::Verification(Denial::ChainLimits), 1),
            (RefusalCause::Verification(Denial::LimitExhausted), 11),
            (RefusalCause::OperationMismatch, 10),
            (RefusalCause::Charge(ChargeError::NonceReplayed), 9),
            (RefusalCause::Charge(ChargeError::LimitExhausted), 11),
            (RefusalCause::Charge(ChargeError::LedgerFull), 11),
            (RefusalCause::RevocationsUnavailable, 8),
            (RefusalCause::LedgerUnavailable, 11),
        ];
        for (cause, step) in causes {
            let refusal = Refusal::from(cause);
            assert_eq!(refusal.step(), step);
            assert_eq!(refusal.name(), format!("authority_step_{step}"));
            assert_eq!(
                refusal.to_string(),
                format!("authority refused at step {step}")
            );
        }
    }

    #[test]
    fn a_full_ledger_refuses_instead_of_forgetting() {
        let world = World::new();
        let command = BrowserCommand::NamespaceStatus;
        let small = AuthorityGate::single_user(
            public(&world.node),
            GateSettings {
                ledger: LedgerConfig {
                    max_nonces: 1,
                    max_grants: 16,
                },
                ..GateSettings::default()
            },
        );
        small.refresh_revocations(NOW);
        assert!(world
            .attempt_on(&small, &command, world.body(&command, 1))
            .is_ok());
        let refusal = world
            .attempt_on(&small, &command, world.body(&command, 2))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Charge(ChargeError::LedgerFull)
        );
        assert_eq!(refusal.step(), 11);
    }

    #[test]
    fn an_authorized_invocation_gets_a_receipt_signed_by_the_node() {
        let world = World::new();
        let command = navigate();
        let authorized = world.attempt(&command, world.body(&command, 1)).unwrap();
        let receipt = world
            .gate
            .receipt(&world.node, &authorized, [0xee; 32], NOW)
            .unwrap();
        assert_eq!(receipt.verify(&public(&world.node)), Ok(()));
        let verdict: Result<Decision, Denial> = Ok(authorized.decision.clone());
        assert_eq!(receipt.matches(&authorized.invocation, &verdict), Ok(()));
        assert_eq!(receipt.body.effect, Some(Effect::Irreversible));
        assert_eq!(receipt.body.leaf, world.leaf.id().unwrap());

        // Only the node's key can sign it.
        assert_eq!(
            world
                .gate
                .receipt(&world.agent, &authorized, [0xee; 32], NOW),
            Err(ReceiptError::NotRecipient)
        );
    }

    #[test]
    fn a_gate_from_configuration_trusts_the_node_and_the_listed_roots() {
        let node = public(&key(9));
        let listed = "ab".repeat(32);
        let own: String = node.iter().map(|byte| format!("{byte:02x}")).collect();
        let config = AuthorityConfig {
            trusted_roots: vec![listed, own],
            skew_secs: 30,
            max_invocation_age_secs: 120,
            ..AuthorityConfig::default()
        };
        let gate = AuthorityGate::from_config(node, &config).unwrap();
        assert_eq!(gate.recipient(), node);
        assert_eq!(gate.trusted_roots().to_vec(), vec![node, [0xabu8; 32]]);
        assert_eq!(gate.settings().skew_secs, 30);
        assert_eq!(gate.settings().max_invocation_age_secs, 120);
        assert_eq!(
            gate.settings().revocation_bounds,
            RevocationBounds::default()
        );

        let bad = AuthorityConfig {
            trusted_roots: vec!["not a key".to_string()],
            ..AuthorityConfig::default()
        };
        assert!(AuthorityGate::from_config(node, &bad).is_err());

        let clamped = AuthorityGate::single_user(
            node,
            GateSettings {
                skew_secs: MAX_CLOCK_SKEW_SECS + 1,
                ..GateSettings::default()
            },
        );
        assert_eq!(clamped.settings().skew_secs, MAX_CLOCK_SKEW_SECS);
    }

    // The same gate over a ledger that is kept in a backend.

    use crate::authority_ledger::FlakyBackend;

    fn durable_gate_with(
        world: &World,
        backend: &Arc<FlakyBackend>,
        settings: GateSettings,
        now: u64,
    ) -> AuthorityGate {
        let as_dyn: Arc<dyn StorageBackend> = backend.clone();
        let gate = AuthorityGate::new_durable(
            public(&world.node),
            vec![public(&world.node)],
            settings,
            as_dyn,
            now,
        )
        .unwrap();
        gate.refresh_revocations(NOW);
        gate
    }

    fn durable_gate(world: &World, backend: &Arc<FlakyBackend>, now: u64) -> AuthorityGate {
        durable_gate_with(world, backend, GateSettings::default(), now)
    }

    #[test]
    fn a_durable_gate_authorizes_and_refuses_replays_like_the_memory_gate() {
        let world = World::new();
        let backend = Arc::new(FlakyBackend::new());
        let gate = durable_gate(&world, &backend, NOW);
        let command = resolve();
        assert!(world
            .attempt_on(&gate, &command, world.body(&command, 1))
            .is_ok());
        let refusal = world
            .attempt_on(&gate, &command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::NonceReplayed)
        );
        assert_eq!(refusal.step(), 9);
        assert!(world
            .attempt_on(&gate, &command, world.body(&command, 2))
            .is_ok());
    }

    #[test]
    fn a_durable_gate_enforces_a_call_limit_and_a_refusal_charges_nothing() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(2),
            ..Limits::default()
        });
        let backend = Arc::new(FlakyBackend::new());
        let gate = durable_gate(&world, &backend, NOW);
        let command = navigate();
        let mut weak = world.body(&command, 1);
        weak.effect = Effect::Read;
        assert!(world.attempt_on(&gate, &command, weak).is_err());
        assert!(world
            .attempt_on(&gate, &command, world.body(&command, 2))
            .is_ok());
        assert!(world
            .attempt_on(&gate, &command, world.body(&command, 3))
            .is_ok());
        let refusal = world
            .attempt_on(&gate, &command, world.body(&command, 4))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::LimitExhausted)
        );
        assert_eq!(refusal.step(), 11);
    }

    #[test]
    fn a_restarted_gate_remembers_spent_limits_and_nonces() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(2),
            ..Limits::default()
        });
        let backend = Arc::new(FlakyBackend::new());
        let command = resolve();
        {
            let gate = durable_gate(&world, &backend, NOW);
            assert!(world
                .attempt_on(&gate, &command, world.body(&command, 1))
                .is_ok());
            assert!(world
                .attempt_on(&gate, &command, world.body(&command, 2))
                .is_ok());
        }
        let restarted = durable_gate(&world, &backend, NOW + 1);
        let exhausted = world
            .attempt_on(&restarted, &command, world.body(&command, 3))
            .unwrap_err();
        assert_eq!(
            exhausted.cause(),
            RefusalCause::Verification(Denial::LimitExhausted)
        );
        let replay = world
            .attempt_on(&restarted, &command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(
            replay.cause(),
            RefusalCause::Verification(Denial::NonceReplayed)
        );

        // A memory gate forgets both, which is why the durable one exists.
        let forgetful = AuthorityGate::single_user(public(&world.node), GateSettings::default());
        forgetful.refresh_revocations(NOW);
        assert!(world
            .attempt_on(&forgetful, &command, world.body(&command, 1))
            .is_ok());
    }

    #[test]
    fn a_durable_gate_that_cannot_write_refuses_at_step_11_and_charges_nothing() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(1),
            ..Limits::default()
        });
        let backend = Arc::new(FlakyBackend::new());
        let gate = durable_gate(&world, &backend, NOW);
        let command = resolve();
        backend.allow_puts(0);
        let refusal = world
            .attempt_on(&gate, &command, world.body(&command, 1))
            .unwrap_err();
        assert_eq!(refusal.cause(), RefusalCause::LedgerUnavailable);
        assert_eq!(refusal.step(), 11);
        assert_eq!(refusal.to_string(), "authority refused at step 11");

        // Neither the nonce nor the single call was used up.
        backend.allow_puts(usize::MAX);
        assert!(world
            .attempt_on(&gate, &command, world.body(&command, 1))
            .is_ok());
        assert_eq!(
            world
                .attempt_on(&gate, &command, world.body(&command, 2))
                .unwrap_err()
                .step(),
            11
        );
    }

    #[test]
    fn a_full_durable_ledger_refuses_instead_of_forgetting() {
        let world = World::new();
        let backend = Arc::new(FlakyBackend::new());
        let gate = durable_gate_with(
            &world,
            &backend,
            GateSettings {
                ledger: LedgerConfig {
                    max_nonces: 1,
                    max_grants: 16,
                },
                ..GateSettings::default()
            },
            NOW,
        );
        let command = BrowserCommand::NamespaceStatus;
        assert!(world
            .attempt_on(&gate, &command, world.body(&command, 1))
            .is_ok());
        let refusal = world
            .attempt_on(&gate, &command, world.body(&command, 2))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Charge(ChargeError::LedgerFull)
        );
    }

    #[test]
    fn of_threads_racing_a_call_limit_a_durable_gate_lets_only_the_limit_pass() {
        let world = World::with_leaf_limits(Limits {
            calls: Some(3),
            ..Limits::default()
        });
        let backend = Arc::new(FlakyBackend::new());
        let gate = durable_gate(&world, &backend, NOW);
        let command = BrowserCommand::NamespaceStatus;
        let chain = world.chain();
        let op = op_of(&command);
        const THREADS: u8 = 8;
        let invocations: Vec<Vec<u8>> = (1..=THREADS)
            .map(|nonce| world.sign(world.body(&command, nonce)))
            .collect();
        let barrier = Barrier::new(usize::from(THREADS));
        let results: Vec<Result<Authorized, Refusal>> = std::thread::scope(|scope| {
            let handles: Vec<_> = invocations
                .iter()
                .map(|invocation| {
                    let barrier = &barrier;
                    let chain = &chain;
                    let command = &command;
                    let gate = &gate;
                    scope.spawn(move || {
                        barrier.wait();
                        gate.authorize(
                            chain.as_slice(),
                            invocation.as_slice(),
                            command,
                            op,
                            SESSION,
                            NOW,
                        )
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 3);

        // What the winners spent is in the backend.
        drop(gate);
        let restarted = durable_gate(&world, &backend, NOW);
        let refusal = world
            .attempt_on(&restarted, &command, world.body(&command, 100))
            .unwrap_err();
        assert_eq!(
            refusal.cause(),
            RefusalCause::Verification(Denial::LimitExhausted)
        );
    }

    #[test]
    fn a_gate_refuses_to_open_over_a_row_it_cannot_place() {
        let world = World::new();
        let backend = Arc::new(FlakyBackend::new());
        backend
            .put(crate::authority_ledger::USAGE_NAMESPACE, &[0u8; 5], &[])
            .unwrap();
        let as_dyn: Arc<dyn StorageBackend> = backend;
        let opened = AuthorityGate::new_durable(
            public(&world.node),
            vec![public(&world.node)],
            GateSettings::default(),
            as_dyn,
            NOW,
        );
        assert!(opened.is_err());
    }
}
