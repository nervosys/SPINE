//! The bodies of `AUTH_REQUEST` and `RECEIPT` (`docs/spec/00-wire.md` 8.5 and
//! 8.6).
//!
//! An `AUTH_REQUEST` body is the closed map `{auth, command}`: `command` is
//! exactly the item a plain `REQUEST` carries, and `auth` is the closed map
//! `{chain, invocation}` of the grant chain (an array of byte strings, root
//! first) and the signed invocation (a byte string). This module reads and
//! writes that shape and nothing about what the bytes mean: the grants and the
//! invocation are decoded, bounded and verified by `spine-authority` and the
//! node's gate, so that every failure there has one authority verdict.
//!
//! What the decoder is strict about, and why:
//!
//! * Both maps are read as maps. A struct that serde derives would also accept
//!   a CBOR array in its place, which would give one value two encodings; the
//!   readers here accept only `map`.
//! * The `command` member is read as a generic CBOR item, re-encoded (the body
//!   is deterministic CBOR, so this gives back the received bytes), and decoded
//!   by [`crate::native::decode_command`], so the command is judged exactly as
//!   a `REQUEST` body is and the bytes the invocation's `op` hashes are the
//!   bytes of that item.
//! * Memory. The frame limit bounds the body, but an array of a million empty
//!   byte strings would still cost far more than the body's size. The chain is
//!   read one item at a time and at most [`MAX_CHAIN_ITEMS`] + 1 items are
//!   kept: enough for the gate to refuse a ninth at its first step.

use std::fmt;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::cbor::{self, CborError};
use crate::native::{classify_request_error, decode_command, RequestError};
use crate::{wirefmt, BrowserCommand};

/// Most grants a chain may hold (`spine_authority::MAX_CHAIN_DEPTH`). Items
/// beyond this are counted and dropped; the gate refuses the chain at step 1.
pub const MAX_CHAIN_ITEMS: usize = 8;

/// Longest `RECEIPT` body, in bytes (`spine_authority::MAX_RECEIPT_BYTES`).
pub const MAX_RECEIPT_BYTES: usize = 512;

/// The prefix of the digest a receipt carries as its `result`: see
/// [`result_digest`].
pub const RESULT_DOMAIN: &[u8] = b"spine-wire-result-v1\0";

/// Size of the single-use `nonce` of an invocation, in bytes.
pub const NONCE_BYTES: usize = 16;

/// An `AUTH_REQUEST` body that decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthRequest {
    /// The first `MAX_CHAIN_ITEMS + 1` items of `auth.chain`, root first, as
    /// sent. The gate refuses a chain of no items, of more than eight items and
    /// an item over its size bound.
    pub chain: Vec<Vec<u8>>,
    /// How many items `auth.chain` holds, kept or not.
    pub chain_items: usize,
    /// `auth.invocation`, as sent.
    pub invocation: Vec<u8>,
    /// The deterministic encoding of the `command` item, which is what the
    /// invocation's `op` is the SHA-256 of.
    pub command_bytes: Vec<u8>,
    /// The decoded command.
    pub command: BrowserCommand,
}

/// `SHA-256("spine-wire-result-v1" || 0x00 || body)`: the `result` of the
/// receipt for a request whose `RESPONSE` body was `body`.
pub fn result_digest(body: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(RESULT_DOMAIN);
    hasher.update(body);
    hasher.finalize().into()
}

/// The `op` of an invocation for a command: the SHA-256 of the deterministic
/// encoding of the command item (`native::encode_command`), which are the
/// bytes a plain `REQUEST` for it would carry.
pub fn command_op(command_bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(command_bytes).into()
}

/// 16 bytes from the operating system's random source, for an invocation's
/// `nonce`.
pub fn random_nonce() -> [u8; NONCE_BYTES] {
    use rand::RngCore;
    let mut nonce = [0u8; NONCE_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    nonce
}

/// A byte string, written as one.
struct ByteString<'a>(&'a [u8]);

impl Serialize for ByteString<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

#[derive(Serialize)]
struct AuthOut<'a> {
    chain: Vec<ByteString<'a>>,
    invocation: ByteString<'a>,
}

#[derive(Serialize)]
struct AuthRequestOut<'a> {
    auth: AuthOut<'a>,
    command: &'a BrowserCommand,
}

