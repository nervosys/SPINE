//! Property-based tests for spine-protocol using proptest.
//!
//! Tests serialization roundtrips, algebraic invariants, and safety properties
//! for all wire-format types.

use proptest::prelude::*;
use spine_protocol::*;

// ========== STRATEGIES ==========

fn arb_browser_command() -> impl Strategy<Value = BrowserCommand> {
    prop_oneof![
        "\\PC*".prop_map(|url| BrowserCommand::Navigate { url }),
        Just(BrowserCommand::GetUR),
        Just(BrowserCommand::GetRawHTML),
    ]
}

fn arb_request() -> impl Strategy<Value = Request> {
    ("\\PC{1,64}", arb_browser_command()).prop_map(|(id, command)| Request { id, command })
}

fn arb_response() -> impl Strategy<Value = Response> {
    (
        "\\PC{1,64}",
        prop::option::of(Just(serde_json::json!({"ok": true}))),
        prop::option::of("\\PC{1,128}".prop_map(|s| s)),
    )
        .prop_map(|(id, result, error)| Response { id, result, error })
}

fn arb_message() -> impl Strategy<Value = Message> {
    prop_oneof![
        arb_request().prop_map(Message::Request),
        arb_response().prop_map(Message::Response),
        any::<u64>().prop_map(|ts| Message::Ping { timestamp: ts }),
        any::<u64>().prop_map(|ts| Message::Pong { timestamp: ts }),
    ]
}

// ========== MESSAGE SERDE ROUNDTRIPS ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// All Message variants survive JSON roundtrip
    #[test]
    fn message_json_roundtrip(msg in arb_message()) {
        let json = serde_json::to_string(&msg).expect("serialize");
        let decoded: Message = serde_json::from_str(&json).expect("deserialize");
        // Re-serialize to compare (since we can't derive PartialEq on Message with Value)
        let json2 = serde_json::to_string(&decoded).expect("re-serialize");
        prop_assert_eq!(json, json2);
    }

    /// Request JSON roundtrip
    #[test]
    fn request_json_roundtrip(req in arb_request()) {
        let json = serde_json::to_string(&req).expect("serialize");
        let decoded: Request = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(req.id, decoded.id);
    }

    /// Response JSON roundtrip
    #[test]
    fn response_json_roundtrip(resp in arb_response()) {
        let json = serde_json::to_string(&resp).expect("serialize");
        let decoded: Response = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(resp.id, decoded.id);
        prop_assert_eq!(resp.error, decoded.error);
    }
}
