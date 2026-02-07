//! Neural Protocol & Neuromorphic PHY Demo
//! 
//! Demonstrates:
//! - Emulated Neuromorphic PHY layer with spike encoding
//! - Neural compression and adaptive protocol selection
//! - Benchmarking against emulated TCP/IP + TLS

use spine_agentic::{
    NeuralProtocol, ProtocolDomain, ProtocolBenchmark,
    TransmissionResult, BenchmarkReport,
};
use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║         SPINE Neural PROTOCOL & PHY LAYER DEMO          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    demo_neural_transmission().await;
    demo_adaptive_protocols().await;
    demo_protocol_benchmarking().await;

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                ALL NEURAL PROTOCOL DEMOS COMPLETE            ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}

async fn demo_neural_transmission() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 1: Neural Transmission over Neuromorphic PHY          │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let mut protocol = NeuralProtocol::new(1000.0, 2.0); // 1Gbps, 2ms latency
    let data = "SPINE: The future of the agentic web is neural.".as_bytes();
    
    println!("  Original Data: \"{}\"", String::from_utf8_lossy(data));
    println!("  Data Size: {} bytes", data.len());

    let result = protocol.transmit(data, ProtocolDomain::RealTime).await.unwrap();

    println!("\n  Transmission Results:");
    println!("    • Compressed Size: {} bytes", result.compressed_size);
    println!("    • Compression Ratio: {:.2}x", result.compression_ratio);
    println!("    • Spike Count: {} spikes", result.spike_count);
    println!("    • Duration: {:?}", result.duration);
    println!("    • Throughput: {:.2} Mbps", result.throughput_mbps);

    println!("\n  ✓ Neural transmission demo complete\n");
}

async fn demo_adaptive_protocols() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 2: Domain-Specific Adaptive Protocols                 │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let mut protocol = NeuralProtocol::new(1000.0, 5.0);
    let large_data = vec![0u8; 1024 * 1024]; // 1MB
    let small_data = vec![0u8; 1024]; // 1KB

    println!("  Testing adaptive configurations:\n");

    // 1. Bulk Data (High compression)
    let bulk_res = protocol.transmit(&large_data, ProtocolDomain::BulkData).await.unwrap();
    println!("    [BulkData] 1MB Payload:");
    println!("      • Compression Ratio: {:.2}x", bulk_res.compression_ratio);
    println!("      • Throughput: {:.2} Mbps", bulk_res.throughput_mbps);

    // 2. Real-Time (Low latency, low compression)
    let rt_res = protocol.transmit(&small_data, ProtocolDomain::RealTime).await.unwrap();
    println!("\n    [RealTime] 1KB Payload:");
    println!("      • Compression Ratio: {:.2}x", rt_res.compression_ratio);
    println!("      • Duration: {:?}", rt_res.duration);

    // 3. IoT (Balanced)
    let iot_res = protocol.transmit(&small_data, ProtocolDomain::IoT).await.unwrap();
    println!("\n    [IoT] 1KB Payload:");
    println!("      • Compression Ratio: {:.2}x", iot_res.compression_ratio);

    println!("\n  ✓ Adaptive protocols demo complete\n");
}

async fn demo_protocol_benchmarking() {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  DEMO 3: Neural Protocol vs TCP/IP + TLS Benchmark          │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("  Running benchmark (10MB payload)...\n");
    
    let report = ProtocolBenchmark::run_comparison(10 * 1024 * 1024).await;

    println!("  Benchmark Report:");
    println!("    • Payload Size: {} MB", report.data_size / (1024 * 1024));
    println!("    • Neural Duration: {:?}", report.neural_duration);
    println!("    • TCP/TLS Duration: {:?}", report.tcp_duration);
    println!("    • Neural Throughput: {:.2} Mbps", report.neural_throughput);
    println!("    • TCP/TLS Throughput: {:.2} Mbps", report.tcp_throughput);
    
    println!("\n  Performance Gain:");
    println!("    • Improvement Factor: {:.2}x faster", report.improvement_factor);
    println!("    • Latency Reduction: {:.2}%", 
        (1.0 - (report.neural_duration.as_secs_f64() / report.tcp_duration.as_secs_f64())) * 100.0);

    println!("\n  ✓ Benchmarking demo complete\n");
}
