//! The `RESPONSE` body and its error codes (`docs/spec/00-wire.md`, C.3, C.7
//! and D).
//!
//! A `RESPONSE` body is `{ok: <result>}` or `{err: {code, message[, details]}}`.
//! [`Reply`] is that shape; [`WireError`] is the `err` value. The codes a v1
//! daemon uses are the `ERR_*` constants below, and [`ALL_ERROR_CODES`] lists
//! them: a peer that wants to branch matches on `code`, never on `message`.

use serde::{Deserialize, Serialize};

use crate::wirefmt::reject_null_value;

pub use crate::connection::{ERR_INVALID_REQUEST, ERR_OVERLOADED, ERR_UNKNOWN_COMMAND};

// ---- Connection-level codes (C.7). `ERR_OVERLOADED`, `ERR_UNKNOWN_COMMAND`
// and `ERR_INVALID_REQUEST` are defined next to the connection state machine
// and re-exported above.

/// The daemon failed in a way that is not the caller's doing, or could not
/// send the answer it computed (for example one over the frame limit).
pub const ERR_INTERNAL: &str = "internal";
/// A request or tool call refused by the rate limiter.
pub const ERR_RATE_LIMIT: &str = "rate_limit";
/// No session slot is free (`max_sessions`).
pub const ERR_SESSION_CAPACITY: &str = "session_capacity";
/// The session the command needs does not exist.
pub const ERR_SESSION_NOT_FOUND: &str = "session_not_found";
/// The session disappeared while the command was running.
pub const ERR_SESSION_EXPIRED: &str = "session_expired";
/// The command needs a loaded page and the session has none.
pub const ERR_NO_PAGE: &str = "no_page_loaded";

// ---- Authority (`AUTH_REQUEST`, `docs/spec/00-wire.md` 8.5 and 13).

/// The node is in mode `required` and the request was a plain `REQUEST`.
pub const ERR_AUTHORITY_REQUIRED: &str = "authority_required";
/// The node is in mode `off` and the request was an `AUTH_REQUEST`.
pub const ERR_AUTHORITY_NOT_ENABLED: &str = "authority_not_enabled";
/// The authority gate refused at gate step 1: no grants, more than 8, or a grant or the invocation over its size bound.
pub const ERR_AUTHORITY_STEP_1: &str = "authority_step_1";
/// The authority gate refused at gate step 2: a grant is malformed, has an unknown version or carries an unknown caveat.
pub const ERR_AUTHORITY_STEP_2: &str = "authority_step_2";
/// The authority gate refused at gate step 3: the root's issuer does not own every resource touched or is not a trusted root.
pub const ERR_AUTHORITY_STEP_3: &str = "authority_step_3";
/// The authority gate refused at gate step 4: the chain is not linked.
pub const ERR_AUTHORITY_STEP_4: &str = "authority_step_4";
/// The authority gate refused at gate step 5: a grant signature does not verify.
pub const ERR_AUTHORITY_STEP_5: &str = "authority_step_5";
/// The authority gate refused at gate step 6: a grant is broader than its parent.
pub const ERR_AUTHORITY_STEP_6: &str = "authority_step_6";
/// The authority gate refused at gate step 7: a grant is not yet valid or has expired.
pub const ERR_AUTHORITY_STEP_7: &str = "authority_step_7";
/// The authority gate refused at gate step 8: a grant is revoked, or revocation data is missing or too old.
pub const ERR_AUTHORITY_STEP_8: &str = "authority_step_8";
/// The authority gate refused at gate step 9: the invocation is malformed, for another chain, invoker, recipient or session, stale, badly signed or replayed.
pub const ERR_AUTHORITY_STEP_9: &str = "authority_step_9";
/// The authority gate refused at gate step 10: the grant does not cover the command's resource, action or effect class, or the invocation is for another operation.
pub const ERR_AUTHORITY_STEP_10: &str = "authority_step_10";
/// The authority gate refused at gate step 11: a limit would be exceeded, or usage could not be recorded.
pub const ERR_AUTHORITY_STEP_11: &str = "authority_step_11";

// ---- Navigation and execution.

