//! Calling a node with authority (`docs/spec/00-wire.md` 8.5 and 8.6).
//!
//! A node that reports `authority = "audit"` or `"required"` in `HELLO_ACK`
//! takes a command in an `AUTH_REQUEST`: the command, the grant chain that
//! entitles the caller, and an invocation the caller signs for this command on
//! this connection. [`AgentClient::request_authorized`] builds the invocation,
//! sends the frame, reads the `RECEIPT` (if the node sends one) and the
//! `RESPONSE`, and checks the receipt against what was sent.
//!
//! # What a client may and may not conclude
//!
//! * In mode `audit` the node checks the invocation and **enforces nothing**:
//!   the command runs whatever the verdict and the answer does not say what the
//!   verdict was. [`Outcome::enforced`] is false. A receipt shows that the
//!   node authorised and performed this invocation; the absence of one in
//!   `audit` is how a client can tell the node would have refused it, and the
//!   reason is not given.
//! * In mode `off` the node refuses an `AUTH_REQUEST`; this client does not
//!   send one ([`AgentClient::request_authorized`] fails first), so a chain is
//!   never sent to a node that ignores it and limits are never believed to
//!   apply when they do not.
//! * Which action, effect class and node resource a command needs is the
//!   node's `classify` (`spine-core`); this crate does not depend on that, so
//!   the caller passes it as a [`Scope`].
//! * A receipt is checked for its signature (the node's key, which the caller
//!   supplies in [`Credentials`]), the invocation it names, the last grant and
//!   the digest of the `RESPONSE` body that came with it. It is not proof that
//!   the effect persisted.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
pub use spine_authority::{Action, Effect, PublicKey};
use spine_authority::{
    Grant, GrantId, Invocation, InvocationBody, Receipt, ResourceName, INVOCATION_VERSION,
    MAX_CHAIN_DEPTH,
};
use spine_protocol::auth::{
    check_receipt_body, command_op, encode_auth_request, random_nonce, result_digest,
};
use spine_protocol::body::Reply;
use spine_protocol::frame::FrameType;
use spine_protocol::hello::AuthorityMode;
use spine_protocol::identify::SigningKey;
use spine_protocol::native::{self, NativeError};
use spine_protocol::{BrowserCommand, WireError};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::AgentClient;

/// What an invocation covers: the action, the effect class and the node
/// resource that the node's classification of the command gives (`spine-core`
/// `authority::classify`, listed in `docs/spec/03-authority.md`). An invocation
/// that names less than the command needs is refused at step 10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// The action.
    pub action: Action,
    /// The declared effect class: the command's or a higher one.
    pub effect: Effect,
    /// The one resource the command touches: `{owner: node key, path:
    /// ["node", <area>]}`.
    pub resource: ResourceName,
}

/// What a client holds to call a node with authority: the key it signs with,
/// the grant chain that entitles that key (root first), and the node's key.
#[derive(Debug, Clone)]
pub struct Credentials {
    signer: SigningKey,
    chain: Vec<Vec<u8>>,
    leaf: GrantId,
    node: PublicKey,
}

impl Credentials {
    /// `chain` is the encoded grants (`Grant::encode`), root first; the last
    /// grant's audience must be `signer`'s public key, which is what makes the
    /// signer the invocation's invoker. `node` is the key of the node the
    /// invocations are for (the `recipient`); a receipt must be signed by it.
    pub fn new(
        signer: SigningKey,
        chain: Vec<Vec<u8>>,
        node: PublicKey,
    ) -> anyhow::Result<Credentials> {
        anyhow::ensure!(!chain.is_empty(), "a chain holds at least one grant");
        anyhow::ensure!(
            chain.len() <= MAX_CHAIN_DEPTH,
            "a chain holds at most {MAX_CHAIN_DEPTH} grants"
        );
        let mut last = None;
        for bytes in &chain {
            last = Some(Grant::decode(bytes).map_err(|e| anyhow::anyhow!("a grant: {e}"))?);
        }
        let leaf = last.ok_or_else(|| anyhow::anyhow!("a chain holds at least one grant"))?;
        anyhow::ensure!(
            leaf.body.aud == signer.verifying_key().to_bytes(),
            "the last grant of the chain is not for the signing key"
        );
        let leaf = leaf
            .id()
            .map_err(|e| anyhow::anyhow!("the last grant has no id: {e}"))?;
        Ok(Credentials {
            signer,
            chain,
            leaf,
            node,
        })
    }

