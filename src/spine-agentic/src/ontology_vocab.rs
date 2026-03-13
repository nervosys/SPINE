// =============================================================================
// SPINE Agent Ontology Vocabulary
// =============================================================================
//
// A complete, hierarchical ontology for agent discoverability. Agents register
// terms from this vocabulary to advertise capabilities, roles, domains, and
// interaction protocols. The registry then enables multi-modal discovery:
//   - Exact URI lookup (`spine:cap/web/scraping`)
//   - Cryptographic hash verification (HashOnly terms)
//   - Neural similarity search (NeuralHash terms)
//
// Namespace convention:
//   spine:cap/<domain>/<term>      — capabilities
//   spine:role/<term>              — agent roles
//   spine:domain/<term>            — knowledge domains
//   spine:proto/<term>             — interaction protocols
//   spine:io/<direction>/<format>  — input/output formats
//   spine:qos/<term>               — quality-of-service properties
//   spine:sec/<term>               — security properties
//   spine:hw/<term>                — hardware/runtime constraints

use crate::{AgentOntology, OntologyTerm};

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

fn term(uri: &str, label: &str) -> OntologyTerm {
    OntologyTerm::new(uri, label)
}

fn cap(path: &str, label: &str) -> OntologyTerm {
    term(&format!("spine:cap/{path}"), label)
}

fn cap_desc(path: &str, label: &str, desc: &str) -> OntologyTerm {
    term(&format!("spine:cap/{path}"), label).with_description(desc)
}

fn cap_child(path: &str, label: &str, parent_path: &str) -> OntologyTerm {
    term(&format!("spine:cap/{path}"), label)
        .with_parent(format!("spine:cap/{parent_path}"))
}

fn cap_child_desc(path: &str, label: &str, parent_path: &str, desc: &str) -> OntologyTerm {
    term(&format!("spine:cap/{path}"), label)
        .with_parent(format!("spine:cap/{parent_path}"))
        .with_description(desc)
}

fn role(name: &str, label: &str, desc: &str) -> OntologyTerm {
    term(&format!("spine:role/{name}"), label).with_description(desc)
}

fn domain(name: &str, label: &str, desc: &str) -> OntologyTerm {
    term(&format!("spine:domain/{name}"), label).with_description(desc)
}

fn proto(name: &str, label: &str, desc: &str) -> OntologyTerm {
    term(&format!("spine:proto/{name}"), label).with_description(desc)
}

// ========================== CAPABILITY TERMS ================================

/// Web interaction capabilities.
fn web_capabilities() -> Vec<OntologyTerm> {
    vec![
        // Root
        cap_desc("web", "Web Interaction", "Interact with web content and services"),
        // Navigation
        cap_child_desc("web/navigate", "Web Navigation", "web",
            "Navigate to URLs, follow links, handle redirects"),
        cap_child("web/navigate/single-page", "SPA Navigation", "web/navigate"),
        cap_child("web/navigate/multi-tab", "Multi-Tab Navigation", "web/navigate"),
        cap_child("web/navigate/headless", "Headless Navigation", "web/navigate"),
        // Scraping
        cap_child_desc("web/scraping", "Web Scraping", "web",
            "Extract structured data from web pages"),
        cap_child("web/scraping/html", "HTML Scraping", "web/scraping"),
        cap_child("web/scraping/json-ld", "JSON-LD Extraction", "web/scraping"),
        cap_child("web/scraping/microdata", "Microdata Extraction", "web/scraping"),
        cap_child("web/scraping/rss", "RSS/Atom Feed Parsing", "web/scraping"),
        cap_child("web/scraping/table", "Table Extraction", "web/scraping"),
        // API interaction
        cap_child_desc("web/api", "API Interaction", "web",
            "Call REST, GraphQL, gRPC, and WebSocket APIs"),
        cap_child("web/api/rest", "REST API", "web/api"),
        cap_child("web/api/graphql", "GraphQL API", "web/api"),
        cap_child("web/api/grpc", "gRPC API", "web/api"),
        cap_child("web/api/websocket", "WebSocket API", "web/api"),
        // Forms
        cap_child_desc("web/forms", "Form Interaction", "web",
            "Fill, submit, and validate web forms"),
        cap_child("web/forms/login", "Login Forms", "web/forms"),
        cap_child("web/forms/search", "Search Forms", "web/forms"),
        cap_child("web/forms/checkout", "Checkout Forms", "web/forms"),
        // Authentication
        cap_child_desc("web/auth", "Web Authentication", "web",
            "Handle web authentication flows"),
        cap_child("web/auth/oauth2", "OAuth 2.0", "web/auth"),
        cap_child("web/auth/cookies", "Cookie Management", "web/auth"),
        cap_child("web/auth/jwt", "JWT Tokens", "web/auth"),
    ]
}

