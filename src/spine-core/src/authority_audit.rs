//! The node side of the authority carrier, in the modes this build runs:
//! `off` and `audit` (`docs/spec/00-wire.md` 8.5, 8.6;
//! `docs/spec/00-wire-revision.md` C1.4, C3, C5).
//!
//! A node answers two kinds of request frame. A plain `REQUEST` carries a
//! command and no authority. An `AUTH_REQUEST` carries a command, a grant
//! chain and a signed invocation. What the node does with each depends on its
//! mode:
//!
//! | Mode | `REQUEST` | `AUTH_REQUEST` |
//! | --- | --- | --- |
//! | `off` | runs | `authority_not_enabled`; does not run |
//! | `audit` | runs; logged and counted as presenting no authority | the gate runs; the command runs **whatever the verdict**; the verdict is logged and counted and the client is not told it; a `RECEIPT` is sent when the gate authorised it |
//! | `required` | `authority_required`; does not run | the gate runs; a refusal is `authority_step_N` and the command does not run |
//!
//! `required` is in [`decide`] so that the table is written once, but no node
//! in this build is in that mode: [`crate::config::AuthorityConfig::validate`]
//! refuses to start one. In `audit` the node enforces nothing, so a client that
//! reads `HELLO_ACK.authority = "audit"` must not believe it is protected by
//! limits or grants; that is the one thing this mode does not promise.
//!
//! What the gate records is the same in `audit` as it will be in `required`:
//! the nonce and the charge of an authorised invocation are written to the
//! ledger before the command runs, so limits and replay protection behave as
//! they will; a refusal changes nothing. In `audit` a charge that the ledger
//! refuses (a limit reached, a replayed nonce) is a refusal like any other: it
//! is counted, logged and not enforced.
//!
//! # What is pure
//!
//! [`decide`] is a pure function of the mode, the kind of frame and the gate's
//! verdict. [`AuthorityNode`] adds the gate, the node key, the counters and the
//! log; it never reads the clock (the caller passes `now`).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ed25519_dalek::SigningKey;
use spine_authority::{Invocation, PublicKey};
use spine_protocol::auth::{result_digest, AuthRequest};
use spine_protocol::body::{
    ERR_AUTHORITY_NOT_ENABLED, ERR_AUTHORITY_REQUIRED, ERR_AUTHORITY_STEP_1, ERR_AUTHORITY_STEP_10,
    ERR_AUTHORITY_STEP_11, ERR_AUTHORITY_STEP_2, ERR_AUTHORITY_STEP_3, ERR_AUTHORITY_STEP_4,
    ERR_AUTHORITY_STEP_5, ERR_AUTHORITY_STEP_6, ERR_AUTHORITY_STEP_7, ERR_AUTHORITY_STEP_8,
    ERR_AUTHORITY_STEP_9,
};
use spine_protocol::hello::AuthorityMode;
use spine_protocol::BrowserCommand;
use spine_storage::StorageBackend;
use tracing::{error, info, warn};

use crate::authority::{classify, AuthorityGate, Authorized, GateSettings, Refusal, RefusalCause};
use crate::operations::AuthorityCounts;

/// The wire code of each gate step, indexed by step number minus one.
const STEP_CODES: [&str; 11] = [
    ERR_AUTHORITY_STEP_1,
    ERR_AUTHORITY_STEP_2,
    ERR_AUTHORITY_STEP_3,
    ERR_AUTHORITY_STEP_4,
    ERR_AUTHORITY_STEP_5,
    ERR_AUTHORITY_STEP_6,
    ERR_AUTHORITY_STEP_7,
    ERR_AUTHORITY_STEP_8,
    ERR_AUTHORITY_STEP_9,
    ERR_AUTHORITY_STEP_10,
    ERR_AUTHORITY_STEP_11,
];

/// The fixed text of each step's error. It names the step and nothing else
/// (`docs/spec/03-authority.md` P31).
const STEP_MESSAGES: [&str; 11] = [
    "authority refused at step 1",
    "authority refused at step 2",
    "authority refused at step 3",
    "authority refused at step 4",
    "authority refused at step 5",
    "authority refused at step 6",
    "authority refused at step 7",
    "authority refused at step 8",
    "authority refused at step 9",
    "authority refused at step 10",
    "authority refused at step 11",
];

/// The wire code for a refusal at `step` (1 to 11). A number outside that
/// range is reported as step 11 rather than not at all: a refusal never
/// becomes an acceptance.
pub fn step_code(step: u8) -> &'static str {
    let index = usize::from(step.clamp(1, 11)) - 1;
    STEP_CODES[index]
}

