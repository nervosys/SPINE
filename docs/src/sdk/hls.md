# HLS Scripting

HLS (SPINE Scripting) is a domain-specific language for server-side execution.

## Execute via Agent

```rust
let result = client.execute_hls("let x = 42 * 2; x").await?;
println!("Result: {:?}", result);
```

## Language Features

### Variables

```
let x = 42;
let name = "SPINE";
let flag = true;
```

### Expressions

```
let sum = 1 + 2 * 3;     // 7
let eq = 10 == 10;        // true
let neg = -5;
```

### Conditionals

```
let grade = if score >= 90 { "A" } else if score >= 80 { "B" } else { "C" };
```

### Loops

```
let sum = 0;
let i = 1;
while i <= 100 {
    sum = sum + i;
    i = i + 1;
}
```

### Functions

```
fn factorial(n) {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}
let result = factorial(10);
```

## Offline Compilation

```rust
use spine_agent::Compiler;

match Compiler::compile("let x = 1 + 2; x") {
    Ok(binary) => {
        println!("Instructions: {}", binary.instructions.len());
        println!("Exports: {:?}", binary.exported_functions.keys());
    }
    Err(e) => eprintln!("Compile error: {}", e),
}
```

## SpineBinary Format

| Field                | Type                     | Description                   |
| -------------------- | ------------------------ | ----------------------------- |
| `instructions`       | `Vec<Instruction>`       | Compiled bytecode             |
| `data`               | `Vec<u8>`                | Static data section           |
| `render_start`       | `usize`                  | Entry point for rendering     |
| `exported_functions` | `HashMap<String, usize>` | Named function entries        |
| `capabilities`       | `Vec<String>`            | Required runtime capabilities |