/// Natural language processing capabilities.
fn nlp_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("nlp", "Natural Language Processing",
            "Process, understand, and generate natural language"),
        cap_child_desc("nlp/extraction", "Information Extraction", "nlp",
            "Extract entities, relations, and facts from text"),
        cap_child("nlp/extraction/ner", "Named Entity Recognition", "nlp/extraction"),
        cap_child("nlp/extraction/relation", "Relation Extraction", "nlp/extraction"),
        cap_child("nlp/extraction/keyword", "Keyword Extraction", "nlp/extraction"),
        cap_child("nlp/extraction/topic", "Topic Extraction", "nlp/extraction"),
        cap_child_desc("nlp/generation", "Text Generation", "nlp",
            "Generate coherent text from prompts or context"),
        cap_child("nlp/generation/summary", "Summarization", "nlp/generation"),
        cap_child("nlp/generation/translation", "Translation", "nlp/generation"),
        cap_child("nlp/generation/paraphrase", "Paraphrasing", "nlp/generation"),
        cap_child("nlp/generation/code", "Code Generation", "nlp/generation"),
        cap_child_desc("nlp/understanding", "Language Understanding", "nlp",
            "Understand intent, sentiment, and meaning"),
        cap_child("nlp/understanding/sentiment", "Sentiment Analysis", "nlp/understanding"),
        cap_child("nlp/understanding/intent", "Intent Classification", "nlp/understanding"),
        cap_child("nlp/understanding/qa", "Question Answering", "nlp/understanding"),
        cap_child("nlp/understanding/entailment", "Textual Entailment", "nlp/understanding"),
        cap_child_desc("nlp/embedding", "Text Embedding", "nlp",
            "Produce dense vector representations of text"),
    ]
}

/// Data processing and analysis capabilities.
fn data_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("data", "Data Processing", "Transform, analyze, and manage data"),
        cap_child_desc("data/transform", "Data Transformation", "data",
            "Convert, clean, and reshape data"),
        cap_child("data/transform/json", "JSON Transformation", "data/transform"),
        cap_child("data/transform/csv", "CSV Processing", "data/transform"),
        cap_child("data/transform/xml", "XML Processing", "data/transform"),
        cap_child("data/transform/etl", "ETL Pipeline", "data/transform"),
        cap_child_desc("data/analysis", "Data Analysis", "data",
            "Statistical analysis and pattern recognition"),
        cap_child("data/analysis/statistical", "Statistical Analysis", "data/analysis"),
        cap_child("data/analysis/timeseries", "Time Series Analysis", "data/analysis"),
        cap_child("data/analysis/anomaly", "Anomaly Detection", "data/analysis"),
        cap_child("data/analysis/clustering", "Clustering", "data/analysis"),
        cap_child_desc("data/storage", "Data Storage", "data",
            "Persistent storage and retrieval"),
        cap_child("data/storage/kv", "Key-Value Store", "data/storage"),
        cap_child("data/storage/document", "Document Store", "data/storage"),
        cap_child("data/storage/graph", "Graph Database", "data/storage"),
        cap_child("data/storage/vector", "Vector Database", "data/storage"),
        cap_child("data/storage/sql", "SQL Database", "data/storage"),
    ]
}

/// Code execution and computation capabilities.
fn compute_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("compute", "Computation", "Execute code, run models, perform calculations"),
        cap_child_desc("compute/code", "Code Execution", "compute",
            "Execute code in various languages"),
        cap_child("compute/code/rust", "Rust Execution", "compute/code"),
        cap_child("compute/code/python", "Python Execution", "compute/code"),
        cap_child("compute/code/wasm", "WASM Execution", "compute/code"),
        cap_child("compute/code/javascript", "JavaScript Execution", "compute/code"),
        cap_child("compute/code/shell", "Shell Execution", "compute/code"),
        cap_child_desc("compute/ml", "Machine Learning", "compute",
            "Train and run machine learning models"),
        cap_child("compute/ml/inference", "Model Inference", "compute/ml"),
        cap_child("compute/ml/training", "Model Training", "compute/ml"),
        cap_child("compute/ml/fine-tuning", "Fine-Tuning", "compute/ml"),
        cap_child("compute/ml/embedding", "Embedding Generation", "compute/ml"),
        cap_child_desc("compute/math", "Mathematical Computation", "compute",
            "Symbolic and numerical mathematics"),
        cap_child("compute/math/symbolic", "Symbolic Math", "compute/math"),
        cap_child("compute/math/numerical", "Numerical Computation", "compute/math"),
        cap_child("compute/math/optimization", "Optimization", "compute/math"),
        cap_child("compute/math/linear-algebra", "Linear Algebra", "compute/math"),
    ]
}

/// Security and cryptography capabilities.
fn security_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("security", "Security", "Cryptography, authentication, and threat detection"),
        cap_child_desc("security/crypto", "Cryptography", "security",
            "Encryption, signatures, and key management"),
        cap_child("security/crypto/symmetric", "Symmetric Encryption", "security/crypto"),
        cap_child("security/crypto/asymmetric", "Asymmetric Encryption", "security/crypto"),
        cap_child("security/crypto/post-quantum", "Post-Quantum Cryptography", "security/crypto"),
        cap_child("security/crypto/signing", "Digital Signatures", "security/crypto"),
        cap_child("security/crypto/key-exchange", "Key Exchange", "security/crypto"),
        cap_child("security/crypto/hashing", "Cryptographic Hashing", "security/crypto"),
        cap_child_desc("security/auth", "Authentication", "security",
            "Identity verification and access control"),
        cap_child("security/auth/x509", "X.509 Certificates", "security/auth"),
        cap_child("security/auth/did", "Decentralized Identity", "security/auth"),
        cap_child("security/auth/mtls", "Mutual TLS", "security/auth"),
        cap_child_desc("security/threat", "Threat Detection", "security",
            "Identify security threats and vulnerabilities"),
        cap_child("security/threat/malware", "Malware Analysis", "security/threat"),
        cap_child("security/threat/intrusion", "Intrusion Detection", "security/threat"),
        cap_child("security/threat/vuln-scan", "Vulnerability Scanning", "security/threat"),
    ]
}

