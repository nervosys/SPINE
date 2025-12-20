//! Chameleon Protocol MIRAS Demo
//!
//! This demo showcases the MIRAS-adaptive ChameleonKey:
//! - Latent-space encoding with automatic variant switching
//! - Anomaly-aware protocol morphing
//! - Combined moving-target defense

use hyperlight_protocol::ChameleonKey;
use hyperlight_neural::MirasVariant;

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║        Chameleon Protocol + MIRAS Demo                        ║");
    println!("║   Adaptive Moving-Target Defense with Continual Learning      ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Demo 1: Basic Chameleon Key
    demo_basic_chameleon();
    
    // Demo 2: MIRAS-enhanced Chameleon Key
    demo_miras_chameleon();
    
    // Demo 3: All MIRAS variants
    demo_variant_selection();
    
    // Demo 4: Traffic pattern adaptation
    demo_traffic_adaptation();

    println!("\n✅ All Chameleon Protocol demos completed!");
}

fn demo_basic_chameleon() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 1: Basic Chameleon Key");
    println!("═══════════════════════════════════════════════════════════════\n");

    let secret: [u8; 32] = [42u8; 32];
    let mut key = ChameleonKey::new(&secret);
    
    println!("   Created Chameleon Key with 32-byte secret");
    println!("   • Variant: {}", key.variant());
    println!("   • Initial anomaly level: {:.4}", key.anomaly_level());
    
    // Encode some messages
    let message = b"GET /api/status HTTP/1.1";
    let encoded = key.encode(message);
    
    println!("\n   Encoding test message ({} bytes):", message.len());
    println!("   • Latent vector dimension: {}", encoded.dim_hint);
    println!("   • Epoch: {}", encoded.epoch);
    println!("   • First 5 latent values: {:?}", &encoded.components[..5.min(encoded.components.len())]);
    
    // Evolve the key
    println!("\n   Evolving key (moving-target defense)...");
    key.evolve(0x12345678);
    
    let encoded2 = key.encode(message);
    println!("   • Same message, new encoding: {:?}", &encoded2.components[..5.min(encoded2.components.len())]);
    println!("   • New epoch: {}", encoded2.epoch);
    
    // Check if encodings differ (they should due to evolution)
    let diff: f32 = encoded.components.iter()
        .zip(encoded2.components.iter())
        .map(|(a, b)| (a - b).abs())
        .sum();
    println!("   • Encoding difference (L1): {:.4}", diff);
    println!();
}

fn demo_miras_chameleon() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 2: MIRAS-Enhanced Chameleon Key");
    println!("═══════════════════════════════════════════════════════════════\n");

    let secret: [u8; 32] = [42u8; 32];
    // Create with explicit MIRAS variant
    let mut key = ChameleonKey::new_with_miras(&secret, MirasVariant::Yaad);
    
    println!("   Created MIRAS Chameleon Key (YAAD variant)");
    println!("   • Active variant: {}", key.variant());
    
    // Process multiple messages
    println!("\n   Processing message sequence:");
    let messages = [
        "Request 1: Normal traffic",
        "Request 2: Normal traffic",
        "Request 3: ANOMALY_DETECTED!",
        "Request 4: Back to normal",
        "Request 5: Normal traffic",
    ];
    
    for (i, msg) in messages.iter().enumerate() {
        let encoded = key.encode(msg.as_bytes());
        let anomaly = key.anomaly_level();
        
        let indicator = if msg.contains("ANOMALY") { "⚠️" } else { "  " };
        println!("   {} [{}/{}] Encoded {} dims, anomaly: {:.4}", 
            indicator, i + 1, messages.len(), encoded.components.len(), anomaly);
    }
    
    println!("\n   Final anomaly level: {:.4}", key.anomaly_level());
    println!();
}

fn demo_variant_selection() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 3: MIRAS Variant Selection");
    println!("═══════════════════════════════════════════════════════════════\n");

    let secret: [u8; 32] = [42u8; 32];
    let variants = [
        (MirasVariant::Titans, "🧠 Titans", "Surprise-gated memory writes"),
        (MirasVariant::Yaad, "🛡️ YAAD", "Outlier-robust gradient clipping"),
        (MirasVariant::Moneta { p: 2.0 }, "⏳ MONETA", "Lp-norm stability for long sessions"),
        (MirasVariant::Memora, "⚖️ MEMORA", "Probability-constrained updates"),
    ];

    for (variant, name, desc) in variants {
        let mut key = ChameleonKey::new_with_miras(&secret, variant);
        
        // Process some traffic
        for _ in 0..5 {
            key.encode(b"Test message for variant comparison");
        }
        
        println!("   {} {}", name, desc);
        println!("      • Variant active: {}", key.variant());
        println!("      • Anomaly after 5 messages: {:.4}", key.anomaly_level());
        println!();
    }
}

fn demo_traffic_adaptation() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 4: Traffic Pattern Adaptation");
    println!("═══════════════════════════════════════════════════════════════\n");

    let secret: [u8; 32] = [42u8; 32];
    let mut key = ChameleonKey::new_with_miras(&secret, MirasVariant::Titans);
    
    println!("   Simulating realistic traffic patterns...\n");
    
    // Phase 1: API requests
    println!("   📡 Phase 1: Normal API Traffic");
    for i in 0..10 {
        let msg = format!("GET /api/users/{} HTTP/1.1", i);
        key.encode(msg.as_bytes());
    }
    println!("      Variant: {}, Anomaly: {:.4}", key.variant(), key.anomaly_level());
    
    // Phase 2: Mixed traffic
    println!("\n   🔀 Phase 2: Mixed Traffic");
    for i in 0..10 {
        let msg = if i % 4 == 0 {
            format!("POST /submit {{\"data\": \"value_{}\"}}", i * 7)
        } else if i % 3 == 0 {
            format!("DELETE /api/item/{}", i)
        } else {
            "GET /api/status".to_string()
        };
        key.encode(msg.as_bytes());
    }
    println!("      Variant: {}, Anomaly: {:.4}", key.variant(), key.anomaly_level());
    
    // Phase 3: Attack simulation
    println!("\n   🚨 Phase 3: Attack Pattern Simulation");
    let attacks = [
        "'; DROP TABLE users; --",
        "UNION SELECT * FROM passwords",
        "../../../etc/passwd",
        "<script>alert('xss')</script>",
        "{{constructor.constructor('return this')()}}",
    ];
    for attack in attacks {
        key.encode(attack.as_bytes());
    }
    println!("      Variant: {}, Anomaly: {:.4}", key.variant(), key.anomaly_level());
    
    // Phase 4: Recovery
    println!("\n   🔄 Phase 4: Recovery to Normal Traffic");
    for i in 0..20 {
        let msg = format!("GET /api/normal/{}", i);
        key.encode(msg.as_bytes());
    }
    println!("      Variant: {}, Anomaly: {:.4}", key.variant(), key.anomaly_level());
    
    // Key evolution summary
    println!("\n   📊 Key Evolution Summary:");
    for i in 0..3 {
        key.evolve((i + 1) * 0xDEADBEEF);
        println!("      Evolution {}: New encoding space established", i + 1);
    }
    println!();
}