/// `execute_binary` asked for a capability the origin does not grant.
pub const ERR_CAPABILITY_DENIED: &str = "capability_denied";
/// `execute_binary`: the program failed to compile, instantiate or run.
pub const ERR_EXECUTION_FAILED: &str = "execution_failed";
/// `store_knowledge`: the entry (key, compact JSON of the value, tags) is over
/// 64 KiB.
pub const ERR_KNOWLEDGE_ENTRY_TOO_LARGE: &str = "knowledge_entry_too_large";
/// `store_knowledge` or an `execute_binary` store: the session already holds
/// 1,024 entries, or the daemon-wide knowledge budget is spent.
pub const ERR_KNOWLEDGE_CAPACITY: &str = "knowledge_capacity";
/// `execute_binary`: the run requested an action this daemon does not perform;
/// the program is refused whole.
pub const ERR_ACTION_UNSUPPORTED: &str = "action_unsupported";

// ---- Namespace.

/// The namespace (mesh) is not enabled on this origin.
pub const ERR_NAMESPACE_UNAVAILABLE: &str = "namespace_unavailable";
/// `mesh_call`: `peer_id` is not a UUID.
pub const ERR_INVALID_PEER_ID: &str = "invalid_peer_id";
/// `mesh_call`: the call could not be delivered or the peer refused it.
pub const ERR_MESH_CALL_FAILED: &str = "mesh_call_failed";
/// `resolve_names`: more names than one batch allows.
pub const ERR_BATCH_TOO_LARGE: &str = "batch_too_large";
/// `find_providers`: the local index failed.
pub const ERR_LOOKUP_FAILED: &str = "lookup_failed";
/// `find_providers`: the mesh lookup failed.
pub const ERR_LOOKUP_UNAVAILABLE: &str = "lookup_unavailable";
/// `publish_name` and `publish_resource`: the record is past its expiry.
pub const ERR_RECORD_EXPIRED: &str = "record_expired";
/// `publish_name` and `publish_resource`: the record is not a name record.
pub const ERR_MALFORMED_RECORD: &str = "malformed_record";
/// `publish_name`: the record failed verification or was refused by policy.
pub const ERR_RECORD_REJECTED: &str = "record_rejected";
/// `publish_resource`: the bytes were not stored or the binding was refused.
pub const ERR_PUBLISH_FAILED: &str = "publish_failed";
/// `read_resource`: the hash or offset names nothing this origin holds.
pub const ERR_RESOURCE_NOT_FOUND: &str = "resource_not_found";
/// `delete_resource`: the proof does not verify.
pub const ERR_DELETE_REFUSED: &str = "delete_refused";
/// `crawl_names`: the seed is not a valid name.
pub const ERR_INVALID_SEED: &str = "invalid_seed";

// ---- Tools (`tool_call`).

/// A tool call whose arguments do not fit the tool's schema.
pub const ERR_INVALID_ARGS: &str = "invalid_args";
/// A tool call for a tool this origin does not offer.
pub const ERR_UNSUPPORTED_TOOL: &str = "unsupported_tool";
/// A tool, or `get_ur` on a loaded page, that could not parse its input.
pub const ERR_PARSE_ERROR: &str = "parse_error";

// ---- Outbound policy. `navigate` passes these through unchanged: the fetcher's
// typed error carries its code (`outbound_http_status` adds `details.status`).