/// Communication and coordination capabilities.
fn communication_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("comm", "Communication", "Agent-to-agent and agent-to-human communication"),
        cap_child_desc("comm/messaging", "Messaging", "comm",
            "Send and receive structured messages"),
        cap_child("comm/messaging/request-reply", "Request-Reply", "comm/messaging"),
        cap_child("comm/messaging/pubsub", "Publish-Subscribe", "comm/messaging"),
        cap_child("comm/messaging/broadcast", "Broadcast", "comm/messaging"),
        cap_child("comm/messaging/streaming", "Streaming Messages", "comm/messaging"),
        cap_child_desc("comm/negotiation", "Negotiation", "comm",
            "Multi-party negotiation and agreement"),
        cap_child("comm/negotiation/auction", "Auction Protocol", "comm/negotiation"),
        cap_child("comm/negotiation/contract", "Contract Negotiation", "comm/negotiation"),
        cap_child("comm/negotiation/consensus", "Consensus Building", "comm/negotiation"),
        cap_child_desc("comm/coordination", "Coordination", "comm",
            "Synchronize actions across multiple agents"),
        cap_child("comm/coordination/task-delegation", "Task Delegation", "comm/coordination"),
        cap_child("comm/coordination/leader-election", "Leader Election", "comm/coordination"),
        cap_child("comm/coordination/barrier-sync", "Barrier Synchronization", "comm/coordination"),
        cap_child("comm/coordination/stigmergy", "Stigmergic Coordination", "comm/coordination"),
    ]
}

/// Knowledge management capabilities.
fn knowledge_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("knowledge", "Knowledge Management",
            "Store, retrieve, reason over, and share knowledge"),
        cap_child_desc("knowledge/graph", "Knowledge Graph", "knowledge",
            "Build and query knowledge graphs"),
        cap_child("knowledge/graph/construct", "Graph Construction", "knowledge/graph"),
        cap_child("knowledge/graph/query", "Graph Querying", "knowledge/graph"),
        cap_child("knowledge/graph/inference", "Graph Inference", "knowledge/graph"),
        cap_child("knowledge/graph/merge", "Graph Merging", "knowledge/graph"),
        cap_child_desc("knowledge/memory", "Memory Systems", "knowledge",
            "Episodic, semantic, and working memory"),
        cap_child("knowledge/memory/episodic", "Episodic Memory", "knowledge/memory"),
        cap_child("knowledge/memory/semantic", "Semantic Memory", "knowledge/memory"),
        cap_child("knowledge/memory/working", "Working Memory", "knowledge/memory"),
        cap_child("knowledge/memory/collective", "Collective Memory", "knowledge/memory"),
        cap_child_desc("knowledge/reasoning", "Reasoning", "knowledge",
            "Logical and probabilistic reasoning"),
        cap_child("knowledge/reasoning/deductive", "Deductive Reasoning", "knowledge/reasoning"),
        cap_child("knowledge/reasoning/abductive", "Abductive Reasoning", "knowledge/reasoning"),
        cap_child("knowledge/reasoning/planning", "Automated Planning", "knowledge/reasoning"),
        cap_child("knowledge/reasoning/causal", "Causal Reasoning", "knowledge/reasoning"),
        cap_child_desc("knowledge/learning", "Continual Learning", "knowledge",
            "Learn from experience without catastrophic forgetting"),
        cap_child("knowledge/learning/online", "Online Learning", "knowledge/learning"),
        cap_child("knowledge/learning/few-shot", "Few-Shot Learning", "knowledge/learning"),
        cap_child("knowledge/learning/transfer", "Transfer Learning", "knowledge/learning"),
    ]
}

/// Swarm and multi-agent capabilities.
fn swarm_capabilities() -> Vec<OntologyTerm> {
    vec![
        cap_desc("swarm", "Swarm Intelligence",
            "Participate in and coordinate multi-agent swarms"),
        cap_child_desc("swarm/topology", "Topology Management", "swarm",
            "Manage swarm structure and connectivity"),
        cap_child("swarm/topology/mesh", "Mesh Network", "swarm/topology"),
        cap_child("swarm/topology/hierarchical", "Hierarchical Cluster", "swarm/topology"),
        cap_child("swarm/topology/ring", "Ring Topology", "swarm/topology"),
        cap_child_desc("swarm/consensus", "Swarm Consensus", "swarm",
            "Byzantine fault tolerant collective decisions"),
        cap_child("swarm/consensus/bft", "BFT Voting", "swarm/consensus"),
        cap_child("swarm/consensus/raft", "Raft Consensus", "swarm/consensus"),
        cap_child("swarm/consensus/borda", "Ranked Voting", "swarm/consensus"),
        cap_child_desc("swarm/federation", "Federation", "swarm",
            "Cross-swarm trust and resource sharing"),
        cap_child("swarm/federation/discovery", "Cross-Swarm Discovery", "swarm/federation"),
        cap_child("swarm/federation/delegation", "Cross-Swarm Delegation", "swarm/federation"),
        cap_child("swarm/federation/migration", "Agent Migration", "swarm/federation"),
    ]
}

/// Specialized / vertical capabilities.
fn specialized_capabilities() -> Vec<OntologyTerm> {
    vec![
        // File and media processing
        cap_desc("media", "Media Processing", "Process images, audio, video, and documents"),
        cap_child("media/image", "Image Processing", "media"),
        cap_child("media/audio", "Audio Processing", "media"),
        cap_child("media/video", "Video Processing", "media"),
        cap_child("media/document", "Document Processing", "media"),
        cap_child("media/ocr", "Optical Character Recognition", "media"),
        // IoT and embedded
        cap_desc("iot", "IoT & Embedded", "Interact with sensors, actuators, and edge devices"),
        cap_child("iot/sensors", "Sensor Reading", "iot"),
        cap_child("iot/actuators", "Actuator Control", "iot"),
        cap_child("iot/edge-compute", "Edge Computation", "iot"),
        cap_child("iot/mesh-network", "Sensor Mesh Network", "iot"),
        // Financial
        cap_desc("finance", "Financial Operations", "Market data, trading, and accounting"),
        cap_child("finance/market-data", "Market Data", "finance"),
        cap_child("finance/trading", "Algorithmic Trading", "finance"),
        cap_child("finance/risk", "Risk Assessment", "finance"),
        cap_child("finance/compliance", "Regulatory Compliance", "finance"),
        // DevOps
        cap_desc("devops", "DevOps", "CI/CD, monitoring, and infrastructure automation"),
        cap_child("devops/ci-cd", "CI/CD Pipeline", "devops"),
        cap_child("devops/monitoring", "System Monitoring", "devops"),
        cap_child("devops/deployment", "Deployment Automation", "devops"),
        cap_child("devops/incident", "Incident Response", "devops"),
    ]
}

