# Structured Data Extraction

SPINE's parser includes a declarative extraction framework for pulling structured data from HTML documents.

## Overview

Instead of writing custom CSS-selector code for every page, agents define an **ExtractionSchema** — a declarative description of the fields to extract, their types, and any post-processing transforms.

## Core Types

| Type | Purpose |
|------|---------|
| `ExtractionSchema` | Named schema with ordered field definitions |
| `FieldDefinition` | CSS selector, field type, optional attribute, transform pipeline |
| `FieldType` | Text, Integer, Float, Boolean, Url, List, Record (nested) |
| `FieldTransform` | Trim, Lowercase, Uppercase, RegexCapture |
| `SchemaRegistry` | Multi-schema registration and batch extraction |

## Field Types

- **Text**: Extract text content or attribute value as a string.
- **Integer / Float / Boolean**: Parse extracted text into typed values.
- **Url**: Extract and normalize URL values.
- **List**: Collect all matching elements into an array.
- **Record**: Recursive sub-document extraction for nested structures.

## Transforms

Transforms are applied in pipeline order after extraction:

1. `Trim` — Strip leading/trailing whitespace
2. `Lowercase` / `Uppercase` — Case normalization
3. `RegexCapture(pattern)` — Extract first capture group

## Example

```rust,ignore
let schema = ExtractionSchema::new("article")
    .field("title", "h1.title", FieldType::Text)
    .field("author", ".byline", FieldType::Text)
    .field_with_attr("link", "a.permalink", "href", FieldType::Url)
    .field_with_transform("date", ".pubdate", FieldType::Text,
        vec![FieldTransform::Trim]);
```