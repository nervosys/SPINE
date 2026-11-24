//! Property tests for `ChameleonKey`. They used to sit in
//! `spine-protocol/tests/proptest_protocol.rs` behind the `research` feature.

use proptest::prelude::*;
use spine_protocol_research::ChameleonKey;

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