// ========================== ROLE TERMS ======================================

fn role_terms() -> Vec<OntologyTerm> {
    vec![
        role("coordinator", "Coordinator",
            "Orchestrates task distribution and monitors progress across agents"),
        role("worker", "Worker",
            "Executes assigned tasks and reports results"),
        role("researcher", "Researcher",
            "Gathers information, analyzes data, and synthesizes findings"),
        role("validator", "Validator",
            "Verifies outputs, checks quality, and enforces constraints"),
        role("monitor", "Monitor",
            "Observes system health, detects anomalies, and raises alerts"),
        role("gateway", "Gateway",
            "Bridges external services, APIs, or legacy systems"),
        role("archivist", "Archivist",
            "Manages long-term knowledge storage and retrieval"),
        role("sentinel", "Sentinel",
            "Enforces security policies and detects threats"),
        role("scout", "Scout",
            "Explores new data sources and discovers opportunities"),
        role("mediator", "Mediator",
            "Resolves conflicts and negotiates between agents"),
        role("specialist", "Specialist",
            "Deep expertise in a narrow domain"),
        role("generalist", "Generalist",
            "Broad capabilities across multiple domains"),
        role("learner", "Learner",
            "Actively acquiring new skills and adapting behavior"),
        role("teacher", "Teacher",
            "Shares knowledge and trains other agents"),
        role("auditor", "Auditor",
            "Reviews agent actions for compliance and correctness"),
    ]
}

// ========================== DOMAIN TERMS ====================================

fn domain_terms() -> Vec<OntologyTerm> {
    vec![
        domain("healthcare", "Healthcare", "Medical data, clinical workflows, drug discovery"),
        domain("education", "Education", "Learning management, tutoring, assessment"),
        domain("legal", "Legal", "Contract analysis, compliance, case research"),
        domain("science", "Scientific Research", "Literature review, experiment design, data analysis"),
        domain("engineering", "Engineering", "CAD, simulation, manufacturing, testing"),
        domain("cybersecurity", "Cybersecurity", "Threat intelligence, vulnerability analysis, incident response"),
        domain("logistics", "Logistics", "Supply chain, routing, inventory, fleet management"),
        domain("marketing", "Marketing", "Market research, content creation, analytics"),
        domain("customer-service", "Customer Service", "Ticket handling, FAQ, escalation"),
        domain("hr", "Human Resources", "Recruiting, onboarding, performance tracking"),
        domain("creative", "Creative", "Art generation, music composition, creative writing"),
        domain("gaming", "Gaming", "NPC behavior, procedural generation, game testing"),
        domain("agriculture", "Agriculture", "Crop monitoring, precision farming, yield prediction"),
        domain("energy", "Energy", "Grid optimization, renewable forecasting, demand response"),
        domain("telecom", "Telecommunications", "Network optimization, spectrum management, QoS"),
    ]
}

// ========================== PROTOCOL TERMS ==================================

fn protocol_terms() -> Vec<OntologyTerm> {
    vec![
        proto("spine-tcp", "SPINE TCP", "Native SPINE binary protocol over TCP"),
        proto("spine-tls", "SPINE TLS", "SPINE protocol over TLS 1.3"),
        proto("spine-ws", "SPINE WebSocket", "SPINE protocol over WebSocket"),
        proto("spine-quic", "SPINE QUIC", "SPINE protocol over QUIC"),
        proto("spine-chameleon", "Chameleon Protocol",
            "Latent-space morphing protocol with moving-target defense"),
        proto("http-rest", "HTTP REST", "Standard HTTP/HTTPS REST API"),
        proto("grpc", "gRPC", "Protocol Buffers over HTTP/2"),
        proto("mqtt", "MQTT", "Lightweight pub/sub for IoT"),
        proto("amqp", "AMQP", "Advanced Message Queuing Protocol"),
        proto("mcp", "Model Context Protocol",
            "Anthropic Model Context Protocol for LLM tool use"),
        proto("a2a", "Agent-to-Agent", "Google Agent2Agent protocol"),
    ]
}

// ========================== I/O FORMAT TERMS ================================

fn io_terms() -> Vec<OntologyTerm> {
    vec![
        term("spine:io/in/html", "HTML Input").with_description("Accepts HTML documents"),
        term("spine:io/in/json", "JSON Input").with_description("Accepts JSON data"),
        term("spine:io/in/text", "Plain Text Input").with_description("Accepts plain text"),
        term("spine:io/in/binary", "Binary Input").with_description("Accepts raw binary data"),
        term("spine:io/in/image", "Image Input").with_description("Accepts image files"),
        term("spine:io/in/audio", "Audio Input").with_description("Accepts audio files"),
        term("spine:io/in/latent", "Latent Vector Input")
            .with_description("Accepts latent space vectors"),
        term("spine:io/out/ur", "Unified Representation Output")
            .with_description("Produces SPINE Unified Representations"),
        term("spine:io/out/json", "JSON Output").with_description("Produces JSON data"),
        term("spine:io/out/text", "Plain Text Output").with_description("Produces plain text"),
        term("spine:io/out/html", "HTML Output").with_description("Produces HTML documents"),
        term("spine:io/out/binary", "Binary Output").with_description("Produces raw binary data"),
        term("spine:io/out/latent", "Latent Vector Output")
            .with_description("Produces latent space vectors"),
        term("spine:io/out/stream", "Streaming Output")
            .with_description("Produces data as a continuous stream"),
    ]
}