/// The body of an `AUTH_REQUEST`: `{auth: {chain, invocation}, command}`.
/// `chain` and `invocation` are the encodings `spine_authority::Grant::encode`
/// and `Invocation::encode` give, written as they are and not re-encoded.
pub fn encode_auth_request(
    chain: &[Vec<u8>],
    invocation: &[u8],
    command: &BrowserCommand,
) -> Result<Vec<u8>, CborError> {
    cbor::to_vec(&AuthRequestOut {
        auth: AuthOut {
            chain: chain
                .iter()
                .map(|grant| ByteString(grant.as_slice()))
                .collect(),
            invocation: ByteString(invocation),
        },
        command,
    })
}

/// One byte string of the chain.
struct ChainItem(Vec<u8>);

impl<'de> Deserialize<'de> for ChainItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ChainItem, D::Error> {
        wirefmt::bytes::deserialize(deserializer).map(ChainItem)
    }
}

/// The chain as read: the items kept and the number there were.
struct Chain {
    items: Vec<Vec<u8>>,
    count: usize,
}

struct ChainVisitor;

impl<'de> Visitor<'de> for ChainVisitor {
    type Value = Chain;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an array of byte strings")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Chain, A::Error> {
        let mut items: Vec<Vec<u8>> = Vec::new();
        let mut count = 0usize;
        while let Some(ChainItem(bytes)) = seq.next_element()? {
            count += 1;
            if items.len() <= MAX_CHAIN_ITEMS {
                items.push(bytes);
            }
        }
        Ok(Chain { items, count })
    }
}

impl<'de> Deserialize<'de> for Chain {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Chain, D::Error> {
        deserializer.deserialize_seq(ChainVisitor)
    }
}

/// `{chain, invocation}`.
struct Auth {
    chain: Chain,
    invocation: Vec<u8>,
}

struct AuthVisitor;

const AUTH_FIELDS: &[&str] = &["chain", "invocation"];

impl<'de> Visitor<'de> for AuthVisitor {
    type Value = Auth;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a map with the keys chain and invocation")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Auth, A::Error> {
        let mut chain: Option<Chain> = None;
        let mut invocation: Option<Vec<u8>> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "chain" => {
                    if chain.is_some() {
                        return Err(de::Error::duplicate_field("chain"));
                    }
                    chain = Some(map.next_value()?);
                }
                "invocation" => {
                    if invocation.is_some() {
                        return Err(de::Error::duplicate_field("invocation"));
                    }
                    invocation = Some(map.next_value::<ChainItem>()?.0);
                }
                other => return Err(de::Error::unknown_field(other, AUTH_FIELDS)),
            }
        }
        Ok(Auth {
            chain: chain.ok_or_else(|| de::Error::missing_field("chain"))?,
            invocation: invocation.ok_or_else(|| de::Error::missing_field("invocation"))?,
        })
    }
}

impl<'de> Deserialize<'de> for Auth {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Auth, D::Error> {
        deserializer.deserialize_map(AuthVisitor)
    }
}

/// `{auth, command}`, with the command still a generic item.
struct Shell {
    auth: Auth,
    command: ciborium::Value,
}

struct ShellVisitor;

const SHELL_FIELDS: &[&str] = &["auth", "command"];

impl<'de> Visitor<'de> for ShellVisitor {
    type Value = Shell;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a map with the keys auth and command")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Shell, A::Error> {
        let mut auth: Option<Auth> = None;
        let mut command: Option<ciborium::Value> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "auth" => {
                    if auth.is_some() {
                        return Err(de::Error::duplicate_field("auth"));
                    }
                    auth = Some(map.next_value()?);
                }
                "command" => {
                    if command.is_some() {
                        return Err(de::Error::duplicate_field("command"));
                    }
                    command = Some(map.next_value()?);
                }
                other => return Err(de::Error::unknown_field(other, SHELL_FIELDS)),
            }
        }
        Ok(Shell {
            auth: auth.ok_or_else(|| de::Error::missing_field("auth"))?,
            command: command.ok_or_else(|| de::Error::missing_field("command"))?,
        })
    }
}

impl<'de> Deserialize<'de> for Shell {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Shell, D::Error> {
        deserializer.deserialize_map(ShellVisitor)
    }
}

