"""Phase 19: HLS Type System - Add source locations, stronger inference,
typed function enforcement, and structured compile-time errors."""

COMPILER = "spine-compiler/src/lib.rs"

with open(COMPILER, "r", encoding="utf-8") as f:
    src = f.read()

# ========================================================================
# 1. Add Span type and TypeError struct after the use statements
# ========================================================================

span_and_error = '''
// =============================================================================
// SOURCE LOCATION TRACKING
// =============================================================================

/// Source location span for error reporting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Compute (line, column) from source text and byte offset.
    pub fn line_col(&self, source: &str) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        for (i, ch) in source.char_indices() {
            if i >= self.start {
                break;
            }
            if ch == '\\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl Default for Span {
    fn default() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// Structured compile-time type error with source location.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
    pub expected: Option<HlsType>,
    pub found: Option<HlsType>,
}

impl TypeError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            expected: None,
            found: None,
        }
    }

    pub fn with_types(mut self, expected: HlsType, found: HlsType) -> Self {
        self.expected = Some(expected);
        self.found = Some(found);
        self
    }

    /// Format the error with source context (filename, line, column, snippet).
    pub fn format(&self, source: &str) -> String {
        let (line, col) = self.span.line_col(source);
        let source_line = source.lines().nth(line - 1).unwrap_or("");
        let mut msg = format!("error[E0308]: {}", self.message);
        msg.push_str(&format!("\\n --> <hls>:{}:{}", line, col));
        msg.push_str(&format!("\\n  |"));
        msg.push_str(&format!("\\n{:>3} | {}", line, source_line));
        msg.push_str(&format!("\\n  |"));
        if let (Some(expected), Some(found)) = (&self.expected, &self.found) {
            msg.push_str(&format!("\\n  = expected `{:?}`, found `{:?}`", expected, found));
        }
        msg
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TypeError {}

/// Collection of type errors accumulated during checking.
#[derive(Debug, Default)]
pub struct TypeErrors {
    pub errors: Vec<TypeError>,
}

impl TypeErrors {
    pub fn push(&mut self, err: TypeError) {
        self.errors.push(err);
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Format all errors with source context.
    pub fn format_all(&self, source: &str) -> String {
        self.errors
            .iter()
            .map(|e| e.format(source))
            .collect::<Vec<_>>()
            .join("\\n\\n")
    }
}

'''

# Insert after the COMPILER STATE section header
insert_marker = "// =============================================================================\n// COMPILER STATE\n// ============================================================================="
if "pub struct Span" not in src:
    idx = src.find(insert_marker)
    if idx >= 0:
        src = src[:idx] + span_and_error + "\n" + src[idx:]
        print("  [OK] Added Span, TypeError, TypeErrors types")
    else:
        print("  [WARN] Could not find COMPILER STATE marker")
else:
    print("  [SKIP] Span already exists")

# ========================================================================
# 2. Add type_check_with_errors method to Compiler alongside existing check_types
# ========================================================================