pub const ERR_OUTBOUND_DISABLED: &str = "outbound_disabled";
pub const ERR_OUTBOUND_ADDRESS_DENIED: &str = "outbound_address_denied";
pub const ERR_OUTBOUND_SCHEME_DENIED: &str = "outbound_scheme_denied";
pub const ERR_OUTBOUND_CREDENTIALS_DENIED: &str = "outbound_credentials_denied";
pub const ERR_OUTBOUND_FRAGMENT_DENIED: &str = "outbound_fragment_denied";
pub const ERR_OUTBOUND_REDIRECT_DENIED: &str = "outbound_redirect_denied";
pub const ERR_OUTBOUND_ENCODING_DENIED: &str = "outbound_encoding_denied";
pub const ERR_OUTBOUND_RESPONSE_LIMIT: &str = "outbound_response_limit";
pub const ERR_OUTBOUND_BUSY: &str = "outbound_busy";
pub const ERR_OUTBOUND_TIMEOUT: &str = "outbound_timeout";
pub const ERR_OUTBOUND_INVALID_URL: &str = "outbound_invalid_url";
pub const ERR_OUTBOUND_INVALID_HOST: &str = "outbound_invalid_host";
pub const ERR_OUTBOUND_INVALID_PORT: &str = "outbound_invalid_port";
pub const ERR_OUTBOUND_DNS_FAILED: &str = "outbound_dns_failed";
pub const ERR_OUTBOUND_CLIENT_FAILED: &str = "outbound_client_failed";
pub const ERR_OUTBOUND_REQUEST_FAILED: &str = "outbound_request_failed";
pub const ERR_OUTBOUND_BODY_FAILED: &str = "outbound_body_failed";
pub const ERR_OUTBOUND_INVALID_UTF8: &str = "outbound_invalid_utf8";
/// The URL is longer than 8192 bytes.
pub const ERR_OUTBOUND_URL_LIMIT: &str = "outbound_url_limit";
/// Name resolution returned no address or more than 64.
pub const ERR_OUTBOUND_DNS_LIMIT: &str = "outbound_dns_limit";
/// The origin answered with a status that is neither success nor redirect; the
/// status is in `details.status`.
pub const ERR_OUTBOUND_HTTP_STATUS: &str = "outbound_http_status";

/// The codes that mean the connection layer or the decoder refused the
/// request itself, as opposed to a command that ran and failed.
pub const PROTOCOL_ERROR_CODES: &[&str] =
    &[ERR_OVERLOADED, ERR_UNKNOWN_COMMAND, ERR_INVALID_REQUEST];

/// Every error code a v1 daemon produces, once each. Peers must tolerate a
/// code that is not in this list (a later daemon may add one).
pub const ALL_ERROR_CODES: &[&str] = &[
    ERR_OVERLOADED,
    ERR_UNKNOWN_COMMAND,
    ERR_INVALID_REQUEST,
    ERR_INTERNAL,
    ERR_RATE_LIMIT,
    ERR_AUTHORITY_REQUIRED,
    ERR_AUTHORITY_NOT_ENABLED,
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
    ERR_SESSION_CAPACITY,
    ERR_SESSION_NOT_FOUND,
    ERR_SESSION_EXPIRED,
    ERR_NO_PAGE,
    ERR_CAPABILITY_DENIED,
    ERR_EXECUTION_FAILED,
    ERR_KNOWLEDGE_ENTRY_TOO_LARGE,
    ERR_KNOWLEDGE_CAPACITY,
    ERR_ACTION_UNSUPPORTED,
    ERR_NAMESPACE_UNAVAILABLE,
    ERR_INVALID_PEER_ID,
    ERR_MESH_CALL_FAILED,
    ERR_BATCH_TOO_LARGE,
    ERR_LOOKUP_FAILED,
    ERR_LOOKUP_UNAVAILABLE,
    ERR_RECORD_EXPIRED,
    ERR_MALFORMED_RECORD,
    ERR_RECORD_REJECTED,
    ERR_PUBLISH_FAILED,
    ERR_RESOURCE_NOT_FOUND,
    ERR_DELETE_REFUSED,
    ERR_INVALID_SEED,
    ERR_INVALID_ARGS,
    ERR_UNSUPPORTED_TOOL,
    ERR_PARSE_ERROR,
    ERR_OUTBOUND_DISABLED,
    ERR_OUTBOUND_ADDRESS_DENIED,
    ERR_OUTBOUND_SCHEME_DENIED,
    ERR_OUTBOUND_CREDENTIALS_DENIED,
    ERR_OUTBOUND_FRAGMENT_DENIED,
    ERR_OUTBOUND_REDIRECT_DENIED,
    ERR_OUTBOUND_ENCODING_DENIED,
    ERR_OUTBOUND_RESPONSE_LIMIT,
    ERR_OUTBOUND_BUSY,
    ERR_OUTBOUND_TIMEOUT,
    ERR_OUTBOUND_INVALID_URL,
    ERR_OUTBOUND_INVALID_HOST,
    ERR_OUTBOUND_INVALID_PORT,
    ERR_OUTBOUND_DNS_FAILED,
    ERR_OUTBOUND_CLIENT_FAILED,
    ERR_OUTBOUND_REQUEST_FAILED,
    ERR_OUTBOUND_BODY_FAILED,
    ERR_OUTBOUND_INVALID_UTF8,
    ERR_OUTBOUND_URL_LIMIT,
    ERR_OUTBOUND_DNS_LIMIT,
    ERR_OUTBOUND_HTTP_STATUS,
];

