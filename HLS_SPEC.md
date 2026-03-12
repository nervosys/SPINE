# SPINE Source Language (HLS) Specification

**Version**: 0.1.0-alpha  
**Status**: Experimental

## Overview

SPINE Source (HLS) is a declarative language for defining web interfaces as executable programs. It compiles to SPINE Binary (HLB), which can be executed by the SPINE Core engine.

## Design Goals

1. **Human-Readable**: Easy to write and understand
2. **AI-Friendly**: Simple syntax that LLMs can generate
3. **Executable**: Programs, not documents
4. **Composable**: Elements can be nested and reused
5. **Event-Driven**: Native support for interactions and state changes

## Syntax

### Elements

The basic building block of HLS is an **element**:

```hls
element TagName {
  // Element body
}
```

Elements compile to `DefineElement` instructions in HLB.

### Attributes

Elements can have attributes:

```hls
element Button {
  attribute text "Click Me"
  attribute class "primary"
  attribute id "submit-btn"
}
```

Attributes compile to `SetAttribute` instructions.

### Nesting

Elements can contain child elements:

```hls
element App {
  element Header {
    text "Welcome"
  }
  
  element Content {
    element Sidebar {}
    element MainArea {}
  }
}
```

Child relationships compile to `AddChild` instructions.

### Text Content

Shorthand for text-only elements:

```hls
element Heading {
  text "Hello, World!"
}

// Equivalent to:
element Heading {
  attribute text "Hello, World!"
}
```

### Events

Elements can emit events:

```hls
element Button {
  text "Submit"
  on_click -> emit("form_submitted", { form_id: 1 })
}
```

Events compile to `EmitEvent` instructions.

### Comments

```hls
// Single-line comment

/*
 * Multi-line comment
 */

element App {
  // This is a comment inside an element
}
```

## Semantic Elements

HLS supports a set of semantic elements that map to standard web concepts. These are used by the `HumanTranspiler` to convert legacy HTML into HLS.

| HLS Element  | HTML Equivalent | Description                                     |
| ------------ | --------------- | ----------------------------------------------- |
| `Navigation` | `<nav>`         | Navigation links and menus                      |
| `Article`    | `<article>`     | Self-contained content                          |
| `Section`    | `<section>`     | Generic document section                        |
| `Header`     | `<header>`      | Introductory content or nav links               |
| `Footer`     | `<footer>`      | Footer for its nearest sectioning content       |
| `Main`       | `<main>`        | Main content of the document                    |
| `Aside`      | `<aside>`       | Content indirectly related to main content      |
| `Form`       | `<form>`        | Interactive controls for submitting information |
| `Table`      | `<table>`       | Tabular data                                    |
| `Button`     | `<button>`      | Clickable button                                |
| `Input`      | `<input>`       | Interactive control for user input              |
| `Heading`    | `<h1>`-`<h6>`   | Section headings                                |
| `Paragraph`  | `<p>`           | Paragraph of text                               |
| `Link`       | `<a>`           | Hyperlink                                       |
| `Image`      | `<img>`         | Embedded image                                  |
| `List`       | `<ul>`, `<ol>`  | List of items                                   |
| `ListItem`   | `<li>`          | Item in a list                                  |
| `Container`  | `<div>`         | Generic flow content container                  |
| `Span`       | `<span>`        | Generic inline container                        |

## Data Types

### Primitives

- **String**: `"text"` (double-quoted)
- **Number**: `42`, `3.14`
- **Boolean**: `true`, `false`
- **Null**: `null`

### Collections

- **Object**: `{ key: "value", count: 10 }`
- **Array**: `[1, 2, 3]`, `["a", "b", "c"]`

## Implemented Syntax

### Variables

Variables are declared using `let`:

```hls
let title = "My App"
let count = 42
let is_active = true
let items = [1, 2, 3]

element Header {
  text title
}
```

### State Variables

State variables are reactive and can trigger re-renders:

```hls
state counter = 0
state theme = "dark"

element Counter {
  text "Count: 0"
}
```

### Conditionals

Conditional rendering with `if`/`else`:

```hls
let user_logged_in = true

element Content {
  if user_logged_in {
    element Dashboard {
      text "Welcome!"
    }
  } else {
    element LoginForm {
      text "Please log in"
    }
  }
}
```

### Loops

Iterate over collections with `for...in`:

```hls
let items = [1, 2, 3]

element List {
  for item in items {
    element ListItem {
      text "Item"
    }
  }
}
```

### While Loops

Conditional loops with `while`:

```hls
let i = 0

while i < 5 {
  element Counter {
    text "Iteration"
  }
}
```

### Expressions

Full expression support with operators:

```hls
// Arithmetic
let sum = 1 + 2
let diff = 10 - 5
let product = 3 * 4
let quotient = 20 / 4
let remainder = 17 % 5

// Comparison
let is_greater = count > 5
let is_equal = name == "test"

// Logical
let both = a && b
let either = a || b
let negated = !flag

// Ternary
let result = condition ? "yes" : "no"

// String concatenation
let full_name = first ++ " " ++ last
```

### Built-in Functions

```hls
// Length of string or list
let size = len(items)

// Convert to string
let text = str(42)

// Convert to number
let num = num("42")

// Print to console (debug)
print("Debug message")

// Protocol operations
morph()              // Trigger protocol morphing
decoy()              // Inject decoy traffic
stream_latent([...]) // Stream latent vector
```

### Events

Emit events from within programs:

```hls
emit("button_clicked", { id: 42 })
```

### Functions