/// The error for a body that is not `{auth, command}`. When its `command`
/// member names a command outside the set the answer is `unknown_command`, as
/// for a plain `REQUEST` (7.4); anything else is `invalid_request`.
fn shape_error(body: &[u8], error: &CborError) -> RequestError {
    if let Ok(ciborium::Value::Map(entries)) = cbor::from_slice::<ciborium::Value>(body) {
        for (key, value) in entries {
            if key == ciborium::Value::Text("command".to_string()) {
                if let Ok(bytes) = cbor::to_vec(&value) {
                    let verdict = classify_request_error(&bytes, error);
                    if verdict.code == crate::body::ERR_UNKNOWN_COMMAND {
                        return verdict;
                    }
                }
            }
        }
    }
    RequestError {
        code: crate::body::ERR_INVALID_REQUEST,
        message: format!(
            "the AUTH_REQUEST body is not {{auth: {{chain, invocation}}, command}}: {error}"
        ),
    }
}

/// Reads an `AUTH_REQUEST` body.
///
/// The failure is the `RESPONSE` error to answer with: `unknown_command` when
/// the `command` member names a command outside the set, `invalid_request` for
/// every other body that is not exactly `{auth: {chain, invocation}, command}`
/// with a chain of byte strings, an invocation that is a byte string and a
/// command that decodes as a `REQUEST` body does. Authority is not consulted
/// for such a body.
pub fn decode_auth_request(body: &[u8]) -> Result<AuthRequest, RequestError> {
    let shell: Shell = match cbor::from_slice(body) {
        Ok(shell) => shell,
        Err(error) => return Err(shape_error(body, &error)),
    };
    let command_bytes = cbor::to_vec(&shell.command).map_err(|error| RequestError {
        code: crate::body::ERR_INVALID_REQUEST,
        message: format!("the command could not be read: {error}"),
    })?;
    let command = decode_command(&command_bytes)?;
    Ok(AuthRequest {
        chain: shell.auth.chain.items,
        chain_items: shell.auth.chain.count,
        invocation: shell.auth.invocation,
        command_bytes,
        command,
    })
}