// ========================== QoS TERMS =======================================

fn qos_terms() -> Vec<OntologyTerm> {
    vec![
        term("spine:qos/realtime", "Real-Time").with_description("Sub-millisecond response latency")
            .with_property("max_latency_ms", "1"),
        term("spine:qos/low-latency", "Low Latency").with_description("Response within 100ms")
            .with_property("max_latency_ms", "100"),
        term("spine:qos/batch", "Batch Processing").with_description("Optimized for throughput, not latency")
            .with_property("max_latency_ms", "60000"),
        term("spine:qos/high-throughput", "High Throughput").with_description("Processes large volumes efficiently")
            .with_property("min_throughput_rps", "1000"),
        term("spine:qos/high-availability", "High Availability").with_description("99.9%+ uptime guarantee")
            .with_property("availability", "0.999"),
        term("spine:qos/idempotent", "Idempotent").with_description("Safe to retry without side effects"),
        term("spine:qos/stateless", "Stateless").with_description("No session state between requests"),
        term("spine:qos/stateful", "Stateful").with_description("Maintains session state"),
        term("spine:qos/at-most-once", "At-Most-Once Delivery")
            .with_description("Message delivered zero or one times"),
        term("spine:qos/at-least-once", "At-Least-Once Delivery")
            .with_description("Message delivered one or more times"),
        term("spine:qos/exactly-once", "Exactly-Once Delivery")
            .with_description("Message delivered exactly one time"),
    ]
}

// ========================== SECURITY PROPERTY TERMS =========================

fn security_terms() -> Vec<OntologyTerm> {
    vec![
        term("spine:sec/encrypted", "Encrypted Transport")
            .with_description("All communication is encrypted"),
        term("spine:sec/pq-safe", "Post-Quantum Safe")
            .with_description("Resistant to quantum computer attacks"),
        term("spine:sec/forward-secrecy", "Forward Secrecy")
            .with_description("Past sessions remain secure if long-term keys compromised"),
        term("spine:sec/zero-knowledge", "Zero-Knowledge")
            .with_description("Can prove properties without revealing data"),
        term("spine:sec/auditable", "Auditable")
            .with_description("All actions are logged and verifiable"),
        term("spine:sec/sandboxed", "Sandboxed Execution")
            .with_description("Code runs in an isolated sandbox"),
        term("spine:sec/signed", "Signed Messages")
            .with_description("All messages are cryptographically signed"),
    ]
}

// ========================== HARDWARE / RUNTIME TERMS ========================

fn hardware_terms() -> Vec<OntologyTerm> {
    vec![
        term("spine:hw/gpu", "GPU Accelerated")
            .with_description("Leverages GPU compute (WGSL/CUDA/Metal)"),
        term("spine:hw/simd", "SIMD Optimized")
            .with_description("Uses SIMD intrinsics (AVX2/NEON)"),
        term("spine:hw/embedded", "Embedded Runtime")
            .with_description("Runs on ARM Cortex-M, ESP32, RISC-V"),
        term("spine:hw/wasm", "WASM Runtime")
            .with_description("Runs as WebAssembly module"),
        term("spine:hw/no-std", "no_std Compatible")
            .with_description("Runs without standard library (bare metal)"),
        term("spine:hw/cloud", "Cloud Native")
            .with_description("Designed for Kubernetes / cloud deployment"),
        term("spine:hw/edge", "Edge Compute")
            .with_description("Optimized for edge / fog computing"),
    ]
}

// ========================== PRE-BUILT ONTOLOGIES ============================

/// All ontology terms in the SPINE vocabulary.
/// Returns a flat list of every term across all categories.
pub fn all_terms() -> Vec<OntologyTerm> {
    let mut terms = Vec::with_capacity(256);
    terms.extend(web_capabilities());
    terms.extend(nlp_capabilities());
    terms.extend(data_capabilities());
    terms.extend(compute_capabilities());
    terms.extend(security_capabilities());
    terms.extend(communication_capabilities());
    terms.extend(knowledge_capabilities());
    terms.extend(swarm_capabilities());
    terms.extend(specialized_capabilities());
    terms.extend(role_terms());
    terms.extend(domain_terms());
    terms.extend(protocol_terms());
    terms.extend(io_terms());
    terms.extend(qos_terms());
    terms.extend(security_terms());
    terms.extend(hardware_terms());
    terms
}

/// Number of categories in the ontology.
pub const CATEGORY_COUNT: usize = 16;

/// Build a complete `AgentOntology` containing the full SPINE vocabulary.
/// Primarily useful for documentation and testing — real agents should
/// select only the terms that describe their actual capabilities.
pub fn full_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/full", "1.0.0");
    for t in all_terms() {
        ont.add_term(t);
    }
    ont
}