fn step_message(step: u8) -> &'static str {
    let index = usize::from(step.clamp(1, 11)) - 1;
    STEP_MESSAGES[index]
}

/// Which request frame a command arrived in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// `REQUEST`: a command and no authority.
    Request,
    /// `AUTH_REQUEST`: a command, a chain and an invocation.
    AuthRequest,
}

/// What the gate said about an `AUTH_REQUEST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    /// Chain, invocation and command agree, and the charge was recorded.
    Authorised,
    /// The gate refused at this step (1 to 11); nothing was recorded.
    Refused(u8),
}

/// What happened to one request, for the counters and the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A plain `REQUEST` in mode `off`: nothing to record.
    Plain,
    /// A plain `REQUEST` in mode `audit` ran with no authority presented.
    Unauthenticated,
    /// An `AUTH_REQUEST` in mode `off`.
    NotEnabled,
    /// A plain `REQUEST` in mode `required`.
    AuthorityRequired,
    /// The gate authorised an `AUTH_REQUEST`.
    Authorised,
    /// The gate refused an `AUTH_REQUEST` at this step.
    Refused(u8),
}

/// What the node does with one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Whether the command runs.
    pub run: bool,
    /// The error code to answer with instead of running the command, and the
    /// fixed text that goes with it. `None` when the command runs.
    pub refuse_with: Option<(&'static str, &'static str)>,
    /// Whether a `RECEIPT` follows once the command has run.
    pub receipt: bool,
    /// What to record.
    pub outcome: Outcome,
}

/// Whether the gate must be asked about a request: an `AUTH_REQUEST` in any
/// mode but `off`.
pub fn needs_gate(mode: AuthorityMode, kind: FrameKind) -> bool {
    kind == FrameKind::AuthRequest && mode != AuthorityMode::Off
}

/// The mode table (C1.4): what the node does with a request of `kind` in
/// `mode`, given the gate's verdict where [`needs_gate`] says it was asked.
///
/// A verdict that is missing where it was needed counts as a refusal at step
/// 11, so a gap in the caller can only make the node stricter.
pub fn decide(mode: AuthorityMode, kind: FrameKind, verdict: Option<GateVerdict>) -> Plan {
    let run = |outcome| Plan {
        run: true,
        refuse_with: None,
        receipt: false,
        outcome,
    };
    let refuse = |code, message, outcome| Plan {
        run: false,
        refuse_with: Some((code, message)),
        receipt: false,
        outcome,
    };
    match (mode, kind) {
        (AuthorityMode::Off, FrameKind::Request) => run(Outcome::Plain),
        (AuthorityMode::Off, FrameKind::AuthRequest) => refuse(
            ERR_AUTHORITY_NOT_ENABLED,
            "this node does not take authority requests",
            Outcome::NotEnabled,
        ),
        (AuthorityMode::Audit, FrameKind::Request) => run(Outcome::Unauthenticated),
        (AuthorityMode::Required, FrameKind::Request) => refuse(
            ERR_AUTHORITY_REQUIRED,
            "this node requires authority for every request",
            Outcome::AuthorityRequired,
        ),
        (AuthorityMode::Audit, FrameKind::AuthRequest) => {
            match verdict.unwrap_or(GateVerdict::Refused(11)) {
                GateVerdict::Authorised => Plan {
                    receipt: true,
                    ..run(Outcome::Authorised)
                },
                // The verdict is the node's to log, not the client's to see.
                GateVerdict::Refused(step) => run(Outcome::Refused(step)),
            }
        }
        (AuthorityMode::Required, FrameKind::AuthRequest) => {
            match verdict.unwrap_or(GateVerdict::Refused(11)) {
                GateVerdict::Authorised => Plan {
                    receipt: true,
                    ..run(Outcome::Authorised)
                },
                GateVerdict::Refused(step) => {
                    refuse(step_code(step), step_message(step), Outcome::Refused(step))
                }
            }
        }
    }
}

/// What the daemon must do with a request, after the gate has been asked.
#[derive(Debug)]
pub enum Admission {
    /// Answer with this error code and text; the command does not run.
    Refuse {
        code: &'static str,
        message: &'static str,
    },
    /// Run the command. `Some` when the gate authorised it: the daemon issues
    /// the receipt from it once the command has been answered.
    Run(Option<Box<Authorized>>),
}

/// What the gate needs to know about the connection an `AUTH_REQUEST` came on.
#[derive(Debug, Clone, Copy)]
pub struct AuthContext {
    /// The key the connection proved with `IDENTIFY`, if it did.
    pub identity: Option<[u8; 32]>,
    /// The connection's `HELLO_ACK.binding`: the invocation's `session`.
    pub binding: [u8; 32],
    /// The node's clock, Unix seconds.
    pub now: u64,
}

