//! A working agent web, end to end, in one process.
//!
//! Run with: `cargo run -p spine-name --example agent_web`
//!
//! Three publishers mint self-certifying names, publish signed records, and link
//! to one another. A fourth agent then does what no amount of transport could
//! let it do before: find a tool by *what it does*, resolve a name it has never
//! seen, and walk the resulting graph — with no certificate authority, no
//! registrar, and no central index anywhere in the path.

use ed25519_dalek::SigningKey;
use spine_name::{
    CrawlBudget, CrawlFrontier, Endpoint, Link, LocalResolver, NameRecord, Rel, Resolver, SpineUri,
};

const NOW: u64 = 1_700_000_000;

/// Mint a keypair and the self-certifying name it certifies against.
fn identity(seed: u8) -> (SigningKey, SpineUri) {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let name = SpineUri::did(key.verifying_key().to_bytes());
    (key, name)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resolver = LocalResolver::at_time(NOW);

    // ---- Three publishers -------------------------------------------------
    let (search_key, search_name) = identity(1);
    let (index_key, index_name) = identity(2);
    let (fetch_key, fetch_name) = identity(3);

    println!("Minted three self-certifying names — no registrar involved:");
    println!("  search : {search_name}");
    println!("  index  : {index_name}");
    println!("  fetch  : {fetch_name}\n");

    // The search tool advertises a capability and depends on the fetcher.
    let mut search = NameRecord::new(search_name.clone(), 1, NOW)?
        .with_endpoint(Endpoint::new("quic", "10.0.0.1:9440").with_priority(0))
        .with_endpoint(Endpoint::new("tcp", "10.0.0.1:9441").with_priority(1))
        .with_capability("web.search")
        .with_meta("title", "Search tool")
        .with_link(Link::new(Rel::Requires, fetch_name.clone()).with_title("fetcher"))
        .with_link(Link::new(Rel::Peer, index_name.clone()).with_title("index"));
    search.sign(&search_key)?;

    let mut index = NameRecord::new(index_name.clone(), 1, NOW)?
        .with_endpoint(Endpoint::new("tcp", "10.0.0.2:9440"))
        .with_capability("web.search")
        .with_capability("web.index")
        .with_meta("title", "Index tool");
    index.sign(&index_key)?;

    let mut fetch = NameRecord::new(fetch_name.clone(), 1, NOW)?
        .with_endpoint(Endpoint::new("tcp", "10.0.0.3:9440"))
        .with_capability("web.fetch")
        .with_meta("title", "Fetcher");
    fetch.sign(&fetch_key)?;

    for record in [&search, &index, &fetch] {
        resolver.publish(record.clone())?;
    }
    println!("Published 3 signed records.\n");

    // ---- 1. Find a tool by what it does, not where it lives ---------------
    println!("1. Capability lookup — `web.search`:");
    for provider in resolver.find_providers("web.search").await? {
        let title = provider.meta.get("title").map(String::as_str).unwrap_or("-");
        println!(
            "   {title:<12} {} endpoint(s)  {}",
            provider.endpoints.len(),
            provider.name
        );
    }
    println!("   (ranked by reachability — no search engine consulted)\n");

    // ---- 2. Resolve a name and verify it against itself -------------------
    println!("2. Resolving {search_name}");
    let hit = resolver.resolve(&search_name).await?;
    println!("   provenance : {:?}", hit.provenance);
    println!("   verifies   : {}", hit.record.verify().is_ok());
    println!(
        "   preferred  : {}://{}",
        hit.record.endpoints_by_priority()[0].transport,
        hit.record.endpoints_by_priority()[0].address
    );
    println!("   (the name IS the key — nothing external was trusted)\n");

    // ---- 3. Batch resolution ----------------------------------------------
    let batch = vec![search_name.clone(), index_name.clone(), fetch_name.clone()];
    let results = resolver.resolve_many(&batch).await;
    println!(
        "3. Batch resolved {}/{} names in one call.\n",
        results.iter().filter(|r| r.is_ok()).count(),
        results.len()
    );

    // ---- 4. Walk the graph -------------------------------------------------
    println!("4. Crawling from the search tool (depth 2):");
    let mut frontier = CrawlFrontier::new(CrawlBudget::default().with_max_depth(2));
    frontier.seed(search_name.clone());

    while let Some(visit) = frontier.next_visit() {
        match resolver.resolve(&visit.uri).await {
            Ok(res) => {
                let title = res.record.meta.get("title").map(String::as_str).unwrap_or("-");
                let via = visit.via.map(|r| r.as_str().to_string());
                println!(
                    "   depth {}  via {:<9} {title:<12} caps={:?}",
                    visit.depth,
                    via.as_deref().unwrap_or("seed"),
                    res.record.capabilities
                );
                frontier.expand(&visit.uri, &res.record.links, visit.depth);
            }
            Err(e) => println!("   depth {}  unresolved: {e}", visit.depth),
        }
    }
    println!(
        "   visited {} names, {} skipped\n",
        frontier.visited_count(),
        frontier.skipped().len()
    );

    // ---- 5. Content addressing --------------------------------------------
    let payload = b"an artifact an agent produced";
    let blob = SpineUri::blob_of(payload);
    println!("5. Content-addressed name for a {}-byte artifact:", payload.len());
    println!("   {blob}");
    println!("   immutable: {} — cacheable forever, never revalidated", blob.is_immutable());

    let stats = resolver.cache_stats();
    println!("\nCache: {} hits, {} misses", stats.hits, stats.misses);
    Ok(())
}