    /// The public key the invocations are signed by.
    pub fn invoker(&self) -> PublicKey {
        self.signer.verifying_key().to_bytes()
    }

    /// The node the invocations are for.
    pub fn node(&self) -> PublicKey {
        self.node
    }

    /// The signed invocation for one command.
    ///
    /// `command_bytes` is the encoding of the command as it goes in the
    /// `AUTH_REQUEST` ([`native::encode_command`]); `op` is its SHA-256.
    /// `session` is the connection's `HELLO_ACK.binding`, `time` Unix seconds
    /// (the server's `time` plus the time since it was read, not the client's
    /// own clock) and `nonce` 16 single-use bytes. `cost` and `bytes` are
    /// declared as 0: this client sets no usage limits of its own.
    pub fn invocation(
        &self,
        scope: &Scope,
        command_bytes: &[u8],
        session: [u8; 32],
        time: u64,
        nonce: [u8; 16],
    ) -> anyhow::Result<Invocation> {
        let body = InvocationBody {
            version: INVOCATION_VERSION,
            invoker: self.invoker(),
            recipient: self.node,
            session,
            nonce,
            time,
            res: vec![scope.resource.clone()],
            act: scope.action,
            effect: scope.effect,
            op: command_op(command_bytes),
            leaf: self.leaf,
            cost: 0,
            bytes: 0,
        };
        Invocation::sign(&self.signer, body)
            .map_err(|e| anyhow::anyhow!("signing the invocation: {e}"))
    }
}

/// The answer to an `AUTH_REQUEST`.
#[derive(Debug)]
pub struct Outcome<T> {
    /// The `RESPONSE`: the command's `ok` value, or its `err`.
    pub reply: Result<T, WireError>,
    /// The node's signed receipt, checked, when it sent one. The node sends it
    /// only for an invocation it authorised and ran, whether the command then
    /// answered `ok` or `err`.
    pub receipt: Option<Receipt>,
    /// The authority mode the node reported at the handshake.
    pub mode: AuthorityMode,
}

impl<T> Outcome<T> {
    /// Whether the node refuses a command its gate refused: true only in mode
    /// `required`. False in `audit`, where a command with no receipt ran all
    /// the same.
    pub fn enforced(&self) -> bool {
        self.mode == AuthorityMode::Required
    }
}

/// Checks a `RECEIPT` body against the invocation it should be for and the
/// `RESPONSE` body that came with it.
///
/// The body has the receipt shape and size; it decodes canonically; it is
/// well formed, signed by `node` and valid; it records an allowed invocation;
/// it names this invocation and its last grant; and its `result` is the digest
/// of `response_body` ([`result_digest`]).
pub fn verify_receipt(
    body: &[u8],
    node: &PublicKey,
    invocation: &Invocation,
    response_body: &[u8],
) -> anyhow::Result<Receipt> {
    check_receipt_body(body).map_err(|e| anyhow::anyhow!("the receipt: {e}"))?;
    let receipt = Receipt::decode(body).map_err(|e| anyhow::anyhow!("the receipt: {e}"))?;
    receipt
        .verify(node)
        .map_err(|e| anyhow::anyhow!("the receipt: {e}"))?;
    let id = invocation
        .id()
        .map_err(|e| anyhow::anyhow!("the invocation has no id: {e}"))?;
    anyhow::ensure!(receipt.body.allowed, "the receipt is for a refusal");
    anyhow::ensure!(
        receipt.body.invocation == id,
        "the receipt is for another invocation"
    );
    anyhow::ensure!(
        receipt.body.leaf == invocation.body.leaf,
        "the receipt names another last grant"
    );
    anyhow::ensure!(
        receipt.body.result == result_digest(response_body),
        "the receipt is for another answer"
    );
    Ok(receipt)
}