/// The `err` of a failed `RESPONSE`: `{code, message[, details]}`. A closed
/// struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireError {
    /// A stable snake_case identifier; see [`ALL_ERROR_CODES`].
    pub code: String,
    /// Human-readable text. Not for matching.
    pub message: String,
    /// Optional structured detail.
    #[serde(
        default,
        deserialize_with = "reject_null_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub details: Option<serde_json::Value>,
}

impl WireError {
    /// An error with no `details`.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> WireError {
        WireError {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// The same error with `details` attached.
    pub fn with_details(mut self, details: serde_json::Value) -> WireError {
        self.details = Some(details);
        self
    }
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WireError {}

/// A `RESPONSE` body: `{ok: T}` or `{err: WireError}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply<T> {
    #[serde(rename = "ok")]
    Ok(T),
    #[serde(rename = "err")]
    Err(WireError),
}

impl<T> Reply<T> {
    /// The reply as a `Result`.
    pub fn into_result(self) -> Result<T, WireError> {
        match self {
            Reply::Ok(value) => Ok(value),
            Reply::Err(error) => Err(error),
        }
    }
}

impl<T> From<Result<T, WireError>> for Reply<T> {
    fn from(result: Result<T, WireError>) -> Reply<T> {
        match result {
            Ok(value) => Reply::Ok(value),
            Err(error) => Reply::Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_are_unique_snake_case() {
        let mut seen = HashSet::new();
        for code in ALL_ERROR_CODES {
            assert!(seen.insert(*code), "duplicate {code}");
            assert!(
                !code.is_empty()
                    && code
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
                "{code}"
            );
        }
        for code in PROTOCOL_ERROR_CODES {
            assert!(ALL_ERROR_CODES.contains(code));
        }
    }

    #[test]
    fn the_list_has_64_codes_and_the_stage_3_and_4_codes_are_in_it() {
        assert_eq!(ALL_ERROR_CODES.len(), 64);
        for code in [
            ERR_KNOWLEDGE_ENTRY_TOO_LARGE,
            ERR_KNOWLEDGE_CAPACITY,
            ERR_ACTION_UNSUPPORTED,
            ERR_DELETE_REFUSED,
        ] {
            assert!(ALL_ERROR_CODES.contains(&code), "{code}");
            assert!(!PROTOCOL_ERROR_CODES.contains(&code), "{code}");
        }
        let authority: Vec<&str> = ALL_ERROR_CODES
            .iter()
            .copied()
            .filter(|code| code.starts_with("authority_"))
            .collect();
        assert_eq!(authority.len(), 13);
        assert_eq!(authority[0], ERR_AUTHORITY_REQUIRED);
        assert_eq!(authority[1], ERR_AUTHORITY_NOT_ENABLED);
        for (index, code) in authority[2..].iter().enumerate() {
            assert_eq!(*code, format!("authority_step_{}", index + 1));
            assert!(!PROTOCOL_ERROR_CODES.contains(code), "{code}");
        }
    }

    #[test]
    fn the_connection_helpers_agree_with_reply() {
        let ok = crate::connection::response_ok_body(&7u32).unwrap();
        assert_eq!(ok, crate::cbor::to_vec(&Reply::Ok(7u32)).unwrap());
        let err = crate::connection::response_err_body("overloaded", "m").unwrap();
        assert_eq!(
            err,
            crate::cbor::to_vec(&Reply::<u32>::Err(WireError::new("overloaded", "m"))).unwrap()
        );
    }
}
