//! Schema-driven structured data extraction from HTML.
//!
//! Defines [`ExtractionSchema`] for specifying expected data shapes,
//! and [`SchemaRegistry`] for extracting structured [`DataValue`]s
//! from raw HTML using CSS selectors.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schema defining expected structured data from a web page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionSchema {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

/// A single field to extract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDef {
    pub name: String,
    /// CSS selector to locate the element(s).
    pub selector: String,
    pub field_type: FieldType,
    pub required: bool,
    /// When set, extract this attribute instead of text content
    /// (e.g., `"href"`, `"src"`, `"data-price"`).
    #[serde(default)]
    pub attribute: Option<String>,
    /// Post-extraction transforms applied in order.
    #[serde(default)]
    pub transforms: Vec<Transform>,
}

/// Expected field type; drives parsing and validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FieldType {
    Text,
    Integer,
    Float,
    Boolean,
    Url,
    /// Extract all matching elements as a list.
    List(Box<FieldType>),
    /// Nested record extracted from each child element.
    Record(Vec<FieldDef>),
}

/// Post-extraction transforms applied to raw text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Transform {
    Trim,
    Lowercase,
    Uppercase,
    /// Capture the first group of a regex.
    RegexCapture(String),
}

/// An extracted value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataValue {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Url(String),
    List(Vec<DataValue>),
    Record(HashMap<String, DataValue>),
    Null,
}

/// Result of extracting data via a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedData {
    pub schema_name: String,
    pub fields: HashMap<String, DataValue>,
    /// Non-fatal problems encountered during extraction.
    pub warnings: Vec<String>,
}

/// Errors that prevent extraction.
#[derive(Debug, thiserror::Error)]
pub enum ExtractionError {
    #[error("schema not found: {0}")]
    SchemaNotFound(String),
    #[error("invalid selector `{selector}`: {reason}")]
    InvalidSelector { selector: String, reason: String },
    #[error("required field `{0}` missing")]
    RequiredFieldMissing(String),
}

/// Registry of schemas with extraction capabilities.
#[derive(Debug, Default)]
pub struct SchemaRegistry {
    schemas: HashMap<String, ExtractionSchema>,
}

impl SchemaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, schema: ExtractionSchema) {
        self.schemas.insert(schema.name.clone(), schema);
    }

    pub fn list_schemas(&self) -> Vec<&str> {
        self.schemas.keys().map(|s| s.as_str()).collect()
    }

    pub fn get_schema(&self, name: &str) -> Option<&ExtractionSchema> {
        self.schemas.get(name)
    }

    /// Extract structured data from HTML using the named schema.
    pub fn extract(
        &self,
        html: &str,
        schema_name: &str,
    ) -> Result<ExtractedData, ExtractionError> {
        let schema = self
            .schemas
            .get(schema_name)
            .ok_or_else(|| ExtractionError::SchemaNotFound(schema_name.to_string()))?;

        let document = Html::parse_document(html);
        let mut fields = HashMap::new();
        let mut warnings = Vec::new();

        for field_def in &schema.fields {
            match extract_field(&document, field_def) {
                Ok(value) => {
                    fields.insert(field_def.name.clone(), value);
                }
                Err(ExtractionError::RequiredFieldMissing(ref name)) if field_def.required => {
                    return Err(ExtractionError::RequiredFieldMissing(name.clone()));
                }
                Err(e) => {
                    if field_def.required {
                        return Err(e);
                    }
                    warnings.push(format!("field `{}`: {}", field_def.name, e));
                    fields.insert(field_def.name.clone(), DataValue::Null);
                }
            }
        }

        Ok(ExtractedData {
            schema_name: schema_name.to_string(),
            fields,
            warnings,
        })
    }
}