/// How long one frame may take to write, as for a plain request.
const WRITE_DEADLINE: Duration = native::WRITE_DEADLINE;

impl<S> AgentClient<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    /// Sends `command` in an `AUTH_REQUEST` signed with `credentials` and
    /// returns the answer and the checked receipt.
    ///
    /// Fails without sending anything if the node's mode is `off`, or if this
    /// connection proved an identity other than the credentials' key (the node
    /// would refuse it at step 9). An `Err` result is for a failure of the
    /// connection or a receipt that does not check out; the command's own
    /// error is in [`Outcome::reply`]. The wait for the answer is not bounded
    /// here: bound it with a timeout and drop the connection if it fires.
    pub async fn request_authorized<T>(
        &mut self,
        credentials: &Credentials,
        scope: &Scope,
        command: &BrowserCommand,
    ) -> anyhow::Result<Outcome<T>>
    where
        T: Serialize + DeserializeOwned,
    {
        let mode = self.conn.authority();
        anyhow::ensure!(
            mode != AuthorityMode::Off,
            "the node's authority mode is off: it takes no authority requests"
        );
        if let Some(identity) = self.conn.identity() {
            anyhow::ensure!(
                identity == credentials.invoker(),
                "this connection proved another key than the one the invocation is signed with"
            );
        }
        let command_bytes = native::encode_command(command)?;
        let time = self
            .conn
            .server_time()
            .saturating_add(self.connected_at.elapsed().as_secs());
        let invocation = credentials.invocation(
            scope,
            &command_bytes,
            self.conn.binding(),
            time,
            random_nonce(),
        )?;
        let invocation_bytes = invocation
            .encode()
            .map_err(|e| anyhow::anyhow!("encoding the invocation: {e}"))?;
        let body = encode_auth_request(&credentials.chain, &invocation_bytes, command)?;
        let id = match tokio::time::timeout(WRITE_DEADLINE, self.conn.request_auth(body)).await {
            Ok(result) => result.map_err(NativeError::from)?,
            Err(_) => {
                self.conn.abandon();
                return Err(NativeError::WriteTimeout.into());
            }
        };

        let mut receipt_body: Option<Vec<u8>> = None;
        let response_body = loop {
            match self.conn.recv().await.map_err(NativeError::from)? {
                None => return Err(NativeError::Eof.into()),
                Some(frame) if frame.request_id == id => match frame.frame_type {
                    FrameType::Receipt => receipt_body = Some(frame.body),
                    FrameType::Response => break frame.body,
                    _ => {}
                },
                Some(_) => {}
            }
        };
        let reply: Reply<T> = native::decode_reply(&response_body)?;
        let receipt = match receipt_body {
            Some(bytes) => Some(verify_receipt(
                &bytes,
                &credentials.node,
                &invocation,
                &response_body,
            )?),
            None => None,
        };
        Ok(Outcome {
            reply: reply.into_result(),
            receipt,
            mode,
        })
    }

    /// Like [`AgentClient::request_authorized`], for a caller that wants the
    /// value or the error and the receipt: the command's `err` is an error
    /// ([`NativeError::Remote`]) and the receipt is `None` when the node sent
    /// none.
    pub async fn call_authorized<T>(
        &mut self,
        credentials: &Credentials,
        scope: &Scope,
        command: &BrowserCommand,
    ) -> anyhow::Result<(T, Option<Receipt>)>
    where
        T: Serialize + DeserializeOwned,
    {
        let outcome = self
            .request_authorized::<T>(credentials, scope, command)
            .await?;
        let value = outcome.reply.map_err(NativeError::from)?;
        Ok((value, outcome.receipt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_authority::{GrantSpec, Limits, ReceiptBody, ResourcePattern, RECEIPT_VERSION};
    use spine_protocol::auth::decode_auth_request;
    use spine_protocol::connection::{response_ok_body, Connection};
    use spine_protocol::frame::Frame;
    use tokio::io::{duplex, DuplexStream};

    const LIMIT: Duration = Duration::from_secs(20);
    const NOW: u64 = 1_700_000_000;

    fn key(n: u8) -> SigningKey {
        SigningKey::from_bytes(&[n; 32])
    }

    fn public(k: &SigningKey) -> PublicKey {
        k.verifying_key().to_bytes()
    }

    fn credentials() -> Credentials {
        let node = key(9);
        let agent = key(3);
        let root = Grant::issue_root(
            &node,
            GrantSpec {
                aud: public(&agent),
                role: "principal".to_string(),
                res: vec![ResourcePattern {
                    owner: public(&node),
                    path: vec!["node".to_string()],
                    prefix: true,
                }],
                act: vec![Action::Read, Action::Invoke],
                effect: Effect::Irreversible,
                delegable: 0,
                nbf: NOW - 100,
                exp: NOW + 3_600,
                limits: Limits::default(),
                caveats: Vec::new(),
                nonce: [1; 16],
            },
        )
        .unwrap();
        Credentials::new(agent, vec![root.encode().unwrap()], public(&node)).unwrap()
    }

    fn scope() -> Scope {
        Scope {
            action: Action::Invoke,
            effect: Effect::Irreversible,
            resource: ResourceName {
                owner: public(&key(9)),
                path: vec!["node".to_string(), "outbound".to_string()],
            },
        }
    }

    fn navigate() -> BrowserCommand {
        BrowserCommand::Navigate {
            url: "https://example.org/".to_string(),
        }
    }

    #[test]
    fn credentials_need_a_chain_for_the_signing_key() {
        let node = key(9);
        let agent = key(3);
        let c = credentials();
        assert_eq!(c.invoker(), public(&agent));
        assert_eq!(c.node(), public(&node));
        assert!(Credentials::new(agent.clone(), Vec::new(), public(&node)).is_err());
        // The chain is for the agent, not for another key.
        assert!(Credentials::new(key(4), c.chain.clone(), public(&node)).is_err());
        // Not a grant.
        assert!(Credentials::new(agent.clone(), vec![vec![1, 2, 3]], public(&node)).is_err());
        // More than eight grants.
        assert!(Credentials::new(agent, vec![c.chain[0].clone(); 9], public(&node)).is_err());
    }

    #[test]
    fn an_invocation_covers_the_command_this_connection_and_this_chain() {
        let c = credentials();
        let command_bytes = native::encode_command(&navigate()).unwrap();
        let session = [0x77; 32];
        let nonce = [5; 16];
        let invocation = c
            .invocation(&scope(), &command_bytes, session, NOW, nonce)
            .unwrap();
        let body = &invocation.body;
        assert_eq!(body.invoker, c.invoker());
        assert_eq!(body.recipient, c.node());
        assert_eq!(body.session, session);
        assert_eq!(body.nonce, nonce);
        assert_eq!(body.time, NOW);
        assert_eq!(body.res, vec![scope().resource]);
        assert_eq!(body.act, Action::Invoke);
        assert_eq!(body.effect, Effect::Irreversible);
        assert_eq!(body.op, command_op(&command_bytes));
        assert_eq!(body.leaf, c.leaf);
        assert_eq!((body.cost, body.bytes), (0, 0));
        assert!(invocation.verify_signature());
        assert!(invocation.is_well_formed());
        // Another command, another op.
        let other = c.invocation(&scope(), b"x", session, NOW, nonce).unwrap();
        assert_ne!(other.body.op, body.op);
    }

    // ---- A node that speaks the carrier, in-process.

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Behaviour {
        /// A correct receipt, then the answer.
        Honest,
        /// No receipt: the node would have refused (mode audit).
        NoReceipt,
        /// A receipt for another answer.
        WrongDigest,
        /// A receipt signed by a key that is not the node's.
        WrongSigner,
        /// A receipt for another invocation.
        OtherInvocation,
        /// A receipt that is a refusal.
        Denial,
    }

    fn receipt_for(
        signer: &SigningKey,
        invocation: &Invocation,
        result: [u8; 32],
        mutate: impl FnOnce(&mut ReceiptBody),
    ) -> Vec<u8> {
        let mut body = ReceiptBody {
            version: RECEIPT_VERSION,
            recipient: public(signer),
            invocation: invocation.id().unwrap(),
            leaf: invocation.body.leaf,
            allowed: true,
            effect: Some(invocation.body.effect),
            step: None,
            calls: 1,
            cost: 0,
            bytes: 0,
            result,
            time: NOW,
        };
        mutate(&mut body);
        Receipt::sign_unchecked(signer, body)
            .unwrap()
            .encode()
            .unwrap()
    }

    /// Serves one `AUTH_REQUEST`, checking what the client sent as a node
    /// would, and answers it as `behaviour` says.
    fn spawn_node(
        io: DuplexStream,
        mode: AuthorityMode,
        behaviour: Behaviour,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut local = native::local_config();
            local.authority = mode;
            let mut conn = Connection::server_handshake(io, &local, native::DEFAULT_HELLO_DEADLINE)
                .await
                .unwrap();
            while let Ok(Some(frame)) = conn.recv().await {
                if frame.frame_type != FrameType::AuthRequest {
                    panic!("expected an AUTH_REQUEST, got {:?}", frame.frame_type);
                }
                let request = decode_auth_request(&frame.body).unwrap();
                assert_eq!(request.command, navigate());
                let invocation = Invocation::decode(&request.invocation).unwrap();
                let node = key(9);
                assert_eq!(invocation.body.recipient, public(&node));
                assert_eq!(invocation.body.session, conn.binding());
                assert_eq!(invocation.body.op, command_op(&request.command_bytes));
                assert_eq!(request.chain, credentials().chain);
                assert!(invocation.verify_signature());
                // The invocation's time is the server's plus the elapsed time.
                let skew = invocation.body.time.abs_diff(conn.server_time());
                assert!(
                    skew <= 5,
                    "time {} vs {}",
                    invocation.body.time,
                    conn.server_time()
                );

                let response = response_ok_body(&serde_json::json!({"navigated": true})).unwrap();
                let digest = result_digest(&response);
                let receipt = match behaviour {
                    Behaviour::Honest => Some(receipt_for(&node, &invocation, digest, |_| {})),
                    Behaviour::NoReceipt => None,
                    Behaviour::WrongDigest => {
                        Some(receipt_for(&node, &invocation, [9; 32], |_| {}))
                    }
                    Behaviour::WrongSigner => {
                        Some(receipt_for(&key(77), &invocation, digest, |_| {}))
                    }
                    Behaviour::OtherInvocation => {
                        Some(receipt_for(&node, &invocation, digest, |b| {
                            b.invocation = [1; 32];
                        }))
                    }
                    Behaviour::Denial => Some(receipt_for(&node, &invocation, [0; 32], |b| {
                        b.allowed = false;
                        b.effect = None;
                        b.step = Some(9);
                        b.calls = 0;
                    })),
                };
                if let Some(body) = receipt {
                    conn.send(Frame {
                        frame_type: FrameType::Receipt,
                        request_id: frame.request_id,
                        body,
                    })
                    .await
                    .unwrap();
                }
                conn.send(Frame {
                    frame_type: FrameType::Response,
                    request_id: frame.request_id,
                    body: response,
                })
                .await
                .unwrap();
            }
        })
    }

    async fn client(mode: AuthorityMode, behaviour: Behaviour) -> AgentClient<DuplexStream> {
        let (client_io, server_io) = duplex(64 * 1024);
        spawn_node(server_io, mode, behaviour);
        tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn a_checked_receipt_comes_back_with_the_answer() {
        let mut c = client(AuthorityMode::Audit, Behaviour::Honest).await;
        let outcome = tokio::time::timeout(
            LIMIT,
            c.request_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            outcome.reply.unwrap(),
            serde_json::json!({"navigated": true})
        );
        let receipt = outcome.receipt.expect("a receipt");
        assert!(receipt.body.allowed);
        assert_eq!(receipt.body.recipient, public(&key(9)));
        assert_eq!(outcome.mode, AuthorityMode::Audit);
        // Audit enforces nothing, and the outcome says so.
        assert!(!Outcome::<()> {
            reply: Ok(()),
            receipt: None,
            mode: AuthorityMode::Audit
        }
        .enforced());
        assert!(Outcome::<()> {
            reply: Ok(()),
            receipt: None,
            mode: AuthorityMode::Required
        }
        .enforced());
        // Another call on the same connection works: the id was released.
        let (value, receipt) = tokio::time::timeout(
            LIMIT,
            c.call_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(value, serde_json::json!({"navigated": true}));
        assert!(receipt.is_some());
    }

    #[tokio::test]
    async fn no_receipt_is_not_an_error_in_audit() {
        let mut c = client(AuthorityMode::Audit, Behaviour::NoReceipt).await;
        let outcome = tokio::time::timeout(
            LIMIT,
            c.request_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate()),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(outcome.reply.is_ok());
        assert!(outcome.receipt.is_none());
        assert!(!outcome.enforced());
    }

    #[tokio::test]
    async fn a_receipt_that_does_not_check_out_is_an_error() {
        for behaviour in [
            Behaviour::WrongDigest,
            Behaviour::WrongSigner,
            Behaviour::OtherInvocation,
            Behaviour::Denial,
        ] {
            let mut c = client(AuthorityMode::Audit, behaviour).await;
            let result = tokio::time::timeout(
                LIMIT,
                c.request_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate()),
            )
            .await
            .unwrap();
            assert!(result.is_err(), "a bad receipt was accepted");
        }
    }

    #[tokio::test]
    async fn nothing_is_sent_to_a_node_that_takes_no_authority() {
        let (client_io, server_io) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let mut conn = Connection::server_handshake(
                server_io,
                &native::local_config(),
                native::DEFAULT_HELLO_DEADLINE,
            )
            .await
            .unwrap();
            // The client sends nothing and hangs up.
            matches!(conn.recv().await, Ok(None) | Err(_))
        });
        let mut c = tokio::time::timeout(LIMIT, AgentClient::over(client_io))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.authority(), AuthorityMode::Off);
        let error = c
            .request_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("off"), "{error}");
        assert_eq!(c.conn.in_flight_len(), 0);
        drop(c);
        assert!(tokio::time::timeout(LIMIT, server).await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn an_identity_that_is_not_the_invoker_is_not_sent() {
        let (client_io, server_io) = duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let mut local = native::local_config();
            local.authority = AuthorityMode::Audit;
            let mut conn =
                Connection::server_handshake(server_io, &local, native::DEFAULT_HELLO_DEADLINE)
                    .await
                    .unwrap();
            while let Ok(Some(frame)) = conn.recv().await {
                assert_ne!(frame.frame_type, FrameType::AuthRequest);
            }
        });
        let mut c =
            tokio::time::timeout(LIMIT, AgentClient::over_with_identity(client_io, &key(55)))
                .await
                .unwrap()
                .unwrap();
        let error = c
            .request_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("another key"), "{error}");
        drop(c);
        tokio::time::timeout(LIMIT, server).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_node_that_answers_with_an_error_gives_it_in_the_reply() {
        let (client_io, server_io) = duplex(64 * 1024);
        tokio::spawn(async move {
            let mut local = native::local_config();
            local.authority = AuthorityMode::Required;
            let mut conn =
                Connection::server_handshake(server_io, &local, native::DEFAULT_HELLO_DEADLINE)
                    .await
                    .unwrap();
            while let Ok(Some(frame)) = conn.recv().await {
                conn.respond_err(
                    frame.request_id,
                    "authority_step_9",
                    "authority refused at step 9",
                )
                .await
                .unwrap();
            }
        });
        let mut c = AgentClient::over(client_io).await.unwrap();
        let outcome = c
            .request_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate())
            .await
            .unwrap();
        assert_eq!(outcome.reply.as_ref().unwrap_err().code, "authority_step_9");
        assert!(outcome.receipt.is_none());
        assert!(outcome.enforced());
        let error = c
            .call_authorized::<serde_json::Value>(&credentials(), &scope(), &navigate())
            .await
            .unwrap_err();
        assert_eq!(native::remote_code(&error), Some("authority_step_9"));
    }
}