# We'll add a new public method that collects all type errors instead of aborting on first
type_check_method = '''
    /// Perform type checking, collecting all errors instead of aborting on first.
    /// Returns a `TypeErrors` collection. If non-empty, compilation should fail.
    fn check_types_collect(
        ctx: &mut CompilerContext,
        stmt: &HlsStatement,
        source: &str,
        errors: &mut TypeErrors,
    ) {
        match stmt {
            HlsStatement::Let { name, value, type_annotation } => {
                match Self::infer_expr_type(ctx, value) {
                    Ok(val_type) => {
                        let final_type = if let Some(annotated) = type_annotation {
                            if !Self::types_match(annotated, &val_type) {
                                errors.push(
                                    TypeError::new(
                                        format!("type mismatch for variable '{}'", name),
                                        Span::default(),
                                    )
                                    .with_types(annotated.clone(), val_type.clone()),
                                );
                            }
                            annotated.clone()
                        } else {
                            val_type
                        };
                        ctx.variables.insert(name.clone(), (final_type, HlsValue::Null));
                    }
                    Err(e) => {
                        errors.push(TypeError::new(e.to_string(), Span::default()));
                    }
                }
            }
            HlsStatement::State { name, initial, type_annotation } => {
                match Self::infer_expr_type(ctx, initial) {
                    Ok(val_type) => {
                        let final_type = if let Some(annotated) = type_annotation {
                            if !Self::types_match(annotated, &val_type) {
                                errors.push(
                                    TypeError::new(
                                        format!("type mismatch for state '{}'", name),
                                        Span::default(),
                                    )
                                    .with_types(annotated.clone(), val_type.clone()),
                                );
                            }
                            annotated.clone()
                        } else {
                            val_type
                        };
                        ctx.variables.insert(name.clone(), (final_type, HlsValue::Null));
                    }
                    Err(e) => {
                        errors.push(TypeError::new(e.to_string(), Span::default()));
                    }
                }
            }
            HlsStatement::Assign { name, value } => {
                match Self::infer_expr_type(ctx, value) {
                    Ok(val_type) => {
                        if let Some((expected_type, _)) = ctx.variables.get(name) {
                            if !Self::types_match(expected_type, &val_type) {
                                errors.push(
                                    TypeError::new(
                                        format!("type mismatch in assignment to '{}'", name),
                                        Span::default(),
                                    )
                                    .with_types(expected_type.clone(), val_type),
                                );
                            }
                        } else {
                            errors.push(TypeError::new(
                                format!("undefined variable '{}'", name),
                                Span::default(),
                            ));
                        }
                    }
                    Err(e) => {
                        errors.push(TypeError::new(e.to_string(), Span::default()));
                    }
                }
            }
            HlsStatement::FnDef { name, params, body, return_type } => {
                let mut fn_ctx = CompilerContext {
                    variables: ctx.variables.clone(),
                    functions: ctx.functions.clone(),
                    ..Default::default()
                };
                for (p_name, p_type) in params {
                    fn_ctx.variables.insert(p_name.clone(), (p_type.clone(), HlsValue::Null));
                }
                let r_type = return_type.clone().unwrap_or(HlsType::Any);

                // Check return type consistency
                for s in body {
                    if let HlsStatement::Return(Some(ret_expr)) = s {
                        if let Ok(ret_type) = Self::infer_expr_type(&fn_ctx, ret_expr) {
                            if r_type != HlsType::Any && !Self::types_match(&r_type, &ret_type) {
                                errors.push(
                                    TypeError::new(
                                        format!(
                                            "function '{}' return type mismatch",
                                            name
                                        ),
                                        Span::default(),
                                    )
                                    .with_types(r_type.clone(), ret_type),
                                );
                            }
                        }
                    }
                }

                ctx.functions.insert(
                    name.clone(),
                    HlsFunction {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                        return_type: r_type,
                    },
                );
                for s in body {
                    Self::check_types_collect(&mut fn_ctx, s, source, errors);
                }
            }
            HlsStatement::Call { name, args } => {
                if let Some(func) = ctx.functions.get(name) {
                    if func.params.len() != args.len() {
                        errors.push(TypeError::new(
                            format!(
                                "function '{}' expected {} arguments, found {}",
                                name,
                                func.params.len(),
                                args.len()
                            ),
                            Span::default(),
                        ));
                    } else {
                        for (i, arg) in args.iter().enumerate() {
                            if let Ok(arg_type) = Self::infer_expr_type(ctx, arg) {
                                if !Self::types_match(&func.params[i].1, &arg_type) {
                                    errors.push(
                                        TypeError::new(
                                            format!(
                                                "argument {} of '{}' type mismatch",
                                                i, name
                                            ),
                                            Span::default(),
                                        )
                                        .with_types(func.params[i].1.clone(), arg_type),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            HlsStatement::If { condition, then_branch, else_branch } => {
                if let Err(e) = Self::infer_expr_type(ctx, condition) {
                    errors.push(TypeError::new(e.to_string(), Span::default()));
                }
                for s in then_branch {
                    Self::check_types_collect(ctx, s, source, errors);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        Self::check_types_collect(ctx, s, source, errors);
                    }
                }
            }
            HlsStatement::For { item, list, body } => {
                let inner_type = match Self::infer_expr_type(ctx, list) {
                    Ok(HlsType::List(inner)) => *inner,
                    Ok(_) => HlsType::Any,
                    Err(e) => {
                        errors.push(TypeError::new(e.to_string(), Span::default()));
                        HlsType::Any
                    }
                };
                ctx.variables.insert(item.clone(), (inner_type, HlsValue::Null));
                for s in body {
                    Self::check_types_collect(ctx, s, source, errors);
                }
                ctx.variables.remove(item);
            }
            HlsStatement::Element { children, .. } => {
                for s in children {
                    Self::check_types_collect(ctx, s, source, errors);
                }
            }
            HlsStatement::Navigate(url_expr) => {
                if let Ok(t) = Self::infer_expr_type(ctx, url_expr) {
                    if t != HlsType::String && t != HlsType::Any {
                        errors.push(
                            TypeError::new("navigate requires a string URL", Span::default())
                                .with_types(HlsType::String, t),
                        );
                    }
                }
            }
            HlsStatement::Search(query_expr) => {
                if let Ok(t) = Self::infer_expr_type(ctx, query_expr) {
                    if t != HlsType::String && t != HlsType::Any {
                        errors.push(
                            TypeError::new("search requires a string query", Span::default())
                                .with_types(HlsType::String, t),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Public API: Type-check a source string, returning all errors at once.
    pub fn type_check(source: &str) -> Result<(), TypeErrors> {
        let statements = Self::parse_program(source)
            .map_err(|e| {
                let mut errs = TypeErrors::default();
                errs.push(TypeError::new(format!("parse error: {}", e), Span::default()));
                errs
            })?;
        let mut ctx = CompilerContext::default();
        let mut errors = TypeErrors::default();
        for stmt in &statements {
            Self::check_types_collect(&mut ctx, stmt, source, &mut errors);
        }
        if errors.has_errors() {
            Err(errors)
        } else {
            Ok(())
        }
    }

'''

