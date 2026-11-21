//! Property-based tests for `LatentVector`, moved here from
//! `spine-protocol/tests/proptest_protocol.rs` together with the type.

use proptest::prelude::*;
use spine_protocol_research::LatentVector;

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