/// Checks that `body` has the shape of a `RECEIPT` body: at most
/// [`MAX_RECEIPT_BYTES`] bytes, deterministic CBOR, and the closed map
/// `{body: map, sig: 64 bytes}` that `spine_authority::Receipt::encode`
/// writes. What the receipt says, and whether its signature is the node's, is
/// for `spine-authority` to judge.
pub fn check_receipt_body(body: &[u8]) -> Result<(), String> {
    if body.len() > MAX_RECEIPT_BYTES {
        return Err(format!(
            "a RECEIPT body is at most {MAX_RECEIPT_BYTES} bytes, this one has {}",
            body.len()
        ));
    }
    let value = cbor::from_slice::<ciborium::Value>(body).map_err(|e| e.to_string())?;
    let ciborium::Value::Map(entries) = value else {
        return Err("a RECEIPT body is a map".to_string());
    };
    if entries.len() != 2 {
        return Err("a RECEIPT body holds exactly body and sig".to_string());
    }
    let mut body_is_map = false;
    let mut sig_is_64_bytes = false;
    for (key, value) in &entries {
        match (key, value) {
            (ciborium::Value::Text(name), ciborium::Value::Map(_)) if name == "body" => {
                body_is_map = true;
            }
            (ciborium::Value::Text(name), ciborium::Value::Bytes(sig))
                if name == "sig" && sig.len() == 64 =>
            {
                sig_is_64_bytes = true;
            }
            _ => {}
        }
    }
    if body_is_map && sig_is_64_bytes {
        Ok(())
    } else {
        Err("a RECEIPT body is {body: map, sig: 64 bytes}".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::Value;

    fn command() -> BrowserCommand {
        BrowserCommand::Navigate {
            url: "https://example.org/".to_string(),
        }
    }

    fn chain() -> Vec<Vec<u8>> {
        vec![vec![1, 2, 3], vec![4; 40]]
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn bytes_value(items: &[&[u8]]) -> Value {
        Value::Array(items.iter().map(|b| Value::Bytes(b.to_vec())).collect())
    }

    fn body_of(auth: Value, command: Value) -> Vec<u8> {
        cbor::to_vec(&Value::Map(vec![
            (text("auth"), auth),
            (text("command"), command),
        ]))
        .unwrap()
    }

    fn auth_of(chain: Value, invocation: Value) -> Value {
        Value::Map(vec![
            (text("chain"), chain),
            (text("invocation"), invocation),
        ])
    }

    fn command_value(command: &BrowserCommand) -> Value {
        cbor::from_slice(&cbor::to_vec(command).unwrap()).unwrap()
    }

    #[test]
    fn a_body_round_trips_and_keeps_the_command_bytes() {
        let body = encode_auth_request(&chain(), &[9, 9], &command()).unwrap();
        let read = decode_auth_request(&body).unwrap();
        assert_eq!(read.chain, chain());
        assert_eq!(read.chain_items, 2);
        assert_eq!(read.invocation, vec![9, 9]);
        assert_eq!(read.command, command());
        assert_eq!(read.command_bytes, cbor::to_vec(&command()).unwrap());
        // The command item sits in the body exactly as a REQUEST carries it.
        let tail = &body[body.len() - read.command_bytes.len()..];
        assert_eq!(tail, read.command_bytes.as_slice());
    }

    #[test]
    fn the_keys_are_in_deterministic_order() {
        let body = encode_auth_request(&chain(), &[9], &BrowserCommand::GetUR).unwrap();
        // a2 64 "auth" a2 65 "chain" ...
        assert_eq!(&body[..6], &[0xa2, 0x64, b'a', b'u', b't', b'h']);
        assert_eq!(body[6], 0xa2);
        assert_eq!(&body[7..13], &[0x65, b'c', b'h', b'a', b'i', b'n']);
    }

    #[test]
    fn a_chain_is_kept_to_nine_items_and_counted() {
        let many: Vec<Vec<u8>> = (0..40u8).map(|n| vec![n]).collect();
        let body = encode_auth_request(&many, &[1], &BrowserCommand::GetUR).unwrap();
        let read = decode_auth_request(&body).unwrap();
        assert_eq!(read.chain_items, 40);
        assert_eq!(read.chain.len(), MAX_CHAIN_ITEMS + 1);
        assert_eq!(read.chain[..], many[..MAX_CHAIN_ITEMS + 1]);
    }

    #[test]
    fn an_empty_chain_and_an_empty_invocation_decode_for_the_gate_to_refuse() {
        let body = encode_auth_request(&[], &[], &BrowserCommand::GetUR).unwrap();
        let read = decode_auth_request(&body).unwrap();
        assert!(read.chain.is_empty());
        assert_eq!(read.chain_items, 0);
        assert!(read.invocation.is_empty());
    }

    #[test]
    fn a_body_that_is_not_the_closed_map_is_invalid_request() {
        let c = command_value(&command());
        let good_auth = || auth_of(bytes_value(&[&[1]]), Value::Bytes(vec![2]));
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "an array instead of the outer map",
                cbor::to_vec(&Value::Array(vec![good_auth(), c.clone()])).unwrap(),
            ),
            (
                "an array instead of auth",
                body_of(
                    Value::Array(vec![bytes_value(&[&[1]]), Value::Bytes(vec![2])]),
                    c.clone(),
                ),
            ),
            ("auth is null", body_of(Value::Null, c.clone())),
            (
                "chain is a byte string",
                body_of(
                    auth_of(Value::Bytes(vec![1]), Value::Bytes(vec![2])),
                    c.clone(),
                ),
            ),
            (
                "a chain item is text",
                body_of(
                    auth_of(Value::Array(vec![text("x")]), Value::Bytes(vec![2])),
                    c.clone(),
                ),
            ),
            (
                "a chain item is an array of integers",
                body_of(
                    auth_of(
                        Value::Array(vec![Value::Array(vec![Value::Integer(1u8.into())])]),
                        Value::Bytes(vec![2]),
                    ),
                    c.clone(),
                ),
            ),
            (
                "the invocation is text",
                body_of(auth_of(bytes_value(&[&[1]]), text("x")), c.clone()),
            ),
            (
                "the invocation is missing",
                body_of(
                    Value::Map(vec![(text("chain"), bytes_value(&[&[1]]))]),
                    c.clone(),
                ),
            ),
            (
                "the chain is missing",
                body_of(
                    Value::Map(vec![(text("invocation"), Value::Bytes(vec![2]))]),
                    c.clone(),
                ),
            ),
            (
                "an unknown key in auth",
                body_of(
                    Value::Map(vec![
                        (text("chain"), bytes_value(&[&[1]])),
                        (text("extra"), Value::Null),
                        (text("invocation"), Value::Bytes(vec![2])),
                    ]),
                    c.clone(),
                ),
            ),
            (
                "an unknown key in the body",
                cbor::to_vec(&Value::Map(vec![
                    (text("auth"), good_auth()),
                    (text("command"), c.clone()),
                    (text("extra"), Value::Null),
                ]))
                .unwrap(),
            ),
            (
                "the command is missing",
                cbor::to_vec(&Value::Map(vec![(text("auth"), good_auth())])).unwrap(),
            ),
            (
                "auth is missing",
                cbor::to_vec(&Value::Map(vec![(text("command"), c.clone())])).unwrap(),
            ),
            ("the body is null", cbor::to_vec(&Value::Null).unwrap()),
            (
                "the body is a plain REQUEST body",
                cbor::to_vec(&command()).unwrap(),
            ),
            (
                "the command has a bad field",
                body_of(
                    good_auth(),
                    Value::Map(vec![(
                        text("navigate"),
                        Value::Map(vec![(text("url"), Value::Integer(1u8.into()))]),
                    )]),
                ),
            ),
            (
                "the command is not a command",
                body_of(good_auth(), Value::Integer(7u8.into())),
            ),
        ];
        for (what, body) in cases {
            let error = decode_auth_request(&body).unwrap_err();
            assert_eq!(error.code, crate::body::ERR_INVALID_REQUEST, "{what}");
        }
    }

    #[test]
    fn a_command_outside_the_set_is_unknown_command_whatever_else_is_wrong() {
        let unknown = text("no_such_command");
        let good = auth_of(bytes_value(&[&[1]]), Value::Bytes(vec![2]));
        let error = decode_auth_request(&body_of(good, unknown.clone())).unwrap_err();
        assert_eq!(error.code, crate::body::ERR_UNKNOWN_COMMAND);
        let unknown_map = Value::Map(vec![(text("no_such_command"), Value::Null)]);
        let error = decode_auth_request(&body_of(Value::Null, unknown_map)).unwrap_err();
        assert_eq!(error.code, crate::body::ERR_UNKNOWN_COMMAND);
    }

    #[test]
    fn a_body_that_is_not_deterministic_cbor_is_refused() {
        let mut body = encode_auth_request(&chain(), &[9], &BrowserCommand::GetUR).unwrap();
        // a2 -> 9f ... indefinite length is not deterministic CBOR.
        body[0] = 0xbf;
        assert!(decode_auth_request(&body).is_err());
    }

    #[test]
    fn the_result_digest_is_domain_separated() {
        let body = [0xa1, 0x62, b'o', b'k', 0x01];
        let digest = result_digest(&body);
        let mut hasher = Sha256::new();
        hasher.update(b"spine-wire-result-v1\0");
        hasher.update(body);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(digest, expected);
        assert_ne!(digest, <[u8; 32]>::from(Sha256::digest(body)));
        assert_ne!(result_digest(&body), result_digest(&[0xa0]));
    }

    #[test]
    fn nonces_are_not_repeated() {
        let a = random_nonce();
        let b = random_nonce();
        assert_ne!(a, b);
    }

    #[test]
    fn a_receipt_body_has_the_closed_shape() {
        let good = cbor::to_vec(&Value::Map(vec![
            (
                text("body"),
                Value::Map(vec![(text("v"), Value::Integer(1u8.into()))]),
            ),
            (text("sig"), Value::Bytes(vec![0; 64])),
        ]))
        .unwrap();
        assert!(check_receipt_body(&good).is_ok());
        let short_sig = cbor::to_vec(&Value::Map(vec![
            (text("body"), Value::Map(vec![])),
            (text("sig"), Value::Bytes(vec![0; 63])),
        ]))
        .unwrap();
        assert!(check_receipt_body(&short_sig).is_err());
        let extra = cbor::to_vec(&Value::Map(vec![
            (text("body"), Value::Map(vec![])),
            (text("sig"), Value::Bytes(vec![0; 64])),
            (text("x"), Value::Null),
        ]))
        .unwrap();
        assert!(check_receipt_body(&extra).is_err());
        assert!(check_receipt_body(&cbor::to_vec(&Value::Null).unwrap()).is_err());
        let big = cbor::to_vec(&Value::Map(vec![
            (
                text("body"),
                Value::Map(vec![(text("pad"), Value::Bytes(vec![0; 600]))]),
            ),
            (text("sig"), Value::Bytes(vec![0; 64])),
        ]))
        .unwrap();
        assert!(check_receipt_body(&big).is_err());
    }
}
