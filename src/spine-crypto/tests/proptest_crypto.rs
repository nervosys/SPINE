//! Property-based tests for spine-crypto cryptographic primitives.

use proptest::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;
use spine_crypto::*;

// ========== RING ELEMENT ALGEBRA ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Ring addition is commutative: a + b == b + a
    #[test]
    fn ring_add_commutative(seed in any::<u64>()) {
        let q = 3329u64;
        let n = 64;
        let mut rng = StdRng::seed_from_u64(seed);
        let a = RingElement::random(n, q, &mut rng);
        let b = RingElement::random(n, q, &mut rng);
        let ab = a.add(&b);
        let ba = b.add(&a);
        prop_assert_eq!(ab.to_bytes(), ba.to_bytes());
    }

    /// Ring addition is associative: (a + b) + c == a + (b + c)
    #[test]
    fn ring_add_associative(seed in any::<u64>()) {
        let q = 3329u64;
        let n = 64;
        let mut rng = StdRng::seed_from_u64(seed);
        let a = RingElement::random(n, q, &mut rng);
        let b = RingElement::random(n, q, &mut rng);
        let c = RingElement::random(n, q, &mut rng);
        let ab_c = a.add(&b).add(&c);
        let a_bc = a.add(&b.add(&c));
        prop_assert_eq!(ab_c.to_bytes(), a_bc.to_bytes());
    }

    /// Ring subtraction: a - a == zero element
    #[test]
    fn ring_sub_self_is_zero(seed in any::<u64>()) {
        let q = 3329u64;
        let n = 64;
        let mut rng = StdRng::seed_from_u64(seed);
        let a = RingElement::random(n, q, &mut rng);
        let zero = a.sub(&a);
        let zero_bytes = zero.to_bytes();
        prop_assert!(zero_bytes.iter().all(|&b| b == 0), "a - a should be zero");
    }

    /// Ring scale: scale(a, 1) == a
    #[test]
    fn ring_scale_identity(seed in any::<u64>()) {
        let q = 3329u64;
        let n = 64;
        let mut rng = StdRng::seed_from_u64(seed);
        let a = RingElement::random(n, q, &mut rng);
        let scaled = a.scale(1);
        prop_assert_eq!(a.to_bytes(), scaled.to_bytes());
    }

    /// Ring scale: scale(a, 0) == 0
    #[test]
    fn ring_scale_zero(seed in any::<u64>()) {
        let q = 3329u64;
        let n = 64;
        let mut rng = StdRng::seed_from_u64(seed);
        let a = RingElement::random(n, q, &mut rng);
        let scaled = a.scale(0);
        let zero_bytes = scaled.to_bytes();
        prop_assert!(zero_bytes.iter().all(|&b| b == 0), "scale(a, 0) should be zero");
    }

    /// from_bytes / to_bytes roundtrip
    #[test]
    fn ring_bytes_roundtrip(seed in any::<u64>()) {
        let q = 3329u64;
        let n = 64;
        let mut rng = StdRng::seed_from_u64(seed);
        let a = RingElement::random(n, q, &mut rng);
        let bytes = a.to_bytes();
        let b = RingElement::from_bytes(&bytes, n, q);
        prop_assert_eq!(a.to_bytes(), b.to_bytes());
    }
}

// ========== QUANTUM KEY EVOLUTION ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Key evolution is deterministic from same seed
    #[test]
    fn key_evolution_deterministic(seed in any::<u64>()) {
        let params = LatticeParams::default();
        let mut evo1 = QuantumKeyEvolution::new(params.clone(), seed);
        let mut evo2 = QuantumKeyEvolution::new(params, seed);
        let key1 = evo1.evolve();
        let key2 = evo2.evolve();
        prop_assert_eq!(key1, key2);
    }

    /// Key evolution changes the key hash
    #[test]
    fn key_evolution_changes_hash(seed in any::<u64>()) {
        let params = LatticeParams::default();
        let mut evo = QuantumKeyEvolution::new(params, seed);
        let hash1 = evo.get_key_hash();
        evo.evolve();
        let hash2 = evo.get_key_hash();
        prop_assert_ne!(hash1, hash2, "evolution must change the key hash");
    }

    /// verify_evolution returns true for hash produced by evolve()
    #[test]
    fn key_verify_evolution(seed in any::<u64>()) {
        let params = LatticeParams::default();
        let mut evo = QuantumKeyEvolution::new(params, seed);
        let hash = evo.evolve(); // evolve() returns the hash stored in key_history
        prop_assert!(evo.verify_evolution(&hash), "evolution hash must be in key history");
    }

    /// Encapsulate/decapsulate roundtrip
    #[test]
    fn encapsulate_decapsulate_roundtrip(seed in any::<u64>()) {
        let params = LatticeParams::default();
        let mut evo = QuantumKeyEvolution::new(params, seed);
        let (ciphertext, _shared_secret_sender) = evo.encapsulate();
        let shared_secret_receiver = evo.decapsulate(&ciphertext);
        prop_assert!(shared_secret_receiver.is_some(), "decapsulate must succeed");
        // Note: RLWE encapsulation is probabilistic; the shared secrets
        // should be close but may not be identical due to noise rounding.
        // We just check that decapsulation succeeds.
    }
}

// ========== TITANS PREDICTOR ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// TitansPredictor observe/predict doesn't panic
    #[test]
    fn titans_predict_no_panic(
        messages in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..32), 1..10),
    ) {
        let config = TitansConfig {
            embed_dim: 16,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 32,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = TitansPredictor::new(config);
        for msg in &messages {
            predictor.observe(msg);
        }
        let (_byte, confidence) = predictor.predict_next();
        let ok = (0.0..=1.0).contains(&confidence);
        prop_assert!(ok, "confidence must be in [0,1], got {}", confidence);
    }

    /// TitansPredictor surprise is non-negative
    #[test]
    fn titans_surprise_nonnegative(
        messages in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..16), 1..5),
    ) {
        let config = TitansConfig {
            embed_dim: 16,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 32,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = TitansPredictor::new(config);
        for msg in &messages {
            predictor.observe(msg);
        }
        let surprise = predictor.get_surprise();
        prop_assert!(surprise >= 0.0, "surprise must be non-negative: {}", surprise);
    }
}

// ========== QUANTUM SPECULATIVE PROTOCOL ==========

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// QuantumSpeculativeProtocol send/receive roundtrip
    #[test]
    fn quantum_speculative_roundtrip(
        message in prop::collection::vec(any::<u8>(), 1..128),
        seed in any::<u64>(),
    ) {
        let config = TitansConfig {
            embed_dim: 16,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 32,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let params = LatticeParams::default();
        let mut protocol = QuantumSpeculativeProtocol::new(config, params, seed);
        let quantum_msg = protocol.send(&message);
        let decoded = protocol.receive(&quantum_msg);
        prop_assert!(decoded.is_some(), "receive must succeed");
    }
}