/// Extract a single field from the document.
fn extract_field(
    document: &Html,
    field_def: &FieldDef,
) -> Result<DataValue, ExtractionError> {
    let selector = Selector::parse(&field_def.selector).map_err(|_| {
        ExtractionError::InvalidSelector {
            selector: field_def.selector.clone(),
            reason: "CSS parse error".to_string(),
        }
    })?;

    match &field_def.field_type {
        FieldType::List(inner_type) => {
            let values: Vec<DataValue> = document
                .select(&selector)
                .filter_map(|el| {
                    let raw = raw_text_or_attr(&el, field_def.attribute.as_deref());
                    let transformed = apply_transforms(&raw, &field_def.transforms);
                    coerce(&transformed, inner_type).ok()
                })
                .collect();
            Ok(DataValue::List(values))
        }
        FieldType::Record(sub_fields) => {
            if let Some(el) = document.select(&selector).next() {
                let inner_html = el.inner_html();
                let sub_doc = Html::parse_fragment(&inner_html);
                let mut record = HashMap::new();
                for sub_def in sub_fields {
                    match extract_field(&sub_doc, sub_def) {
                        Ok(v) => {
                            record.insert(sub_def.name.clone(), v);
                        }
                        Err(_) if !sub_def.required => {
                            record.insert(sub_def.name.clone(), DataValue::Null);
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(DataValue::Record(record))
            } else {
                Err(ExtractionError::RequiredFieldMissing(
                    field_def.name.clone(),
                ))
            }
        }
        other => {
            if let Some(el) = document.select(&selector).next() {
                let raw = raw_text_or_attr(&el, field_def.attribute.as_deref());
                let transformed = apply_transforms(&raw, &field_def.transforms);
                coerce(&transformed, other)
            } else {
                Err(ExtractionError::RequiredFieldMissing(
                    field_def.name.clone(),
                ))
            }
        }
    }
}

/// Get text content or attribute value from an element.
fn raw_text_or_attr(el: &scraper::ElementRef, attr: Option<&str>) -> String {
    if let Some(attr_name) = attr {
        el.value()
            .attr(attr_name)
            .unwrap_or("")
            .to_string()
    } else {
        el.text().collect::<Vec<_>>().join(" ").trim().to_string()
    }
}

/// Apply a chain of transforms to raw text.
fn apply_transforms(input: &str, transforms: &[Transform]) -> String {
    let mut s = input.to_string();
    for t in transforms {
        s = match t {
            Transform::Trim => s.trim().to_string(),
            Transform::Lowercase => s.to_lowercase(),
            Transform::Uppercase => s.to_uppercase(),
            Transform::RegexCapture(pattern) => {
                // Only compile once per extraction — acceptable for non-hot-path
                if let Ok(re) = regex::Regex::new(pattern) {
                    re.captures(&s)
                        .and_then(|c| c.get(1).or_else(|| c.get(0)))
                        .map(|m| m.as_str().to_string())
                        .unwrap_or(s)
                } else {
                    s
                }
            }
        };
    }
    s
}

/// Coerce a string into the expected type.
fn coerce(value: &str, field_type: &FieldType) -> Result<DataValue, ExtractionError> {
    match field_type {
        FieldType::Text => Ok(DataValue::Text(value.to_string())),
        FieldType::Url => Ok(DataValue::Url(value.to_string())),
        FieldType::Integer => value
            .trim()
            .replace(',', "")
            .parse::<i64>()
            .map(DataValue::Integer)
            .map_err(|_| ExtractionError::InvalidSelector {
                selector: String::new(),
                reason: format!("cannot parse `{value}` as integer"),
            }),
        FieldType::Float => value
            .trim()
            .replace(',', "")
            .parse::<f64>()
            .map(DataValue::Float)
            .map_err(|_| ExtractionError::InvalidSelector {
                selector: String::new(),
                reason: format!("cannot parse `{value}` as float"),
            }),
        FieldType::Boolean => {
            let b = matches!(
                value.trim().to_lowercase().as_str(),
                "true" | "yes" | "1" | "on"
            );
            Ok(DataValue::Boolean(b))
        }
        FieldType::List(_) | FieldType::Record(_) => {
            // Should be handled by extract_field directly
            Ok(DataValue::Text(value.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product_schema() -> ExtractionSchema {
        ExtractionSchema {
            name: "product".to_string(),
            fields: vec![
                FieldDef {
                    name: "title".to_string(),
                    selector: "h1.product-title".to_string(),
                    field_type: FieldType::Text,
                    required: true,
                    attribute: None,
                    transforms: vec![Transform::Trim],
                },
                FieldDef {
                    name: "price".to_string(),
                    selector: "span.price".to_string(),
                    field_type: FieldType::Float,
                    required: true,
                    attribute: None,
                    transforms: vec![Transform::RegexCapture(r"[\d,]+\.?\d*".to_string())],
                },
                FieldDef {
                    name: "image_url".to_string(),
                    selector: "img.product-image".to_string(),
                    field_type: FieldType::Url,
                    required: false,
                    attribute: Some("src".to_string()),
                    transforms: vec![],
                },
                FieldDef {
                    name: "in_stock".to_string(),
                    selector: "span.availability".to_string(),
                    field_type: FieldType::Boolean,
                    required: false,
                    attribute: None,
                    transforms: vec![Transform::Lowercase],
                },
            ],
        }
    }

    const PRODUCT_HTML: &str = r#"
    <html><body>
        <h1 class="product-title">  Widget Pro 3000  </h1>
        <span class="price">$49.99</span>
        <img class="product-image" src="https://example.com/widget.jpg" alt="Widget" />
        <span class="availability">Yes</span>
    </body></html>
    "#;

    #[test]
    fn test_extract_product_basic() {
        let mut registry = SchemaRegistry::new();
        registry.register(product_schema());

        let result = registry.extract(PRODUCT_HTML, "product").unwrap();
        assert_eq!(result.schema_name, "product");
        assert_eq!(
            result.fields.get("title"),
            Some(&DataValue::Text("Widget Pro 3000".to_string()))
        );
        assert_eq!(result.fields.get("price"), Some(&DataValue::Float(49.99)));
        assert_eq!(
            result.fields.get("image_url"),
            Some(&DataValue::Url("https://example.com/widget.jpg".to_string()))
        );
        assert_eq!(
            result.fields.get("in_stock"),
            Some(&DataValue::Boolean(true))
        );
    }

    #[test]
    fn test_extract_schema_not_found() {
        let registry = SchemaRegistry::new();
        let err = registry.extract("<html></html>", "missing").unwrap_err();
        assert!(matches!(err, ExtractionError::SchemaNotFound(_)));
    }

    #[test]
    fn test_extract_required_field_missing() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "test".to_string(),
            fields: vec![FieldDef {
                name: "mandatory".to_string(),
                selector: "span.does-not-exist".to_string(),
                field_type: FieldType::Text,
                required: true,
                attribute: None,
                transforms: vec![],
            }],
        });
        let err = registry.extract("<html><body></body></html>", "test").unwrap_err();
        assert!(matches!(err, ExtractionError::RequiredFieldMissing(_)));
    }

    #[test]
    fn test_extract_optional_field_missing_becomes_null() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "test".to_string(),
            fields: vec![FieldDef {
                name: "optional_field".to_string(),
                selector: "span.missing".to_string(),
                field_type: FieldType::Text,
                required: false,
                attribute: None,
                transforms: vec![],
            }],
        });
        let result = registry
            .extract("<html><body></body></html>", "test")
            .unwrap();
        assert_eq!(
            result.fields.get("optional_field"),
            Some(&DataValue::Null)
        );
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn test_extract_list_field() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "links".to_string(),
            fields: vec![FieldDef {
                name: "urls".to_string(),
                selector: "a".to_string(),
                field_type: FieldType::List(Box::new(FieldType::Url)),
                required: false,
                attribute: Some("href".to_string()),
                transforms: vec![],
            }],
        });
        let html = r#"<html><body>
            <a href="https://a.com">A</a>
            <a href="https://b.com">B</a>
            <a href="https://c.com">C</a>
        </body></html>"#;
        let result = registry.extract(html, "links").unwrap();
        if let Some(DataValue::List(urls)) = result.fields.get("urls") {
            assert_eq!(urls.len(), 3);
            assert_eq!(urls[0], DataValue::Url("https://a.com".to_string()));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn test_extract_integer_field() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "count".to_string(),
            fields: vec![FieldDef {
                name: "qty".to_string(),
                selector: "span.qty".to_string(),
                field_type: FieldType::Integer,
                required: true,
                attribute: None,
                transforms: vec![],
            }],
        });
        let html = r#"<html><body><span class="qty">42</span></body></html>"#;
        let result = registry.extract(html, "count").unwrap();
        assert_eq!(result.fields.get("qty"), Some(&DataValue::Integer(42)));
    }

    #[test]
    fn test_extract_integer_with_commas() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "big".to_string(),
            fields: vec![FieldDef {
                name: "population".to_string(),
                selector: "span".to_string(),
                field_type: FieldType::Integer,
                required: true,
                attribute: None,
                transforms: vec![],
            }],
        });
        let html = r#"<html><body><span>1,234,567</span></body></html>"#;
        let result = registry.extract(html, "big").unwrap();
        assert_eq!(
            result.fields.get("population"),
            Some(&DataValue::Integer(1234567))
        );
    }

    #[test]
    fn test_transform_uppercase() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "upper".to_string(),
            fields: vec![FieldDef {
                name: "label".to_string(),
                selector: "span".to_string(),
                field_type: FieldType::Text,
                required: true,
                attribute: None,
                transforms: vec![Transform::Uppercase],
            }],
        });
        let html = r#"<html><body><span>hello world</span></body></html>"#;
        let result = registry.extract(html, "upper").unwrap();
        assert_eq!(
            result.fields.get("label"),
            Some(&DataValue::Text("HELLO WORLD".to_string()))
        );
    }

    #[test]
    fn test_transform_chain() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "chain".to_string(),
            fields: vec![FieldDef {
                name: "val".to_string(),
                selector: "span".to_string(),
                field_type: FieldType::Text,
                required: true,
                attribute: None,
                transforms: vec![Transform::Trim, Transform::Lowercase],
            }],
        });
        let html = r#"<html><body><span>  HeLLo  </span></body></html>"#;
        let result = registry.extract(html, "chain").unwrap();
        assert_eq!(
            result.fields.get("val"),
            Some(&DataValue::Text("hello".to_string()))
        );
    }

    #[test]
    fn test_extract_record_field() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "card".to_string(),
            fields: vec![FieldDef {
                name: "author".to_string(),
                selector: "div.author".to_string(),
                field_type: FieldType::Record(vec![
                    FieldDef {
                        name: "name".to_string(),
                        selector: "span.name".to_string(),
                        field_type: FieldType::Text,
                        required: true,
                        attribute: None,
                        transforms: vec![],
                    },
                    FieldDef {
                        name: "link".to_string(),
                        selector: "a".to_string(),
                        field_type: FieldType::Url,
                        required: false,
                        attribute: Some("href".to_string()),
                        transforms: vec![],
                    },
                ]),
                required: true,
                attribute: None,
                transforms: vec![],
            }],
        });
        let html = r#"<html><body>
            <div class="author">
                <span class="name">Alice</span>
                <a href="https://alice.dev">profile</a>
            </div>
        </body></html>"#;
        let result = registry.extract(html, "card").unwrap();
        if let Some(DataValue::Record(rec)) = result.fields.get("author") {
            assert_eq!(rec.get("name"), Some(&DataValue::Text("Alice".to_string())));
            assert_eq!(
                rec.get("link"),
                Some(&DataValue::Url("https://alice.dev".to_string()))
            );
        } else {
            panic!("expected Record");
        }
    }

    #[test]
    fn test_invalid_selector() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "bad".to_string(),
            fields: vec![FieldDef {
                name: "x".to_string(),
                selector: "[[[invalid".to_string(),
                field_type: FieldType::Text,
                required: true,
                attribute: None,
                transforms: vec![],
            }],
        });
        let err = registry
            .extract("<html><body></body></html>", "bad")
            .unwrap_err();
        assert!(matches!(err, ExtractionError::InvalidSelector { .. }));
    }

    #[test]
    fn test_schema_serde_roundtrip() {
        let schema = product_schema();
        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: ExtractionSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "product");
        assert_eq!(deserialized.fields.len(), 4);
    }

    #[test]
    fn test_data_value_serde_roundtrip() {
        let val = DataValue::Record(HashMap::from([
            ("a".to_string(), DataValue::Integer(42)),
            (
                "b".to_string(),
                DataValue::List(vec![DataValue::Text("x".to_string())]),
            ),
        ]));
        let json = serde_json::to_string(&val).unwrap();
        let back: DataValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn test_list_schemas() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "a".to_string(),
            fields: vec![],
        });
        registry.register(ExtractionSchema {
            name: "b".to_string(),
            fields: vec![],
        });
        let mut names = registry.list_schemas();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_boolean_false_values() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "bool".to_string(),
            fields: vec![FieldDef {
                name: "flag".to_string(),
                selector: "span".to_string(),
                field_type: FieldType::Boolean,
                required: true,
                attribute: None,
                transforms: vec![],
            }],
        });
        let html = r#"<html><body><span>no</span></body></html>"#;
        let result = registry.extract(html, "bool").unwrap();
        assert_eq!(
            result.fields.get("flag"),
            Some(&DataValue::Boolean(false))
        );
    }

    #[test]
    fn test_extract_attribute() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "meta".to_string(),
            fields: vec![FieldDef {
                name: "custom".to_string(),
                selector: "div".to_string(),
                field_type: FieldType::Text,
                required: true,
                attribute: Some("data-info".to_string()),
                transforms: vec![],
            }],
        });
        let html = r#"<html><body><div data-info="secret">visible</div></body></html>"#;
        let result = registry.extract(html, "meta").unwrap();
        assert_eq!(
            result.fields.get("custom"),
            Some(&DataValue::Text("secret".to_string()))
        );
    }

    #[test]
    fn test_empty_list_extraction() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "empty_list".to_string(),
            fields: vec![FieldDef {
                name: "items".to_string(),
                selector: "li.nonexistent".to_string(),
                field_type: FieldType::List(Box::new(FieldType::Text)),
                required: false,
                attribute: None,
                transforms: vec![],
            }],
        });
        let html = "<html><body></body></html>";
        let result = registry.extract(html, "empty_list").unwrap();
        assert_eq!(
            result.fields.get("items"),
            Some(&DataValue::List(vec![]))
        );
    }

    #[test]
    fn test_regex_capture_price() {
        let mut registry = SchemaRegistry::new();
        registry.register(ExtractionSchema {
            name: "price_test".to_string(),
            fields: vec![FieldDef {
                name: "amount".to_string(),
                selector: "span".to_string(),
                field_type: FieldType::Float,
                required: true,
                attribute: None,
                transforms: vec![Transform::RegexCapture(r"([\d.]+)".to_string())],
            }],
        });
        let html = r#"<html><body><span>Price: $123.45 USD</span></body></html>"#;
        let result = registry.extract(html, "price_test").unwrap();
        assert_eq!(
            result.fields.get("amount"),
            Some(&DataValue::Float(123.45))
        );
    }
}