/// The settings of an audit node that come from configuration.
#[derive(Debug, Clone)]
pub struct AuditSettings {
    /// Keys accepted as the root of a chain besides the node's own.
    pub trusted_roots: Vec<PublicKey>,
    /// `authority.skew_secs`.
    pub skew_secs: u64,
    /// `authority.max_invocation_age_secs`.
    pub max_invocation_age_secs: u64,
}

#[derive(Debug, Default)]
struct Tally {
    authorised: AtomicU64,
    refused: [AtomicU64; 11],
    unauthenticated: AtomicU64,
    not_enabled: AtomicU64,
    malformed: AtomicU64,
    rate_limited: AtomicU64,
    receipts: AtomicU64,
    receipt_failures: AtomicU64,
}

fn bump(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

fn read(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// An identity for the log: the key in hex, or `anonymous`. Never a metric
/// label.
fn who(identity: Option<[u8; 32]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    match identity {
        Some(key) => {
            let mut text = String::with_capacity(64);
            for byte in key {
                text.push(char::from(DIGITS[usize::from(byte >> 4)]));
                text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
            }
            text
        }
        None => "anonymous".to_string(),
    }
}

/// The node's authority state: the mode, the gate and the node key (in mode
/// `audit`), and the counters.
pub struct AuthorityNode {
    mode: AuthorityMode,
    gate: Option<AuthorityGate>,
    signer: Option<SigningKey>,
    tally: Tally,
}

impl fmt::Debug for AuthorityNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorityNode")
            .field("mode", &self.mode)
            .field("gate", &self.gate.is_some())
            .finish_non_exhaustive()
    }
}

impl AuthorityNode {
    /// A node in mode `off`: no gate, no key.
    pub fn off() -> AuthorityNode {
        AuthorityNode {
            mode: AuthorityMode::Off,
            gate: None,
            signer: None,
            tally: Tally::default(),
        }
    }

    /// A node in mode `audit`. `signer` is the node's key: it is the recipient
    /// every invocation must name, a trusted root, and the key that signs
    /// receipts. `backend`, when there is one, makes the usage ledger survive a
    /// restart (see [`crate::authority_ledger`]); `None` keeps it in memory.
    /// `now` is the node's clock.
    pub fn audit(
        settings: &AuditSettings,
        signer: SigningKey,
        backend: Option<Arc<dyn StorageBackend>>,
        now: u64,
    ) -> anyhow::Result<AuthorityNode> {
        let recipient: PublicKey = signer.verifying_key().to_bytes();
        let mut roots: Vec<PublicKey> = vec![recipient];
        for key in &settings.trusted_roots {
            if !roots.contains(key) {
                roots.push(*key);
            }
        }
        let gate_settings = GateSettings {
            skew_secs: settings.skew_secs,
            max_invocation_age_secs: settings.max_invocation_age_secs,
            ..GateSettings::default()
        };
        let gate = match backend {
            Some(backend) => {
                AuthorityGate::new_durable(recipient, roots, gate_settings, backend, now)
                    .map_err(|e| anyhow::anyhow!("opening the authority usage ledger: {e}"))?
            }
            None => AuthorityGate::new(recipient, roots, gate_settings),
        };
        Ok(AuthorityNode {
            mode: AuthorityMode::Audit,
            gate: Some(gate),
            signer: Some(signer),
            tally: Tally::default(),
        })
    }

    /// The mode this node reports in `HELLO_ACK.authority`.
    pub fn mode(&self) -> AuthorityMode {
        self.mode
    }

    /// The node's public key, when it has a gate.
    pub fn node_key(&self) -> Option<PublicKey> {
        self.gate.as_ref().map(AuthorityGate::recipient)
    }

    /// The counters, for `/metrics`.
    pub fn counts(&self) -> AuthorityCounts {
        let t = &self.tally;
        let mut refused = [0u64; 11];
        for (slot, counter) in refused.iter_mut().zip(t.refused.iter()) {
            *slot = read(counter);
        }
        AuthorityCounts {
            enforcing: self.mode == AuthorityMode::Required,
            authorised: read(&t.authorised),
            refused,
            unauthenticated: read(&t.unauthenticated),
            not_enabled: read(&t.not_enabled),
            malformed: read(&t.malformed),
            rate_limited: read(&t.rate_limited),
            receipts: read(&t.receipts),
            receipt_failures: read(&t.receipt_failures),
        }
    }