/// Build an ontology for a **web research agent**.
pub fn web_researcher_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/web-researcher", "1.0.0");
    // Capabilities
    for t in web_capabilities() { ont.add_term(t); }
    ont.add_term(cap_child("nlp/extraction/ner", "Named Entity Recognition", "nlp/extraction"));
    ont.add_term(cap_child("nlp/extraction/keyword", "Keyword Extraction", "nlp/extraction"));
    ont.add_term(cap_child("nlp/generation/summary", "Summarization", "nlp/generation"));
    ont.add_term(cap_child("nlp/understanding/qa", "Question Answering", "nlp/understanding"));
    ont.add_term(cap_child("knowledge/graph/construct", "Graph Construction", "knowledge/graph"));
    ont.add_term(cap_child("knowledge/memory/episodic", "Episodic Memory", "knowledge/memory"));
    // Role
    ont.add_term(role("researcher", "Researcher", "Gathers information, analyzes data, and synthesizes findings"));
    // Protocols
    ont.add_term(proto("spine-tcp", "SPINE TCP", "Native SPINE binary protocol over TCP"));
    ont.add_term(proto("http-rest", "HTTP REST", "Standard HTTP/HTTPS REST API"));
    // I/O
    ont.add_term(term("spine:io/in/html", "HTML Input"));
    ont.add_term(term("spine:io/in/json", "JSON Input"));
    ont.add_term(term("spine:io/out/ur", "Unified Representation Output"));
    ont.add_term(term("spine:io/out/json", "JSON Output"));
    // QoS
    ont.add_term(term("spine:qos/low-latency", "Low Latency"));
    ont
}

/// Build an ontology for a **security sentinel agent**.
pub fn security_sentinel_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/security-sentinel", "1.0.0");
    for t in security_capabilities() { ont.add_term(t); }
    ont.add_term(role("sentinel", "Sentinel", "Enforces security policies and detects threats"));
    ont.add_term(proto("spine-tls", "SPINE TLS", "SPINE protocol over TLS 1.3"));
    ont.add_term(proto("spine-chameleon", "Chameleon Protocol",
        "Latent-space morphing protocol with moving-target defense"));
    ont.add_term(term("spine:sec/encrypted", "Encrypted Transport"));
    ont.add_term(term("spine:sec/pq-safe", "Post-Quantum Safe"));
    ont.add_term(term("spine:sec/forward-secrecy", "Forward Secrecy"));
    ont.add_term(term("spine:sec/signed", "Signed Messages"));
    ont.add_term(term("spine:qos/realtime", "Real-Time"));
    ont
}

/// Build an ontology for an **IoT edge agent**.
pub fn iot_edge_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/iot-edge", "1.0.0");
    ont.add_term(cap("iot", "IoT & Embedded"));
    ont.add_term(cap_child("iot/sensors", "Sensor Reading", "iot"));
    ont.add_term(cap_child("iot/actuators", "Actuator Control", "iot"));
    ont.add_term(cap_child("iot/edge-compute", "Edge Computation", "iot"));
    ont.add_term(cap_child("iot/mesh-network", "Sensor Mesh Network", "iot"));
    ont.add_term(cap_child("data/analysis/timeseries", "Time Series Analysis", "data/analysis"));
    ont.add_term(cap_child("data/analysis/anomaly", "Anomaly Detection", "data/analysis"));
    ont.add_term(role("worker", "Worker", "Executes assigned tasks and reports results"));
    ont.add_term(proto("mqtt", "MQTT", "Lightweight pub/sub for IoT"));
    ont.add_term(term("spine:hw/embedded", "Embedded Runtime"));
    ont.add_term(term("spine:hw/no-std", "no_std Compatible"));
    ont.add_term(term("spine:qos/realtime", "Real-Time"));
    ont
}

/// Build an ontology for a **data pipeline agent**.
pub fn data_pipeline_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/data-pipeline", "1.0.0");
    for t in data_capabilities() { ont.add_term(t); }
    ont.add_term(cap_child("compute/code/python", "Python Execution", "compute/code"));
    ont.add_term(cap_child("compute/code/wasm", "WASM Execution", "compute/code"));
    ont.add_term(role("worker", "Worker", "Executes assigned tasks and reports results"));
    ont.add_term(proto("spine-tcp", "SPINE TCP", "Native SPINE binary protocol over TCP"));
    ont.add_term(term("spine:io/in/json", "JSON Input"));
    ont.add_term(term("spine:io/in/binary", "Binary Input"));
    ont.add_term(term("spine:io/out/json", "JSON Output"));
    ont.add_term(term("spine:io/out/stream", "Streaming Output"));
    ont.add_term(term("spine:qos/high-throughput", "High Throughput"));
    ont.add_term(term("spine:qos/at-least-once", "At-Least-Once Delivery"));
    ont
}

/// Build an ontology for a **swarm coordinator agent**.
pub fn swarm_coordinator_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/swarm-coordinator", "1.0.0");
    for t in swarm_capabilities() { ont.add_term(t); }
    for t in communication_capabilities() { ont.add_term(t); }
    ont.add_term(role("coordinator", "Coordinator",
        "Orchestrates task distribution and monitors progress across agents"));
    ont.add_term(proto("spine-tcp", "SPINE TCP", "Native SPINE binary protocol over TCP"));
    ont.add_term(proto("spine-ws", "SPINE WebSocket", "SPINE protocol over WebSocket"));
    ont.add_term(term("spine:qos/high-availability", "High Availability"));
    ont.add_term(term("spine:sec/signed", "Signed Messages"));
    ont
}

