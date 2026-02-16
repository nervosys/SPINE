# Navigation & Unified Representations

## Navigating

```rust
client.navigate("https://example.com").await?;
```

## Getting Structured Content

```rust
let ur = client.get_ur().await?;
println!("Title: {}", ur.title);
for element in &ur.elements {
    match element {
        Element::Heading { level, text } => println!("H{}: {}", level, text),
        Element::Link { text, url } => println!("[{}]({})", text, url),
        Element::Text(t) => println!("{}", t),
        _ => {}
    }
}
```

The `UnifiedRepresentation` contains:

| Field      | Type                      | Description               |
| ---------- | ------------------------- | ------------------------- |
| `title`    | `String`                  | Page title                |
| `elements` | `Vec<Element>`            | Structured content tree   |
| `metadata` | `HashMap<String, String>` | Meta tags, OpenGraph data |

## Element Types

| Variant                           | Fields                   | Example               |
| --------------------------------- | ------------------------ | --------------------- |
| `Text(String)`                    | Text content             | Paragraph text        |
| `Heading { level, text }`         | `u8`, `String`           | `<h1>` through `<h6>` |
| `Link { text, url }`              | `String`, `String`       | Hyperlinks            |
| `Button { text, action_id }`      | `String`, `String`       | Interactive buttons   |
| `Input { label, input_type, id }` | `String` × 3             | Form inputs           |
| `Image { alt, src }`              | `String`, `String`       | Images                |
| `List { items, ordered }`         | `Vec<Element>`, `bool`   | Lists                 |
| `Container { tag, children }`     | `String`, `Vec<Element>` | Nested containers     |

## Raw HTML

```rust
let html = client.get_raw_html().await?;
```

## Interaction

```rust
// Click an element
client.click("submit-button").await?;

// Type into a field
client.type_text("search-input", "hello world").await?;
```

## Latent Vectors

Extract neural embeddings of page content:

```rust
let latent = client.get_latent_ur(256).await?;
// latent: Vec<f32> of dimension 256
```