    fn record(&self, outcome: Outcome) {
        let t = &self.tally;
        match outcome {
            Outcome::Plain | Outcome::AuthorityRequired => {}
            Outcome::Unauthenticated => bump(&t.unauthenticated),
            Outcome::NotEnabled => bump(&t.not_enabled),
            Outcome::Authorised => bump(&t.authorised),
            Outcome::Refused(step) => {
                let index = usize::from(step.clamp(1, 11)) - 1;
                bump(&t.refused[index]);
            }
        }
    }

    /// In mode `off` an `AUTH_REQUEST` is refused before its body is read:
    /// `Some(Admission::Refuse)`, counted. In any other mode `None`, and the
    /// caller goes on to read the body.
    pub fn refuse_if_off(&self) -> Option<Admission> {
        if needs_gate(self.mode, FrameKind::AuthRequest) {
            return None;
        }
        let plan = decide(self.mode, FrameKind::AuthRequest, None);
        self.record(plan.outcome);
        plan.refuse_with
            .map(|(code, message)| Admission::Refuse { code, message })
    }

    /// An `AUTH_REQUEST` body that was refused before the gate (it was not
    /// `{auth, command}`, or its command did not decode).
    pub fn note_malformed(&self) {
        bump(&self.tally.malformed);
    }

    /// An `AUTH_REQUEST` that the connection's rate limit refused before the
    /// gate: it consumed no nonce and may be sent again.
    pub fn note_rate_limited(&self) {
        bump(&self.tally.rate_limited);
    }

    /// What the node does with a plain `REQUEST` carrying `command`.
    pub fn admit_plain(&self, command: &BrowserCommand, identity: Option<[u8; 32]>) -> Admission {
        let plan = decide(self.mode, FrameKind::Request, None);
        self.record(plan.outcome);
        if plan.outcome == Outcome::Unauthenticated {
            let need = classify(command);
            info!(
                identity = %who(identity),
                action = ?need.action,
                effect = ?need.effect,
                area = %need.node_path.join("/"),
                "authority audit: a command ran with no authority presented"
            );
        }
        match plan.refuse_with {
            Some((code, message)) => Admission::Refuse { code, message },
            None => Admission::Run(None),
        }
    }

    /// What the node does with an `AUTH_REQUEST` whose body decoded and whose
    /// rate limit allowed it.
    ///
    /// In mode `off` this does not touch the gate. Otherwise it runs, in
    /// order: the identity rule (C3.5 step 4: a connection that proved a key
    /// carries only invocations by that key, refused at step 9 without
    /// consuming a nonce) and then the gate. The gate does the CPU work of up
    /// to nine signature checks and may write the ledger, so an async caller
    /// should run this on a blocking thread.
    pub fn admit_auth(&self, request: &AuthRequest, context: &AuthContext) -> Admission {
        let verdict = if needs_gate(self.mode, FrameKind::AuthRequest) {
            Some(self.ask_gate(request, context))
        } else {
            None
        };
        let summary = verdict.as_ref().map(|result| match result {
            Ok(_) => GateVerdict::Authorised,
            Err(refusal) => GateVerdict::Refused(refusal.step()),
        });
        let plan = decide(self.mode, FrameKind::AuthRequest, summary);
        self.record(plan.outcome);
        self.log_verdict(request, context, &plan, verdict.as_ref());
        match (plan.refuse_with, verdict) {
            (Some((code, message)), _) => Admission::Refuse { code, message },
            (None, Some(Ok(authorized))) if plan.receipt => {
                Admission::Run(Some(Box::new(authorized)))
            }
            (None, _) => Admission::Run(None),
        }
    }

    fn ask_gate(
        &self,
        request: &AuthRequest,
        context: &AuthContext,
    ) -> Result<Authorized, Refusal> {
        let Some(gate) = self.gate.as_ref() else {
            // Not reachable: `needs_gate` is false for a node without one.
            return Err(Refusal::from(RefusalCause::LedgerUnavailable));
        };
        if let Some(connection_key) = context.identity {
            if let Ok(invocation) = Invocation::decode(&request.invocation) {
                if invocation.body.invoker != connection_key {
                    return Err(Refusal::from(RefusalCause::IdentityMismatch));
                }
            }
        }
        // The node holds no revocation feed, so its set is empty and complete
        // as of now: a grant is never refused as revoked in this build.
        gate.refresh_revocations(context.now);
        gate.authorize(
            &request.chain,
            &request.invocation,
            &request.command,
            crate::authority::operation_hash(&request.command_bytes),
            context.binding,
            context.now,
        )
    }