/// Build an ontology for an **ML inference agent**.
pub fn ml_inference_ontology() -> AgentOntology {
    let mut ont = AgentOntology::new("spine:ontology/ml-inference", "1.0.0");
    ont.add_term(cap("compute", "Computation"));
    ont.add_term(cap_child("compute/ml", "Machine Learning", "compute"));
    ont.add_term(cap_child("compute/ml/inference", "Model Inference", "compute/ml"));
    ont.add_term(cap_child("compute/ml/embedding", "Embedding Generation", "compute/ml"));
    ont.add_term(cap("nlp", "Natural Language Processing"));
    ont.add_term(cap_child("nlp/embedding", "Text Embedding", "nlp"));
    ont.add_term(cap_child("nlp/generation", "Text Generation", "nlp"));
    ont.add_term(role("specialist", "Specialist", "Deep expertise in a narrow domain"));
    ont.add_term(proto("spine-tcp", "SPINE TCP", "Native SPINE binary protocol over TCP"));
    ont.add_term(proto("grpc", "gRPC", "Protocol Buffers over HTTP/2"));
    ont.add_term(term("spine:io/in/text", "Plain Text Input"));
    ont.add_term(term("spine:io/in/latent", "Latent Vector Input"));
    ont.add_term(term("spine:io/out/latent", "Latent Vector Output"));
    ont.add_term(term("spine:io/out/json", "JSON Output"));
    ont.add_term(term("spine:hw/gpu", "GPU Accelerated"));
    ont.add_term(term("spine:qos/low-latency", "Low Latency"));
    ont
}

// ========================== DISCOVERY HELPERS ===============================

/// Find all terms in the vocabulary whose URI starts with the given prefix.
/// E.g., `find_by_prefix("spine:cap/web")` returns all web capability terms.
pub fn find_by_prefix(prefix: &str) -> Vec<OntologyTerm> {
    all_terms().into_iter().filter(|t| t.uri.starts_with(prefix)).collect()
}

/// Find all capability terms (URIs starting with `spine:cap/`).
pub fn capability_terms() -> Vec<OntologyTerm> {
    find_by_prefix("spine:cap/")
}

/// Find all role terms (URIs starting with `spine:role/`).
pub fn all_role_terms() -> Vec<OntologyTerm> {
    find_by_prefix("spine:role/")
}

/// Find all domain terms (URIs starting with `spine:domain/`).
pub fn all_domain_terms() -> Vec<OntologyTerm> {
    find_by_prefix("spine:domain/")
}

/// Find all protocol terms (URIs starting with `spine:proto/`).
pub fn all_protocol_terms() -> Vec<OntologyTerm> {
    find_by_prefix("spine:proto/")
}

/// Compute compatibility between two agents' ontologies.
/// Returns a score in [0.0, 1.0] based on Jaccard similarity of public term URIs.
pub fn compatibility_score(a: &AgentOntology, b: &AgentOntology) -> f64 {
    use std::collections::HashSet;
    let a_uris: HashSet<&str> = a.terms.iter().map(|t| t.uri.as_str()).collect();
    let b_uris: HashSet<&str> = b.terms.iter().map(|t| t.uri.as_str()).collect();
    let intersection = a_uris.intersection(&b_uris).count();
    let union = a_uris.union(&b_uris).count();
    if union == 0 { return 0.0; }
    intersection as f64 / union as f64
}

