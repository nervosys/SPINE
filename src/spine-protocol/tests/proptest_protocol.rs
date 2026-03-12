//! Property-based tests for spine-protocol using proptest.
//!
//! Tests serialization roundtrips, algebraic invariants, and safety properties
//! for all wire-format types.

use proptest::prelude::*;
use spine_protocol::*;

// ========== STRATEGIES ==========

fn arb_latent_vector() -> impl Strategy<Value = LatentVector> {
    (
        prop::collection::vec(-1e6_f32..1e6, 0..512),
        any::<u16>(),
        any::<u64>(),
    )
        .prop_map(|(components, dim_hint, epoch)| LatentVector {
            components,
            dim_hint,
            epoch,
        })
}

fn arb_browser_command() -> impl Strategy<Value = BrowserCommand> {
    prop_oneof![
        "\\PC*".prop_map(|url| BrowserCommand::Navigate { url }),
        Just(BrowserCommand::GetUR),
        "\\PC*".prop_map(|id| BrowserCommand::Click { element_id: id }),
        ("\\PC*", "\\PC*").prop_map(|(id, text)| BrowserCommand::Type {
            element_id: id,
            text,
        }),
        "\\PC*".prop_map(|q| BrowserCommand::Search { query: q }),
        Just(BrowserCommand::GetRawHTML),
        Just(BrowserCommand::Morph),
        (1usize..512).prop_map(|d| BrowserCommand::GetLatentUR { dimensions: d }),
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
        arb_latent_vector().prop_map(Message::LatentMessage),
        any::<u64>().prop_map(|ts| Message::Ping { timestamp: ts }),
        any::<u64>().prop_map(|ts| Message::Pong { timestamp: ts }),
        any::<u64>().prop_map(|seed| Message::MorphRequest { seed }),
    ]
}

// ========== LATENT VECTOR ROUNDTRIPS ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// LatentVector binary roundtrip: to_bytes_fast / from_bytes_fast
    #[test]
    fn latent_vector_bytes_roundtrip(
        components in prop::collection::vec(-1e6_f32..1e6, 0..512),
        dim_hint in any::<u16>(),
        epoch in any::<u64>(),
    ) {
        let original = LatentVector { components, dim_hint, epoch };
        let bytes = original.to_bytes_fast();
        let decoded = LatentVector::from_bytes_fast(&bytes).expect("decode must succeed");
        prop_assert_eq!(original.dim_hint, decoded.dim_hint);
        prop_assert_eq!(original.epoch, decoded.epoch);
        prop_assert_eq!(original.components.len(), decoded.components.len());
        for (a, b) in original.components.iter().zip(decoded.components.iter()) {
            prop_assert!((a - b).abs() < f32::EPSILON, "component mismatch: {} vs {}", a, b);
        }
    }

    /// LatentVector binary roundtrip: to_bytes_into (zero-alloc variant)
    #[test]
    fn latent_vector_bytes_into_roundtrip(
        components in prop::collection::vec(-1e6_f32..1e6, 0..256),
        dim_hint in any::<u16>(),
        epoch in any::<u64>(),
    ) {
        let original = LatentVector { components, dim_hint, epoch };
        let mut buf = Vec::new();
        original.to_bytes_into(&mut buf);
        let decoded = LatentVector::from_bytes_fast(&buf).expect("decode must succeed");
        prop_assert_eq!(original.dim_hint, decoded.dim_hint);
        prop_assert_eq!(original.epoch, decoded.epoch);
        prop_assert_eq!(original.components.len(), decoded.components.len());
    }

    /// LatentVector bincode roundtrip
    #[test]
    fn latent_vector_bincode_roundtrip(v in arb_latent_vector()) {
        let encoded = v.to_bincode().expect("bincode encode must succeed");
        let decoded = LatentVector::from_bincode(&encoded).expect("bincode decode must succeed");
        prop_assert_eq!(v.dim_hint, decoded.dim_hint);
        prop_assert_eq!(v.epoch, decoded.epoch);
        prop_assert_eq!(v.components.len(), decoded.components.len());
    }

    /// LatentVector binary size is deterministic: 10 + 4*len
    #[test]
    fn latent_vector_size_deterministic(
        components in prop::collection::vec(-1e6_f32..1e6, 0..256),
        dim_hint in any::<u16>(),
        epoch in any::<u64>(),
    ) {
        let v = LatentVector { components, dim_hint, epoch };
        let expected_size = 2 + 8 + 4 * v.components.len(); // u16 + u64 + f32*N
        let bytes = v.to_bytes_fast();
        prop_assert_eq!(bytes.len(), expected_size);
    }
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

// ========== CHAMELEON KEY PROPERTIES ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// ChameleonKey encode produces non-empty latent vector
    #[test]
    fn chameleon_encode_produces_output(
        secret in prop::array::uniform32(any::<u8>()),
        data in prop::collection::vec(any::<u8>(), 1..256),
    ) {
        let mut key = ChameleonKey::new(&secret);
        let vector = key.encode(&data);
        // Neural encoder produces non-empty latent vector
        prop_assert!(!vector.components.is_empty(), "encoded vector must not be empty");
        prop_assert_eq!(vector.epoch, 0);
    }

    /// ChameleonKey evolution changes the epoch monotonically
    #[test]
    fn chameleon_epoch_monotonic(
        secret in prop::array::uniform32(any::<u8>()),
        hash in any::<u64>(),
    ) {
        let mut key = ChameleonKey::new(&secret);
        let v1 = key.encode(b"test");
        prop_assert_eq!(v1.epoch, 0);
        key.evolve(hash);
        let v2 = key.encode(b"test");
        prop_assert_eq!(v2.epoch, 1);
    }
}

// ProtocolMorphology::new() is private - morphology is tested indirectly
// via ProtocolHandler::morph_now() in integration.rs