    fn log_verdict(
        &self,
        request: &AuthRequest,
        context: &AuthContext,
        plan: &Plan,
        verdict: Option<&Result<Authorized, Refusal>>,
    ) {
        let need = classify(&request.command);
        let identity = who(context.identity);
        let area = need.node_path.join("/");
        match (plan.outcome, verdict) {
            (Outcome::Authorised, _) => info!(
                identity = %identity,
                action = ?need.action,
                effect = ?need.effect,
                area = %area,
                "authority audit: the gate authorised the command"
            ),
            (Outcome::Refused(step), Some(Err(refusal))) => warn!(
                identity = %identity,
                action = ?need.action,
                effect = ?need.effect,
                area = %area,
                step,
                cause = %refusal.cause(),
                enforced = !plan.run,
                "authority audit: the gate refused the command"
            ),
            (Outcome::NotEnabled, _) => info!(
                identity = %identity,
                "an AUTH_REQUEST was refused: authority is not enabled on this node"
            ),
            _ => {}
        }
    }

    /// The `RECEIPT` body for an invocation the gate authorised, once its
    /// command has been answered with a `RESPONSE` whose body is
    /// `response_body` (the body the daemon sends, after any replacement of an
    /// answer too large to send). `None`, and a log line and a count, when the
    /// receipt could not be made: the command has run and the client simply
    /// gets no receipt.
    pub fn issue_receipt(
        &self,
        authorized: &Authorized,
        response_body: &[u8],
        now: u64,
    ) -> Option<Vec<u8>> {
        let (gate, signer) = (self.gate.as_ref()?, self.signer.as_ref()?);
        let made = gate
            .receipt(signer, authorized, result_digest(response_body), now)
            .map_err(|e| e.to_string())
            .and_then(|receipt| receipt.encode().map_err(|e| e.to_string()))
            .and_then(|bytes| {
                if bytes.len() > spine_protocol::auth::MAX_RECEIPT_BYTES {
                    Err(format!("the receipt is {} bytes", bytes.len()))
                } else {
                    Ok(bytes)
                }
            });
        match made {
            Ok(bytes) => {
                bump(&self.tally.receipts);
                Some(bytes)
            }
            Err(message) => {
                bump(&self.tally.receipt_failures);
                error!("authority audit: a receipt could not be made: {message}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_authority::{
        Action, Effect, Grant, GrantSpec, InvocationBody, Limits, Receipt, INVOCATION_VERSION,
    };
    use spine_protocol::auth::{decode_auth_request, encode_auth_request};
    use spine_protocol::body::ALL_ERROR_CODES;

    const NOW: u64 = 5_000;
    const BINDING: [u8; 32] = [0x55; 32];

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn public(k: &SigningKey) -> PublicKey {
        k.verifying_key().to_bytes()
    }

    const MODES: [AuthorityMode; 3] = [
        AuthorityMode::Off,
        AuthorityMode::Audit,
        AuthorityMode::Required,
    ];

    #[test]
    fn the_mode_table_for_a_plain_request() {
        let off = decide(AuthorityMode::Off, FrameKind::Request, None);
        assert!(off.run && off.refuse_with.is_none() && !off.receipt);
        assert_eq!(off.outcome, Outcome::Plain);
        let audit = decide(AuthorityMode::Audit, FrameKind::Request, None);
        assert!(audit.run && audit.refuse_with.is_none() && !audit.receipt);
        assert_eq!(audit.outcome, Outcome::Unauthenticated);
        let required = decide(AuthorityMode::Required, FrameKind::Request, None);
        assert!(!required.run && !required.receipt);
        assert_eq!(required.refuse_with.unwrap().0, "authority_required");
    }

    #[test]
    fn the_mode_table_for_an_auth_request() {
        // off: refused whatever a verdict might say, and the gate is not asked.
        assert!(!needs_gate(AuthorityMode::Off, FrameKind::AuthRequest));
        for verdict in [
            None,
            Some(GateVerdict::Authorised),
            Some(GateVerdict::Refused(3)),
        ] {
            let plan = decide(AuthorityMode::Off, FrameKind::AuthRequest, verdict);
            assert!(!plan.run && !plan.receipt);
            assert_eq!(plan.refuse_with.unwrap().0, "authority_not_enabled");
            assert_eq!(plan.outcome, Outcome::NotEnabled);
        }
        // audit: always runs, answers nothing about the verdict.
        assert!(needs_gate(AuthorityMode::Audit, FrameKind::AuthRequest));
        let authorised = decide(
            AuthorityMode::Audit,
            FrameKind::AuthRequest,
            Some(GateVerdict::Authorised),
        );
        assert!(authorised.run && authorised.receipt && authorised.refuse_with.is_none());
        assert_eq!(authorised.outcome, Outcome::Authorised);
        for step in 1..=11u8 {
            let plan = decide(
                AuthorityMode::Audit,
                FrameKind::AuthRequest,
                Some(GateVerdict::Refused(step)),
            );
            assert!(
                plan.run && !plan.receipt && plan.refuse_with.is_none(),
                "{step}"
            );
            assert_eq!(plan.outcome, Outcome::Refused(step));
        }
        // required: runs only when authorised, refuses with the step's code.
        let authorised = decide(
            AuthorityMode::Required,
            FrameKind::AuthRequest,
            Some(GateVerdict::Authorised),
        );
        assert!(authorised.run && authorised.receipt);
        for step in 1..=11u8 {
            let plan = decide(
                AuthorityMode::Required,
                FrameKind::AuthRequest,
                Some(GateVerdict::Refused(step)),
            );
            assert!(!plan.run && !plan.receipt, "{step}");
            assert_eq!(
                plan.refuse_with.unwrap().0,
                format!("authority_step_{step}"),
                "{step}"
            );
        }
    }

    #[test]
    fn a_missing_verdict_never_makes_required_less_strict() {
        let plan = decide(AuthorityMode::Required, FrameKind::AuthRequest, None);
        assert!(!plan.run && !plan.receipt);
        assert_eq!(plan.refuse_with.unwrap().0, "authority_step_11");
        assert_eq!(plan.outcome, Outcome::Refused(11));
    }

    #[test]
    fn a_receipt_is_never_planned_for_a_refusal_or_a_plain_request() {
        for mode in MODES {
            let plain = decide(mode, FrameKind::Request, None);
            assert!(!plain.receipt, "{mode:?}");
            for step in 1..=11u8 {
                let plan = decide(
                    mode,
                    FrameKind::AuthRequest,
                    Some(GateVerdict::Refused(step)),
                );
                assert!(!plan.receipt, "{mode:?} {step}");
            }
        }
        // And off never plans one at all.
        assert!(
            !decide(
                AuthorityMode::Off,
                FrameKind::AuthRequest,
                Some(GateVerdict::Authorised)
            )
            .receipt
        );
    }

    #[test]
    fn every_refusal_code_is_a_listed_code_and_the_step_codes_are_in_order() {
        for step in 1..=11u8 {
            let code = step_code(step);
            assert_eq!(code, format!("authority_step_{step}"));
            assert!(ALL_ERROR_CODES.contains(&code));
            assert_eq!(
                step_message(step),
                format!("authority refused at step {step}")
            );
        }
        assert_eq!(step_code(0), "authority_step_1");
        assert_eq!(step_code(12), "authority_step_11");
    }

    // ---- The node, with a real gate.

    /// The node (key 9) in audit mode; an operator (key 2) holds a root
    /// grant from it and has delegated the whole tree to an agent (key 3).
    struct World {
        node_key: SigningKey,
        agent: SigningKey,
        node: AuthorityNode,
        chain: Vec<Vec<u8>>,
        leaf: Grant,
    }

    fn world(limits: Limits) -> World {
        let node_key = key(9);
        let operator = key(2);
        let agent = key(3);
        let node = AuthorityNode::audit(
            &AuditSettings {
                trusted_roots: Vec::new(),
                skew_secs: 0,
                max_invocation_age_secs: 60,
            },
            node_key.clone(),
            None,
            NOW,
        )
        .unwrap();
        let root =
            AuthorityGate::issue_self_root(&node_key, public(&operator), NOW - 100, 3_600, [1; 16])
                .unwrap();
        let leaf = Grant::delegate(
            &operator,
            &root,
            GrantSpec {
                aud: public(&agent),
                role: "agent".to_string(),
                res: vec![crate::authority::node_tree(public(&node_key))],
                act: vec![Action::Read, Action::Invoke, Action::Publish, Action::Admin],
                effect: Effect::Irreversible,
                delegable: 0,
                nbf: NOW - 100,
                exp: NOW + 1_000,
                limits,
                caveats: Vec::new(),
                nonce: [2; 16],
            },
        )
        .unwrap();
        let chain = vec![root.encode().unwrap(), leaf.encode().unwrap()];
        World {
            node_key,
            agent,
            node,
            chain,
            leaf,
        }
    }

    fn navigate() -> BrowserCommand {
        BrowserCommand::Navigate {
            url: "https://example.org/".to_string(),
        }
    }

    impl World {
        fn invocation(&self, command: &BrowserCommand, nonce: u8, session: [u8; 32]) -> Vec<u8> {
            let need = classify(command);
            let op =
                crate::authority::operation_hash(&spine_protocol::cbor::to_vec(command).unwrap());
            let body = InvocationBody {
                version: INVOCATION_VERSION,
                invoker: public(&self.agent),
                recipient: public(&self.node_key),
                session,
                nonce: [nonce; 16],
                time: NOW,
                res: vec![need.resource(public(&self.node_key))],
                act: need.action,
                effect: need.effect,
                op,
                leaf: self.leaf.id().unwrap(),
                cost: 0,
                bytes: 0,
            };
            Invocation::sign(&self.agent, body)
                .unwrap()
                .encode()
                .unwrap()
        }

        fn request(&self, command: &BrowserCommand, nonce: u8) -> AuthRequest {
            let body = encode_auth_request(
                &self.chain,
                &self.invocation(command, nonce, BINDING),
                command,
            )
            .unwrap();
            decode_auth_request(&body).unwrap()
        }
    }

    fn ctx(identity: Option<[u8; 32]>) -> AuthContext {
        AuthContext {
            identity,
            binding: BINDING,
            now: NOW,
        }
    }

    fn refused_step(admission: &Admission) -> Option<&'static str> {
        match admission {
            Admission::Refuse { code, .. } => Some(*code),
            Admission::Run(_) => None,
        }
    }

    #[test]
    fn an_authorised_request_runs_and_is_counted_and_gets_a_receipt() {
        let w = world(Limits::default());
        let command = navigate();
        let admission = w.node.admit_auth(&w.request(&command, 1), &ctx(None));
        let Admission::Run(Some(authorized)) = admission else {
            panic!("expected a run with an authorisation: {admission:?}");
        };
        let counts = w.node.counts();
        assert_eq!(counts.authorised, 1);
        assert_eq!(counts.refused, [0; 11]);

        let response_body = [0xa1, 0x62, b'o', b'k', 0x01];
        let bytes = w
            .node
            .issue_receipt(&authorized, &response_body, NOW + 1)
            .unwrap();
        assert!(bytes.len() <= 512);
        spine_protocol::auth::check_receipt_body(&bytes).unwrap();
        let receipt = Receipt::decode(&bytes).unwrap();
        receipt.verify(&public(&w.node_key)).unwrap();
        assert!(receipt.body.allowed);
        assert_eq!(receipt.body.result, result_digest(&response_body));
        assert_eq!(receipt.body.time, NOW + 1);
        assert_eq!(receipt.body.effect, Some(Effect::Irreversible));
        assert_eq!(w.node.counts().receipts, 1);
    }

    #[test]
    fn a_refused_request_still_runs_in_audit_and_is_counted_by_step() {
        let w = world(Limits::default());
        let command = navigate();
        // Step 9: an invocation made for another connection.
        let wrong_session = {
            let body =
                encode_auth_request(&w.chain, &w.invocation(&command, 1, [0x66; 32]), &command)
                    .unwrap();
            decode_auth_request(&body).unwrap()
        };
        let admission = w.node.admit_auth(&wrong_session, &ctx(None));
        assert!(matches!(admission, Admission::Run(None)), "{admission:?}");
        // Step 1: no chain.
        let no_chain = {
            let body =
                encode_auth_request(&[], &w.invocation(&command, 2, BINDING), &command).unwrap();
            decode_auth_request(&body).unwrap()
        };
        assert!(matches!(
            w.node.admit_auth(&no_chain, &ctx(None)),
            Admission::Run(None)
        ));
        let counts = w.node.counts();
        assert_eq!(counts.authorised, 0);
        assert_eq!(counts.refused[8], 1, "step 9");
        assert_eq!(counts.refused[0], 1, "step 1");
        assert_eq!(counts.receipts, 0);
    }

    #[test]
    fn a_refusal_records_nothing_so_the_same_nonce_can_still_be_used() {
        let w = world(Limits::default());
        let command = navigate();
        let body = encode_auth_request(&w.chain, &w.invocation(&command, 7, [0x66; 32]), &command)
            .unwrap();
        let bad = decode_auth_request(&body).unwrap();
        assert!(matches!(
            w.node.admit_auth(&bad, &ctx(None)),
            Admission::Run(None)
        ));
        // The same nonce, for this connection, is authorised: the refusal did
        // not consume it.
        let good = w.request(&command, 7);
        assert!(matches!(
            w.node.admit_auth(&good, &ctx(None)),
            Admission::Run(Some(_))
        ));
        // And now it is spent: a replay is refused at step 9, but still runs.
        let replay = w.request(&command, 7);
        assert!(matches!(
            w.node.admit_auth(&replay, &ctx(None)),
            Admission::Run(None)
        ));
        let counts = w.node.counts();
        assert_eq!(counts.authorised, 1);
        assert_eq!(counts.refused[8], 2);
    }

    #[test]
    fn a_limit_is_charged_in_audit_as_it_will_be_in_required() {
        let limits = Limits {
            calls: Some(1),
            ..Limits::default()
        };
        let w = world(limits);
        let command = navigate();
        assert!(matches!(
            w.node.admit_auth(&w.request(&command, 1), &ctx(None)),
            Admission::Run(Some(_))
        ));
        // The second call exceeds the one the grant allows: step 11, and the
        // command would run all the same in audit.
        assert!(matches!(
            w.node.admit_auth(&w.request(&command, 2), &ctx(None)),
            Admission::Run(None)
        ));
        let counts = w.node.counts();
        assert_eq!(counts.authorised, 1);
        assert_eq!(counts.refused[10], 1, "step 11");
    }

    #[test]
    fn a_connection_identity_other_than_the_invoker_is_refused_at_step_9() {
        let w = world(Limits::default());
        let command = navigate();
        let stranger = public(&key(77));
        let admission = w
            .node
            .admit_auth(&w.request(&command, 1), &ctx(Some(stranger)));
        assert!(matches!(admission, Admission::Run(None)), "{admission:?}");
        assert_eq!(w.node.counts().refused[8], 1);
        // The refusal recorded no nonce.
        let agent = public(&w.agent);
        assert!(matches!(
            w.node
                .admit_auth(&w.request(&command, 1), &ctx(Some(agent))),
            Admission::Run(Some(_))
        ));
        // An anonymous connection is not subject to the rule.
        assert!(matches!(
            w.node.admit_auth(&w.request(&command, 2), &ctx(None)),
            Admission::Run(Some(_))
        ));
    }

    #[test]
    fn the_identity_rule_reports_as_step_9() {
        let refusal = Refusal::from(RefusalCause::IdentityMismatch);
        assert_eq!(refusal.step(), 9);
        assert_eq!(refusal.name(), "authority_step_9");
    }

    #[test]
    fn an_off_node_refuses_without_touching_a_gate() {
        let node = AuthorityNode::off();
        let w = world(Limits::default());
        let command = navigate();
        let admission = node.admit_auth(&w.request(&command, 1), &ctx(None));
        assert_eq!(refused_step(&admission), Some("authority_not_enabled"));
        assert_eq!(node.counts().not_enabled, 1);
        assert_eq!(node.counts().authorised, 0);
        assert!(node.node_key().is_none());
        assert!(matches!(
            node.admit_plain(&command, None),
            Admission::Run(None)
        ));
        assert_eq!(node.counts().unauthenticated, 0);
        assert!(!node.counts().enforcing);
    }

    #[test]
    fn an_audit_node_counts_plain_requests_and_runs_them() {
        let w = world(Limits::default());
        assert!(matches!(
            w.node.admit_plain(&navigate(), None),
            Admission::Run(None)
        ));
        assert!(matches!(
            w.node.admit_plain(&BrowserCommand::GetUR, Some([1; 32])),
            Admission::Run(None)
        ));
        assert_eq!(w.node.counts().unauthenticated, 2);
        assert_eq!(w.node.mode(), AuthorityMode::Audit);
        assert_eq!(w.node.node_key(), Some(public(&w.node_key)));
        assert!(!w.node.counts().enforcing);
    }

    #[test]
    fn the_configured_roots_and_the_node_key_are_the_trusted_roots() {
        let node = AuthorityNode::audit(
            &AuditSettings {
                trusted_roots: vec![[4; 32], public(&key(9)), [4; 32]],
                skew_secs: 0,
                max_invocation_age_secs: 60,
            },
            key(9),
            None,
            NOW,
        )
        .unwrap();
        let gate = node.gate.as_ref().unwrap();
        assert_eq!(gate.trusted_roots(), &[public(&key(9)), [4; 32]]);
    }

    #[test]
    fn the_result_digest_of_a_receipt_is_over_the_body_sent() {
        let w = world(Limits::default());
        let command = navigate();
        let Admission::Run(Some(authorized)) =
            w.node.admit_auth(&w.request(&command, 1), &ctx(None))
        else {
            panic!("expected an authorisation");
        };
        let one = w.node.issue_receipt(&authorized, &[0xa0], NOW).unwrap();
        let two = w
            .node
            .issue_receipt(&authorized, &[0xa1, 0x00, 0x00], NOW)
            .unwrap();
        let (one, two) = (
            Receipt::decode(&one).unwrap(),
            Receipt::decode(&two).unwrap(),
        );
        assert_ne!(one.body.result, two.body.result);
        assert_eq!(one.body.invocation, two.body.invocation);
    }

    #[test]
    fn counters_render_with_fixed_labels() {
        let w = world(Limits::default());
        w.node.note_malformed();
        w.node.note_rate_limited();
        let counts = w.node.counts();
        assert_eq!((counts.malformed, counts.rate_limited), (1, 1));
    }
}
