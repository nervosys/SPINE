use lazy_static::lazy_static;
use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider, Resource};
use prometheus::{
    register_counter, register_histogram, register_int_gauge, Counter, Histogram, HistogramOpts,
    IntGauge, Opts, Registry as PromRegistry,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

lazy_static! {
    pub static ref PROM_REGISTRY: PromRegistry = PromRegistry::new();
    pub static ref SESSIONS_ACTIVE: IntGauge = register_int_gauge!(Opts::new(
        "spine_sessions_active",
        "Number of active browser sessions"
    ))
    .unwrap();
    pub static ref COMMANDS_TOTAL: Counter = register_counter!(Opts::new(
        "spine_commands_total",
        "Total number of commands processed"
    ))
    .unwrap();
    pub static ref COMMAND_LATENCY: Histogram = register_histogram!(HistogramOpts::new(
        "spine_command_latency_seconds",
        "Latency of command processing"
    )
    .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0]))
    .unwrap();
    pub static ref PROTOCOL_MORPHS: Counter = register_counter!(Opts::new(
        "spine_protocol_morphs_total",
        "Total number of protocol morphing events"
    ))
    .unwrap();
}

pub fn init_telemetry(service_name: &str) -> anyhow::Result<()> {
    // Configure OpenTelemetry
    global::set_text_map_propagator(TraceContextPropagator::new());

    // OpenTelemetry 0.31 replaced the `new_pipeline().install_batch()` builder
    // with an explicit exporter and provider. The provider has to be handed to
    // `global` as well as used for the tracer: dropping it would shut the batch
    // processor down and silently stop exporting.
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint("http://localhost:4317")
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder()
                .with_service_name(service_name.to_string())
                .build(),
        )
        .build();

    let tracer = provider.tracer("spine-core");
    global::set_tracer_provider(provider);

    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let filter = EnvFilter::from_default_env().add_directive("spine_core=debug".parse()?);

    Registry::default()
        .with(filter)
        .with(telemetry)
        .with(tracing_subscriber::fmt::layer())
        .init();

    Ok(())
}
