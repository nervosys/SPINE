//! # Batch Web Scraper
//!
//! Demonstrates navigating multiple URLs and extracting structured data
//! from each page into a unified dataset.

use anyhow::Result;
use serde_json::json;
use spine_agent::AgentClient;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    let urls = vec![
        "https://example.com",
        "https://httpbin.org/html",
        "https://www.rust-lang.org",
    ];

    let mut results = Vec::new();

    for url in &urls {
        println!("Scraping {}...", url);

        client.navigate(url).await?;
        let ur = client.get_ur().await?;

        // Extract headings
        let headings: Vec<String> = ur
            .elements
            .iter()
            .filter_map(|e| match e {
                spine_parser::Element::Heading { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        // Extract links
        let links: Vec<(String, String)> = ur
            .elements
            .iter()
            .filter_map(|e| match e {
                spine_parser::Element::Link { text, url } => Some((text.clone(), url.clone())),
                _ => None,
            })
            .collect();

        results.push(json!({
            "url": url,
            "title": ur.title,
            "element_count": ur.elements.len(),
            "headings": headings,
            "link_count": links.len(),
        }));
    }

    // Output as JSON array
    println!("\n{}", serde_json::to_string_pretty(&results)?);

    // Store in knowledge base for later retrieval
    for result in &results {
        let key = format!("scrape:{}", result["url"].as_str().unwrap_or("unknown"));
        client
            .store_knowledge(&key, result.clone(), vec!["scraping".into(), "web".into()])
            .await?;
    }

    println!("\nStored {} pages in knowledge base", results.len());
    Ok(())
}