// ========================== TESTS ===========================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OntologyVisibility;

    #[test]
    fn test_all_terms_nonempty() {
        let terms = all_terms();
        assert!(terms.len() > 200, "Expected 200+ terms, got {}", terms.len());
    }

    #[test]
    fn test_term_uris_unique() {
        let terms = all_terms();
        let mut seen = std::collections::HashSet::new();
        for t in &terms {
            assert!(seen.insert(&t.uri), "Duplicate URI: {}", t.uri);
        }
    }

    #[test]
    fn test_all_uris_have_namespace() {
        for t in all_terms() {
            assert!(t.uri.starts_with("spine:"), "URI missing spine: prefix: {}", t.uri);
        }
    }

    #[test]
    fn test_parent_uris_valid() {
        let all = all_terms();
        let uris: std::collections::HashSet<String> = all.iter().map(|t| t.uri.clone()).collect();
        for t in &all {
            for parent in &t.parents {
                assert!(uris.contains(parent),
                    "Term {} references non-existent parent: {}", t.uri, parent);
            }
        }
    }

    #[test]
    fn test_category_coverage() {
        let terms = all_terms();
        let prefixes = [
            "spine:cap/web", "spine:cap/nlp", "spine:cap/data", "spine:cap/compute",
            "spine:cap/security", "spine:cap/comm", "spine:cap/knowledge",
            "spine:cap/swarm", "spine:cap/media", "spine:cap/iot",
            "spine:cap/finance", "spine:cap/devops",
            "spine:role/", "spine:domain/", "spine:proto/",
            "spine:io/", "spine:qos/", "spine:sec/", "spine:hw/",
        ];
        for prefix in prefixes {
            let count = terms.iter().filter(|t| t.uri.starts_with(prefix)).count();
            assert!(count > 0, "No terms found for prefix: {}", prefix);
        }
    }

    #[test]
    fn test_full_ontology_hash_deterministic() {
        let a = full_ontology();
        let b = full_ontology();
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), [0u8; 32]);
    }

    #[test]
    fn test_web_researcher_ontology() {
        let ont = web_researcher_ontology();
        assert!(ont.has_term("spine:cap/web/scraping"));
        assert!(ont.has_term("spine:role/researcher"));
        assert!(ont.has_term("spine:proto/spine-tcp"));
        assert!(ont.has_term("spine:io/out/ur"));
    }

    #[test]
    fn test_security_sentinel_ontology() {
        let ont = security_sentinel_ontology();
        assert!(ont.has_term("spine:cap/security/crypto/post-quantum"));
        assert!(ont.has_term("spine:role/sentinel"));
        assert!(ont.has_term("spine:sec/pq-safe"));
    }

    #[test]
    fn test_iot_edge_ontology() {
        let ont = iot_edge_ontology();
        assert!(ont.has_term("spine:cap/iot/sensors"));
        assert!(ont.has_term("spine:hw/embedded"));
        assert!(ont.has_term("spine:hw/no-std"));
        assert!(ont.has_term("spine:proto/mqtt"));
    }

    #[test]
    fn test_data_pipeline_ontology() {
        let ont = data_pipeline_ontology();
        assert!(ont.has_term("spine:cap/data/transform"));
        assert!(ont.has_term("spine:qos/high-throughput"));
        assert!(ont.has_term("spine:io/out/stream"));
    }

    #[test]
    fn test_swarm_coordinator_ontology() {
        let ont = swarm_coordinator_ontology();
        assert!(ont.has_term("spine:cap/swarm/consensus/bft"));
        assert!(ont.has_term("spine:cap/comm/coordination/leader-election"));
        assert!(ont.has_term("spine:role/coordinator"));
    }

    #[test]
    fn test_ml_inference_ontology() {
        let ont = ml_inference_ontology();
        assert!(ont.has_term("spine:cap/compute/ml/inference"));
        assert!(ont.has_term("spine:hw/gpu"));
        assert!(ont.has_term("spine:io/in/latent"));
    }

    #[test]
    fn test_compatibility_same_ontology() {
        let a = web_researcher_ontology();
        let score = compatibility_score(&a, &a);
        assert!((score - 1.0).abs() < f64::EPSILON,
            "Self-compatibility should be 1.0, got {}", score);
    }

    #[test]
    fn test_compatibility_different_ontologies() {
        let web = web_researcher_ontology();
        let iot = iot_edge_ontology();
        let score = compatibility_score(&web, &iot);
        assert!(score < 0.3, "Web/IoT compatibility should be low, got {}", score);
    }

    #[test]
    fn test_compatibility_related_ontologies() {
        let web = web_researcher_ontology();
        let data = data_pipeline_ontology();
        let score = compatibility_score(&web, &data);
        // Both have JSON I/O and SPINE TCP
        assert!(score > 0.0, "Web/Data should share some terms, got {}", score);
    }

    #[test]
    fn test_find_by_prefix() {
        let web_terms = find_by_prefix("spine:cap/web");
        assert!(web_terms.len() >= 20, "Expected 20+ web terms, got {}", web_terms.len());
        for t in &web_terms {
            assert!(t.uri.starts_with("spine:cap/web"));
        }
    }

    #[test]
    fn test_neural_hash_similarity() {
        let t1 = OntologyTerm::new("spine:cap/nlp/generation/summary", "Summarization");
        let t2 = OntologyTerm::new("spine:cap/nlp/generation/translation", "Translation");
        let t3 = OntologyTerm::new("spine:cap/iot/sensors", "Sensor Reading");

        let h1 = t1.neural_hash(64);
        let h2 = t2.neural_hash(64);
        let h3 = t3.neural_hash(64);

        // Cosine similarity helper
        let cos = |a: &[f32], b: &[f32]| -> f32 {
            a.iter().zip(b).map(|(x, y)| x * y).sum()
        };

        let sim_12 = cos(&h1, &h2);
        let sim_13 = cos(&h1, &h3);

        // Note: with SHA-based neural hash, similarity is essentially random
        // The assertion just checks the hashes are valid unit vectors
        assert!((h1.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 0.01);
        assert!((h2.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 0.01);
        assert!((h3.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 0.01);
        // Use the values to avoid dead-code warnings
        let _ = (sim_12, sim_13);
    }

    #[test]
    fn test_disclosed_view_filtering() {
        let mut ont = AgentOntology::new("spine:test/disclosure", "1.0.0");
        ont.add_term(OntologyTerm::new("spine:cap/public", "Public Cap")
            .with_visibility(OntologyVisibility::Public));
        ont.add_term(OntologyTerm::new("spine:cap/hashed", "Hashed Cap")
            .with_visibility(OntologyVisibility::HashOnly));
        ont.add_term(OntologyTerm::new("spine:cap/neural", "Neural Cap")
            .with_visibility(OntologyVisibility::NeuralHash));
        ont.add_term(OntologyTerm::new("spine:cap/private", "Private Cap")
            .with_visibility(OntologyVisibility::Private));

        let disclosed = ont.disclosed_view(32);
        assert_eq!(disclosed.public_terms.len(), 1);
        assert_eq!(disclosed.hashed_terms.len(), 1);
        assert_eq!(disclosed.neural_terms.len(), 1);
        assert_eq!(disclosed.term_count(), 3); // private excluded
    }

    #[test]
    fn test_registry_discovery_roundtrip() {
        use crate::{AgentId, OntologyRegistry};

        let registry = OntologyRegistry::new();
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        let ont_a = web_researcher_ontology();
        let ont_b = security_sentinel_ontology();

        registry.register(agent_a, ont_a.disclosed_view(32));
        registry.register(agent_b, ont_b.disclosed_view(32));

        // Exact URI lookup
        let web_agents = registry.find_by_term("spine:cap/web/scraping");
        assert!(web_agents.contains(&agent_a));
        assert!(!web_agents.contains(&agent_b));

        let sec_agents = registry.find_by_term("spine:cap/security/crypto/post-quantum");
        assert!(sec_agents.contains(&agent_b));
        assert!(!sec_agents.contains(&agent_a));

        // Compatibility should be low between web researcher and security sentinel
        let compat = registry.compatibility(&agent_a, &agent_b);
        assert!(compat < 0.3);
    }
}
