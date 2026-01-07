//! MIRAS-Adaptive Prediction Demo
//!
//! This demo showcases the MIRAS framework integration with Titans prediction:
//! - Automatic variant switching based on traffic patterns
//! - Combined surprise detection from Titans + MIRAS encoders
//! - Adaptive memory strategies for different traffic conditions

use spine_crypto::{MirasTitansPredictor, TitansConfig};
use spine_neural::MirasVariant;

fn main() {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║          MIRAS-Adaptive Prediction Demo                       ║");
    println!("║  Titans + MIRAS for Continual Learning in Protocol Security   ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Create a MIRAS-enhanced predictor
    let config = TitansConfig {
        embed_dim: 64,
        num_heads: 4,
        num_layers: 2,
        ff_dim: 128,
        max_seq_len: 128,
        memory_size: 32,
        seed: 42,
    };

    println!("📊 Configuration:");
    println!("   • Embed dimension: {}", config.embed_dim);
    println!("   • Attention heads: {}", config.num_heads);
    println!("   • Memory tokens: {}", config.memory_size);
    println!();

    // Demo 1: Basic MIRAS predictor
    demo_basic_predictor(config.clone());
    
    // Demo 2: All MIRAS variants
    demo_all_variants(config.clone());
    
    // Demo 3: Adaptive variant switching
    demo_adaptive_switching(config.clone());
    
    // Demo 4: Anomaly detection
    demo_anomaly_detection(config.clone());
    
    // Demo 5: Combined surprise tracking
    demo_combined_surprise(config);

    println!("\n✅ All demos completed successfully!");
}

fn demo_basic_predictor(config: TitansConfig) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 1: Basic MIRAS Predictor");
    println!("═══════════════════════════════════════════════════════════════\n");

    let mut predictor = MirasTitansPredictor::new(config);
    
    println!("Initial state:");
    println!("   • Variant: {}", predictor.variant());
    println!("   • Anomaly level: {:.4}", predictor.anomaly_level());
    
    // Train on HTTP-like patterns
    let messages = [
        b"GET /api/users HTTP/1.1\r\n".as_slice(),
        b"Host: example.com\r\n",
        b"Accept: application/json\r\n",
        b"\r\n",
        b"HTTP/1.1 200 OK\r\n",
        b"Content-Type: application/json\r\n",
    ];
    
    println!("\n   Training on HTTP message patterns...");
    for (i, msg) in messages.iter().enumerate() {
        predictor.observe(msg);
        let stats = predictor.stats();
        println!("   [{}/{}] Observed {} bytes, surprise: {:.4}", 
            i + 1, messages.len(), msg.len(), stats.titans_surprise);
    }
    
    // Make prediction
    let (next_byte, confidence) = predictor.predict_next();
    println!("\n   Prediction: next byte = {} ('{}')", next_byte, 
        if next_byte.is_ascii_graphic() || next_byte == b' ' { next_byte as char } else { '?' });
    println!("   Confidence: {:.2}%", confidence * 100.0);
    
    let stats = predictor.stats();
    println!("\n   Final stats:");
    println!("   • Messages processed: {}", stats.message_count);
    println!("   • MIRAS-enhanced: {}", stats.miras_enhanced_predictions);
    println!("   • Current variant: {}", stats.current_variant);
    println!();
}

fn demo_all_variants(config: TitansConfig) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 2: All MIRAS Variants");
    println!("═══════════════════════════════════════════════════════════════\n");

    let variants = [
        (MirasVariant::Titans, "Baseline with surprise-gated writes"),
        (MirasVariant::Yaad, "Outlier-robust gradient clipping"),
        (MirasVariant::Moneta { p: 2.0 }, "Lp-norm stability (L2)"),
        (MirasVariant::Memora, "Probability-constrained updates"),
    ];

    for (variant, description) in variants {
        let mut predictor = MirasTitansPredictor::new_with_variant(config.clone(), variant);
        
        // Train briefly
        for _ in 0..3 {
            predictor.observe(b"test message pattern");
        }
        
        let stats = predictor.stats();
        println!("   {} {}", 
            match predictor.variant() {
                "titans" => "🧠",
                "yaad" => "🛡️",
                "moneta" => "⏳",
                "memora" => "⚖️",
                _ => "❓"
            },
            predictor.variant().to_uppercase()
        );
        println!("      {}", description);
        println!("      Surprise after training: {:.4}", stats.titans_surprise);
        println!();
    }
}