```hls
fn create_button(label, action) {
  element Button {
    text label
    on_click -> emit(action)
  }
}

element Toolbar {
  create_button("Save", "save_clicked")
  create_button("Cancel", "cancel_clicked")
}
```

### Reactive State

```hls
state counter = 0

element Counter {
  text "Count: " ++ str(counter)
  
  on_click -> {
    counter = counter + 1
    emit("counter_changed", { value: counter })
  }
}
```

## Compilation Process

### 1. Lexing

HLS source is tokenized into:
- Keywords: `element`, `attribute`, `text`, `on_click`, `emit`
- Identifiers: `App`, `Header`, `button_clicked`
- Literals: `"Hello"`, `42`, `true`
- Symbols: `{`, `}`, `->`, `,`

### 2. Parsing

Tokens are parsed into an Abstract Syntax Tree (AST):

```rust
pub enum AstNode {
    Element {
        tag: String,
        attributes: Vec<Attribute>,
        children: Vec<AstNode>,
        events: Vec<EventHandler>,
    },
    Text(String),
}

pub struct Attribute {
    pub key: String,
    pub value: Value,
}

pub struct EventHandler {
    pub event: String,
    pub action: Action,
}

pub enum Action {
    Emit { name: String, payload: Value },
}
```

### 3. Code Generation

The AST is traversed to generate HLB instructions:

```rust
fn generate(node: AstNode, parent_id: Option<u32>) -> Vec<Instruction> {
    match node {
        AstNode::Element { tag, attributes, children, events } => {
            let id = generate_id();
            let mut instructions = vec![
                Instruction::DefineElement { id, tag }
            ];
            
            for attr in attributes {
                instructions.push(Instruction::SetAttribute {
                    id,
                    key: attr.key,
                    value: attr.value.to_string(),
                });
            }
            
            if let Some(parent) = parent_id {
                instructions.push(Instruction::AddChild {
                    parent_id: parent,
                    child_id: id,
                });
            }
            
            for child in children {
                instructions.extend(generate(child, Some(id)));
            }
            
            for event in events {
                instructions.push(Instruction::EmitEvent {
                    name: event.action.name,
                    payload: event.action.payload,
                });
            }
            
            instructions
        }
        AstNode::Text(text) => {
            // Handle text nodes
            vec![]
        }
    }
}
```

## Example Programs

### Hello World

```hls
element App {
  element Header {
    text "Hello, SPINE!"
  }
}
```

**Compiled HLB**:
```rust
[
    Instruction::DefineElement { id: 1, tag: "App" },
    Instruction::DefineElement { id: 2, tag: "Header" },
    Instruction::SetAttribute { id: 2, key: "text", value: "Hello, SPINE!" },
    Instruction::AddChild { parent_id: 1, child_id: 2 },
]
```

### Interactive Form

```hls
element Form {
  attribute id "login-form"
  
  element Input {
    attribute type "text"
    attribute placeholder "Username"
    attribute id "username"
  }
  
  element Input {
    attribute type "password"
    attribute placeholder "Password"
    attribute id "password"
  }
  
  element Button {
    text "Login"
    on_click -> emit("login_attempted", {
      username_field: "username",
      password_field: "password"
    })
  }
}
```

### Dashboard Layout

```hls
element Dashboard {
  element Navbar {
    text "My Application"
    
    element Button {
      text "Logout"
      on_click -> emit("logout_requested")
    }
  }
  
  element Sidebar {
    element NavItem {
      text "Home"
      attribute href "/home"
    }
    
    element NavItem {
      text "Profile"
      attribute href "/profile"
    }
    
    element NavItem {
      text "Settings"
      attribute href "/settings"
    }
  }
  
  element MainContent {
    element Widget {
      text "Welcome back!"
    }
  }
}
```

## Best Practices

### 1. Use Semantic Element Names

```hls
// Good
element LoginForm {}
element UserProfile {}
element NavigationBar {}

// Avoid
element Div1 {}
element Container {}
element Thing {}
```

### 2. Keep Nesting Shallow

```hls
// Good
element App {
  element Header {}
  element Content {}
  element Footer {}
}

// Avoid
element App {
  element Wrapper {
    element Container {
      element InnerContainer {
        element Content {}
      }
    }
  }
}
```

### 3. Use Descriptive Event Names

```hls
// Good
on_click -> emit("user_profile_updated")
on_submit -> emit("form_validation_passed")

// Avoid
on_click -> emit("event1")
on_submit -> emit("submit")
```

### 4. Group Related Elements

```hls
element LoginSection {
  element LoginForm {}
  element ForgotPasswordLink {}
  element SignupPrompt {}
}
```

## Error Handling

The HLS compiler reports errors with line and column information:

```
Error: Unexpected token '}'
  --> program.hls:5:3
   |
 5 |   }
   |   ^ Expected element name or attribute
```

Common errors:
- Syntax errors (missing braces, invalid tokens)
- Undefined element references
- Type mismatches in attributes
- Circular element dependencies

## Implementation Status

- [x] Basic element definitions
- [x] Attribute assignment
- [x] Text content shorthand
- [x] Element nesting
- [x] Event handlers
- [x] Variables
- [x] Conditionals
- [x] Loops
- [x] Functions
- [x] State management
- [ ] Type checking
- [ ] Optimization passes

## References

- [SPINE Architecture](ARCHITECTURE.md)
- [HLB Instruction Set](src/spine-protocol/src/lib.rs)
- [Compiler Implementation](src/spine-compiler/src/lib.rs)
