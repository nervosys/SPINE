use opentelemetry::global;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};
use prometheus::{Registry as PromRegistry, Counter, Histogram, HistogramOpts, Opts, register_counter, register_histogram};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref PROM_REGISTRY: PromRegistry = PromRegistry::new();
    
    pub static ref SESSIONS_ACTIVE: Counter = register_counter!(
        Opts::new("spine_sessions_active", "Number of active browser sessions")
    ).unwrap();
    
    pub static ref COMMANDS_TOTAL: Counter = register_counter!(
        Opts::new("spine_commands_total", "Total number of commands processed")
    ).unwrap();
    
    pub static ref COMMAND_LATENCY: Histogram = register_histogram!(
        HistogramOpts::new("spine_command_latency_seconds", "Latency of command processing")
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0])
    ).unwrap();
    
    pub static ref PROTOCOL_MORPHS: Counter = register_counter!(
        Opts::new("spine_protocol_morphs_total", "Total number of protocol morphing events")
    ).unwrap();
}

pub fn init_telemetry(service_name: &str) -> anyhow::Result<()> {
    // Configure OpenTelemetry
    global::set_text_map_propagator(TraceContextPropagator::new());

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://localhost:4317"),
        )
        .with_trace_config(
            opentelemetry_sdk::trace::config().with_resource(
                opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
                    "service.name",
                    service_name.to_string(),
                )]),
            ),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let filter = EnvFilter::from_default_env()
        .add_directive("spine_core=debug".parse()?);

    Registry::default()
        .with(filter)
        .with(telemetry)
        .with(tracing_subscriber::fmt::layer())
        .init();

    Ok(())
}