fn demo_adaptive_switching(config: TitansConfig) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 3: Adaptive Variant Switching");
    println!("═══════════════════════════════════════════════════════════════\n");

    let mut predictor = MirasTitansPredictor::new(config);
    predictor.set_anomaly_threshold(0.3); // Lower threshold for demo
    
    println!("   Threshold set to 0.3 for demonstration");
    println!("   Starting variant: {}\n", predictor.variant());
    
    // Phase 1: Normal traffic (should stay Titans)
    println!("   Phase 1: Normal HTTP traffic");
    for i in 0..5 {
        predictor.observe(b"GET /api/status HTTP/1.1\r\n");
        if i == 4 {
            println!("   → After {} messages: variant = {}, anomaly = {:.4}", 
                i + 1, predictor.variant(), predictor.anomaly_level());
        }
    }
    
    // Phase 2: Introduce noise (may trigger YAAD or MEMORA)
    println!("\n   Phase 2: Introducing anomalous patterns");
    for i in 0..10 {
        // Mix of normal and weird patterns
        let msg = if i % 3 == 0 {
            format!("WEIRD_PATTERN_{}_!@#$%^", i * 12345)
        } else {
            "GET /api/normal\r\n".to_string()
        };
        predictor.observe(msg.as_bytes());
    }
    println!("   → After mixed traffic: variant = {}, anomaly = {:.4}", 
        predictor.variant(), predictor.anomaly_level());
    
    println!();
}

fn demo_anomaly_detection(config: TitansConfig) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 4: Anomaly Detection");
    println!("═══════════════════════════════════════════════════════════════\n");

    let mut predictor = MirasTitansPredictor::new(config);
    
    // Establish baseline
    println!("   Establishing baseline with normal patterns...");
    for _ in 0..20 {
        predictor.observe(b"SELECT * FROM users WHERE id = 1;");
    }
    let baseline_surprise = predictor.get_surprise();
    println!("   Baseline Titans surprise: {:.4}", baseline_surprise);
    
    // Introduce SQL injection attempt
    println!("\n   Introducing anomalous pattern (SQL injection)...");
    predictor.observe(b"SELECT * FROM users WHERE id = 1; DROP TABLE users;--");
    
    let anomaly_surprise = predictor.get_surprise();
    let combined = predictor.get_combined_surprise();
    let miras = predictor.get_miras_surprise().unwrap_or(0.0);
    
    println!("\n   🚨 Anomaly Detection Results:");
    println!("   • Titans surprise: {:.4}", anomaly_surprise);
    println!("   • MIRAS surprise: {:.4}", miras);
    println!("   • Combined surprise: {:.4}", combined);
    
    let threshold = 0.5;
    println!("\n   Is anomalous (threshold={})? {}", 
        threshold,
        if predictor.is_anomalous(threshold) { "⚠️ YES" } else { "✅ NO" }
    );
    println!();
}

fn demo_combined_surprise(config: TitansConfig) {
    println!("═══════════════════════════════════════════════════════════════");
    println!("Demo 5: Combined Surprise Tracking");
    println!("═══════════════════════════════════════════════════════════════\n");

    let mut predictor = MirasTitansPredictor::new(config);
    
    println!("   Tracking dual surprise (Titans + MIRAS) over time:\n");
    println!("   {:>4} │ {:>12} │ {:>12} │ {:>12}", 
        "Msg#", "Titans", "MIRAS", "Combined");
    println!("   ─────┼──────────────┼──────────────┼──────────────");
    
    let messages = [
        "Hello World",
        "Hello World",
        "Hello World",
        "UNEXPECTED_PATTERN!",
        "Hello World",
        "Another normal message",
        "!@#$%^&*()",
        "Hello World",
        "Back to normal",
        "Final message",
    ];
    
    for (i, msg) in messages.iter().enumerate() {
        predictor.observe(msg.as_bytes());
        
        let titans = predictor.get_surprise();
        let miras = predictor.get_miras_surprise().unwrap_or(0.0);
        let combined = predictor.get_combined_surprise();
        
        let indicator = if combined > 0.5 { "⚠️" } else { "  " };
        println!("   {:>4} │ {:>12.4} │ {:>12.4} │ {:>12.4} {}", 
            i + 1, titans, miras, combined, indicator);
    }
    
    println!("\n   Final Statistics:");
    let stats = predictor.stats();
    println!("   • Total messages: {}", stats.message_count);
    println!("   • MIRAS-enhanced: {}", stats.miras_enhanced_predictions);
    println!("   • Final anomaly level: {:.4}", stats.anomaly_level);
    println!("   • Active variant: {}", stats.current_variant);
    println!();
}