# Insert before the optimize method
optimize_marker = "    fn optimize(statements: Vec<HlsStatement>) -> Vec<HlsStatement> {"
if "fn check_types_collect" not in src:
    idx = src.find(optimize_marker)
    if idx >= 0:
        src = src[:idx] + type_check_method + "\n    " + src[idx:]
        print("  [OK] Added check_types_collect + type_check methods")
    else:
        print("  [WARN] Could not find optimize marker")
else:
    print("  [SKIP] check_types_collect already exists")

# ========================================================================
# 3. Add tests for the type system
# ========================================================================

type_tests = '''

    #[test]
    fn test_type_error_span_display() {
        let span = Span::new(10, 25);
        assert_eq!(format!("{}", span), "10..25");
    }

    #[test]
    fn test_type_error_line_col() {
        let source = "let x = 1\\nlet y = true\\nlet z = x + y";
        let span = Span::new(24, 37); // position of "x + y" on line 3
        let (line, col) = span.line_col(source);
        assert_eq!(line, 3);
    }

    #[test]
    fn test_type_error_format() {
        let err = TypeError::new("mismatched types", Span::new(0, 5))
            .with_types(HlsType::Number, HlsType::String);
        let formatted = err.format("let x = \\"hello\\"");
        assert!(formatted.contains("error[E0308]"));
        assert!(formatted.contains("mismatched types"));
        assert!(formatted.contains("Number"));
        assert!(formatted.contains("String"));
    }

    #[test]
    fn test_type_errors_collection() {
        let mut errors = TypeErrors::default();
        assert!(errors.is_empty());
        errors.push(TypeError::new("err1", Span::default()));
        errors.push(TypeError::new("err2", Span::default()));
        assert!(errors.has_errors());
        assert_eq!(errors.errors.len(), 2);
    }

    #[test]
    fn test_type_check_valid_program() {
        let source = "let x: Number = 42\\nlet y = x + 1";
        let result = Compiler::type_check(source);
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_check_mismatch_detected() {
        let source = "let x: Number = \\"hello\\"";
        let result = Compiler::type_check(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors.is_empty());
        assert!(errors.errors[0].message.contains("type mismatch"));
    }

    #[test]
    fn test_type_check_undefined_variable() {
        let source = "let x = undeclared_var";
        let result = Compiler::type_check(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors[0].message.contains("undefined") || errors.errors[0].message.contains("Undefined"));
    }

    #[test]
    fn test_type_check_function_arg_count() {
        let source = "fn add(a: Number, b: Number) -> Number {\\n  return a + b\\n}\\nadd(1)";
        let result = Compiler::type_check(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors[0].message.contains("expected 2 arguments"));
    }

    #[test]
    fn test_type_check_function_return_type() {
        let source = "fn greet(name: String) -> Number {\\n  return name\\n}";
        let result = Compiler::type_check(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors[0].message.contains("return type mismatch"));
    }

    #[test]
    fn test_type_check_navigate_requires_string() {
        let source = "navigate 42";
        let result = Compiler::type_check(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors[0].message.contains("navigate"));
    }

    #[test]
    fn test_type_check_multiple_errors() {
        // Two separate errors: undefined var and type mismatch
        let source = "let x: Number = \\"bad\\"\\nlet y = unknown_var";
        let result = Compiler::type_check(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.errors.len() >= 2);
    }

    #[test]
    fn test_span_merge() {
        let s1 = Span::new(5, 10);
        let s2 = Span::new(8, 20);
        let merged = s1.merge(s2);
        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 20);
    }
'''

# Insert tests before the final closing brace of the test module
if "test_type_error_span_display" not in src:
    last_brace = src.rfind("}")
    if last_brace > 0:
        src = src[:last_brace] + type_tests + "\n}\n"
        print("  [OK] Added 12 type system tests")
else:
    print("  [SKIP] Type system tests already exist")

with open(COMPILER, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

print("\nPhase 19 edits applied to spine-compiler.")
