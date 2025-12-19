use nom::{
    bytes::complete::{tag, take_while1},
    character::complete::{multispace0, multispace1, digit1, char as nom_char},
    sequence::{preceded, tuple, terminated},
    branch::alt,
    combinator::{opt, map, recognize, value},
    multi::{many0, separated_list0},
    IResult,
};
use hyperlight_protocol::{HyperlightBinary, Instruction};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

// =============================================================================
// COMPILER STATE
// =============================================================================

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

fn next_id() -> u32 {
    NEXT_ID.fetch_add(1, Ordering::SeqCst)
}

/// Compiler context for tracking state during compilation
#[derive(Default)]
pub struct CompilerContext {
    /// Variable bindings: name -> (type, initial_value)
    pub variables: HashMap<String, (HlsType, HlsValue)>,
    /// Function definitions
    pub functions: HashMap<String, HlsFunction>,
    /// Current scope depth
    pub scope_depth: u32,
    /// Generated instructions
    pub instructions: Vec<Instruction>,
    /// String constants pool
    pub string_pool: Vec<String>,
    /// Parent element stack for nesting
    pub element_stack: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HlsType {
    String,
    Number,
    Boolean,
    Element,
    List(Box<HlsType>),
    Any,
}

#[derive(Debug, Clone)]
pub enum HlsValue {
    String(String),
    Number(f64),
    Boolean(bool),
    List(Vec<HlsValue>),
    Null,
}

#[derive(Debug, Clone)]
pub struct HlsFunction {
    pub name: String,
    pub params: Vec<(String, HlsType)>,
    pub body: Vec<HlsStatement>,
    pub return_type: HlsType,
}

// =============================================================================
// AST TYPES
// =============================================================================

#[derive(Debug, Clone)]
pub enum HlsStatement {
    /// Variable declaration: let name = value
    Let { name: String, value: HlsExpr, type_annotation: Option<HlsType> },
    /// State declaration: state name = value
    State { name: String, initial: HlsExpr, type_annotation: Option<HlsType> },
    /// Function definition: fn name(args) { body }
    FnDef { name: String, params: Vec<(String, HlsType)>, body: Vec<HlsStatement>, return_type: Option<HlsType> },
    /// Element definition
    Element { tag: String, attributes: Vec<(String, HlsExpr)>, children: Vec<HlsStatement>, events: Vec<HlsEvent> },
    /// Conditional: if condition { ... } else { ... }
    If { condition: HlsExpr, then_branch: Vec<HlsStatement>, else_branch: Option<Vec<HlsStatement>> },
    /// Loop: for item in list { ... }
    For { item: String, list: HlsExpr, body: Vec<HlsStatement> },
    /// While loop: while condition { ... }
    While { condition: HlsExpr, body: Vec<HlsStatement> },
    /// Function call
    Call { name: String, args: Vec<HlsExpr> },
    /// Assignment: name = value
    Assign { name: String, value: HlsExpr },
    /// Text content
    Text(HlsExpr),
    /// Emit event
    Emit { event: String, payload: HlsExpr },
    /// Return statement
    Return(Option<HlsExpr>),
    /// Comment (ignored in codegen)
    Comment(String),
}

#[derive(Debug, Clone)]
pub enum HlsExpr {
    /// String literal
    StringLit(String),
    /// Number literal
    NumberLit(f64),
    /// Boolean literal
    BoolLit(bool),
    /// Variable reference
    Var(String),
    /// Binary operation
    BinOp { left: Box<HlsExpr>, op: BinOperator, right: Box<HlsExpr> },
    /// Unary operation
    UnaryOp { op: UnaryOperator, expr: Box<HlsExpr> },
    /// Function call expression
    Call { name: String, args: Vec<HlsExpr> },
    /// Property access: obj.prop
    Property { object: Box<HlsExpr>, property: String },
    /// Index access: arr[idx]
    Index { object: Box<HlsExpr>, index: Box<HlsExpr> },
    /// List literal: [a, b, c]
    List(Vec<HlsExpr>),
    /// Object literal: { key: value, ... }
    Object(Vec<(String, HlsExpr)>),
    /// Ternary: condition ? then : else
    Ternary { condition: Box<HlsExpr>, then_expr: Box<HlsExpr>, else_expr: Box<HlsExpr> },
}

#[derive(Debug, Clone)]
pub struct HlsEvent {
    pub event_type: String,
    pub handler: Vec<HlsStatement>,
}

#[derive(Debug, Clone, Copy)]
pub enum BinOperator {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    Concat,
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOperator {
    Not, Neg,
}

// =============================================================================
// COMPILER
// =============================================================================

pub struct Compiler;

impl Compiler {
    /// Compile HLS source to HLB binary
    pub fn compile(source: &str) -> anyhow::Result<HyperlightBinary> {
        NEXT_ID.store(1, Ordering::SeqCst);
        let mut ctx = CompilerContext::default();
        
        let (_, statements) = parse_program(source)
            .map_err(|e| anyhow::anyhow!("Parse error: {:?}", e))?;
        
        for stmt in statements {
            Self::compile_statement(&mut ctx, &stmt)?;
        }
        
        Ok(HyperlightBinary {
            instructions: ctx.instructions,
            data: ctx.string_pool.join("\0").into_bytes(),
        })
    }
    
    fn compile_statement(ctx: &mut CompilerContext, stmt: &HlsStatement) -> anyhow::Result<()> {
        match stmt {
            HlsStatement::Element { tag, attributes, children, events } => {
                let id = next_id();
                ctx.instructions.push(Instruction::DefineElement {
                    id,
                    tag: tag.clone(),
                });
                
                // Compile attributes
                for (key, value) in attributes {
                    let value_str = Self::eval_expr_to_string(ctx, value);
                    ctx.instructions.push(Instruction::SetAttribute {
                        id,
                        key: key.clone(),
                        value: value_str,
                    });
                }
                
                // Add to parent if exists
                if let Some(&parent_id) = ctx.element_stack.last() {
                    ctx.instructions.push(Instruction::AddChild {
                        parent_id,
                        child_id: id,
                    });
                }
                
                // Push onto stack for children
                ctx.element_stack.push(id);
                
                // Compile children
                for child in children {
                    Self::compile_statement(ctx, child)?;
                }
                
                // Compile events
                for event in events {
                    for handler_stmt in &event.handler {
                        if let HlsStatement::Emit { event: evt_name, payload } = handler_stmt {
                            let payload_json = Self::expr_to_json(ctx, payload);
                            ctx.instructions.push(Instruction::EmitEvent {
                                name: evt_name.clone(),
                                payload: payload_json,
                            });
                        }
                    }
                }
                
                ctx.element_stack.pop();
            }
            
            HlsStatement::Let { name, value } => {
                let val = Self::eval_expr(ctx, value);
                let typ = Self::infer_type(&val);
                ctx.variables.insert(name.clone(), (typ, val));
            }
            
            HlsStatement::State { name, initial } => {
                let val = Self::eval_expr(ctx, initial);
                let typ = Self::infer_type(&val);
                ctx.variables.insert(name.clone(), (typ, val));
                // State variables can trigger re-renders (future: emit state instruction)
            }
            
            HlsStatement::If { condition, then_branch, else_branch } => {
                let cond_val = Self::eval_expr(ctx, condition);
                let is_true = match cond_val {
                    HlsValue::Boolean(b) => b,
                    HlsValue::Number(n) => n != 0.0,
                    HlsValue::String(s) => !s.is_empty(),
                    _ => false,
                };
                
                if is_true {
                    for stmt in then_branch {
                        Self::compile_statement(ctx, stmt)?;
                    }
                } else if let Some(else_stmts) = else_branch {
                    for stmt in else_stmts {
                        Self::compile_statement(ctx, stmt)?;
                    }
                }
            }
            
            HlsStatement::For { item, list, body } => {
                let list_val = Self::eval_expr(ctx, list);
                if let HlsValue::List(items) = list_val {
                    for item_val in items {
                        let typ = Self::infer_type(&item_val);
                        ctx.variables.insert(item.clone(), (typ, item_val));
                        for stmt in body {
                            Self::compile_statement(ctx, stmt)?;
                        }
                    }
                    ctx.variables.remove(item);
                }
            }
            
            HlsStatement::While { condition, body } => {
                let mut iterations = 0;
                const MAX_ITERATIONS: usize = 1000;
                
                loop {
                    let cond_val = Self::eval_expr(ctx, condition);
                    let is_true = match cond_val {
                        HlsValue::Boolean(b) => b,
                        HlsValue::Number(n) => n != 0.0,
                        _ => false,
                    };
                    
                    if !is_true || iterations >= MAX_ITERATIONS {
                        break;
                    }
                    
                    for stmt in body {
                        Self::compile_statement(ctx, stmt)?;
                    }
                    iterations += 1;
                }
            }
            
            HlsStatement::Text(expr) => {
                let text = Self::eval_expr_to_string(ctx, expr);
                let id = next_id();
                ctx.instructions.push(Instruction::DefineElement {
                    id,
                    tag: "text".to_string(),
                });
                ctx.instructions.push(Instruction::SetAttribute {
                    id,
                    key: "content".to_string(),
                    value: text,
                });
                if let Some(&parent_id) = ctx.element_stack.last() {
                    ctx.instructions.push(Instruction::AddChild {
                        parent_id,
                        child_id: id,
                    });
                }
            }
            
            HlsStatement::Emit { event, payload } => {
                let payload_json = Self::expr_to_json(ctx, payload);
                ctx.instructions.push(Instruction::EmitEvent {
                    name: event.clone(),
                    payload: payload_json,
                });
            }
            
            HlsStatement::Assign { name, value } => {
                let val = Self::eval_expr(ctx, value);
                let typ = Self::infer_type(&val);
                ctx.variables.insert(name.clone(), (typ, val));
            }
            
            HlsStatement::Call { name, args } => {
                // Built-in functions
                match name.as_str() {
                    "print" => {
                        for arg in args {
                            let val = Self::eval_expr_to_string(ctx, arg);
                            println!("[HLS] {}", val);
                        }
                    }
                    "morph" => {
                        // Trigger protocol morphing
                        let seed = if let Some(arg) = args.first() {
                            match Self::eval_expr(ctx, arg) {
                                HlsValue::Number(n) => n as u64,
                                _ => 0,
                            }
                        } else {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_nanos() as u64)
                                .unwrap_or(0)
                        };
                        ctx.instructions.push(Instruction::MorphProtocol { seed });
                    }
                    "decoy" => {
                        let noise: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
                        ctx.instructions.push(Instruction::Decoy { noise });
                    }
                    "stream_latent" => {
                        if let Some(arg) = args.first() {
                            if let HlsValue::List(items) = Self::eval_expr(ctx, arg) {
                                let vector: Vec<f32> = items.iter().filter_map(|v| {
                                    if let HlsValue::Number(n) = v { Some(*n as f32) } else { None }
                                }).collect();
                                ctx.instructions.push(Instruction::StreamLatent { vector });
                            }
                        }
                    }
                    _ => {}
                }
            }
            
            HlsStatement::Return(_) | HlsStatement::Comment(_) => {}
        }
        Ok(())
    }
    
    fn eval_expr(ctx: &CompilerContext, expr: &HlsExpr) -> HlsValue {
        match expr {
            HlsExpr::StringLit(s) => HlsValue::String(s.clone()),
            HlsExpr::NumberLit(n) => HlsValue::Number(*n),
            HlsExpr::BoolLit(b) => HlsValue::Boolean(*b),
            HlsExpr::Var(name) => {
                ctx.variables.get(name)
                    .map(|(_, v)| v.clone())
                    .unwrap_or(HlsValue::Null)
            }
            HlsExpr::BinOp { left, op, right } => {
                let l = Self::eval_expr(ctx, left);
                let r = Self::eval_expr(ctx, right);
                Self::eval_binop(l, *op, r)
            }
            HlsExpr::UnaryOp { op, expr } => {
                let v = Self::eval_expr(ctx, expr);
                match op {
                    UnaryOperator::Not => match v {
                        HlsValue::Boolean(b) => HlsValue::Boolean(!b),
                        _ => HlsValue::Boolean(false),
                    },
                    UnaryOperator::Neg => match v {
                        HlsValue::Number(n) => HlsValue::Number(-n),
                        _ => HlsValue::Number(0.0),
                    },
                }
            }
            HlsExpr::List(items) => {
                HlsValue::List(items.iter().map(|e| Self::eval_expr(ctx, e)).collect())
            }
            HlsExpr::Ternary { condition, then_expr, else_expr } => {
                let cond = Self::eval_expr(ctx, condition);
                let is_true = match cond {
                    HlsValue::Boolean(b) => b,
                    HlsValue::Number(n) => n != 0.0,
                    _ => false,
                };
                if is_true {
                    Self::eval_expr(ctx, then_expr)
                } else {
                    Self::eval_expr(ctx, else_expr)
                }
            }
            HlsExpr::Property { object, property } => {
                // Simple property access for now
                let _obj = Self::eval_expr(ctx, object);
                HlsValue::String(format!("{{{}.{}}}", "obj", property))
            }
            HlsExpr::Index { object, index } => {
                let obj = Self::eval_expr(ctx, object);
                let idx = Self::eval_expr(ctx, index);
                match (obj, idx) {
                    (HlsValue::List(items), HlsValue::Number(n)) => {
                        items.get(n as usize).cloned().unwrap_or(HlsValue::Null)
                    }
                    _ => HlsValue::Null,
                }
            }
            HlsExpr::Object(pairs) => {
                // Convert to JSON-like representation
                let obj_str = pairs.iter()
                    .map(|(k, v)| format!("{}: {}", k, Self::eval_expr_to_string(ctx, v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                HlsValue::String(format!("{{{}}}", obj_str))
            }
            HlsExpr::Call { name, args } => {
                // Built-in expression functions
                match name.as_str() {
                    "len" => {
                        if let Some(arg) = args.first() {
                            match Self::eval_expr(ctx, arg) {
                                HlsValue::List(items) => HlsValue::Number(items.len() as f64),
                                HlsValue::String(s) => HlsValue::Number(s.len() as f64),
                                _ => HlsValue::Number(0.0),
                            }
                        } else {
                            HlsValue::Number(0.0)
                        }
                    }
                    "str" => {
                        if let Some(arg) = args.first() {
                            HlsValue::String(Self::eval_expr_to_string(ctx, arg))
                        } else {
                            HlsValue::String(String::new())
                        }
                    }
                    "num" => {
                        if let Some(arg) = args.first() {
                            match Self::eval_expr(ctx, arg) {
                                HlsValue::Number(n) => HlsValue::Number(n),
                                HlsValue::String(s) => HlsValue::Number(s.parse().unwrap_or(0.0)),
                                HlsValue::Boolean(b) => HlsValue::Number(if b { 1.0 } else { 0.0 }),
                                _ => HlsValue::Number(0.0),
                            }
                        } else {
                            HlsValue::Number(0.0)
                        }
                    }
                    _ => HlsValue::Null,
                }
            }
        }
    }
    
    fn eval_binop(left: HlsValue, op: BinOperator, right: HlsValue) -> HlsValue {
        match op {
            BinOperator::Add => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Number(l + r),
                (HlsValue::String(l), HlsValue::String(r)) => HlsValue::String(format!("{}{}", l, r)),
                _ => HlsValue::Null,
            },
            BinOperator::Sub => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Number(l - r),
                _ => HlsValue::Null,
            },
            BinOperator::Mul => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Number(l * r),
                _ => HlsValue::Null,
            },
            BinOperator::Div => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) if r != 0.0 => HlsValue::Number(l / r),
                _ => HlsValue::Null,
            },
            BinOperator::Mod => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) if r != 0.0 => HlsValue::Number(l % r),
                _ => HlsValue::Null,
            },
            BinOperator::Eq => HlsValue::Boolean(Self::values_equal(&left, &right)),
            BinOperator::Ne => HlsValue::Boolean(!Self::values_equal(&left, &right)),
            BinOperator::Lt => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l < r),
                _ => HlsValue::Boolean(false),
            },
            BinOperator::Le => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l <= r),
                _ => HlsValue::Boolean(false),
            },
            BinOperator::Gt => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l > r),
                _ => HlsValue::Boolean(false),
            },
            BinOperator::Ge => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l >= r),
                _ => HlsValue::Boolean(false),
            },
            BinOperator::And => match (left, right) {
                (HlsValue::Boolean(l), HlsValue::Boolean(r)) => HlsValue::Boolean(l && r),
                _ => HlsValue::Boolean(false),
            },
            BinOperator::Or => match (left, right) {
                (HlsValue::Boolean(l), HlsValue::Boolean(r)) => HlsValue::Boolean(l || r),
                _ => HlsValue::Boolean(false),
            },
            BinOperator::Concat => {
                let l = Self::value_to_string(&left);
                let r = Self::value_to_string(&right);
                HlsValue::String(format!("{}{}", l, r))
            }
        }
    }
    
    fn values_equal(a: &HlsValue, b: &HlsValue) -> bool {
        match (a, b) {
            (HlsValue::String(l), HlsValue::String(r)) => l == r,
            (HlsValue::Number(l), HlsValue::Number(r)) => (l - r).abs() < f64::EPSILON,
            (HlsValue::Boolean(l), HlsValue::Boolean(r)) => l == r,
            (HlsValue::Null, HlsValue::Null) => true,
            _ => false,
        }
    }
    
    fn eval_expr_to_string(ctx: &CompilerContext, expr: &HlsExpr) -> String {
        Self::value_to_string(&Self::eval_expr(ctx, expr))
    }
    
    fn value_to_string(val: &HlsValue) -> String {
        match val {
            HlsValue::String(s) => s.clone(),
            HlsValue::Number(n) => n.to_string(),
            HlsValue::Boolean(b) => b.to_string(),
            HlsValue::List(items) => {
                let items_str: Vec<String> = items.iter().map(Self::value_to_string).collect();
                format!("[{}]", items_str.join(", "))
            }
            HlsValue::Null => "null".to_string(),
        }
    }
    
    fn infer_type(val: &HlsValue) -> HlsType {
        match val {
            HlsValue::String(_) => HlsType::String,
            HlsValue::Number(_) => HlsType::Number,
            HlsValue::Boolean(_) => HlsType::Boolean,
            HlsValue::List(_) => HlsType::List(Box::new(HlsType::Any)),
            HlsValue::Null => HlsType::Any,
        }
    }
    
    fn expr_to_json(ctx: &CompilerContext, expr: &HlsExpr) -> serde_json::Value {
        match Self::eval_expr(ctx, expr) {
            HlsValue::String(s) => serde_json::Value::String(s),
            HlsValue::Number(n) => serde_json::json!(n),
            HlsValue::Boolean(b) => serde_json::Value::Bool(b),
            HlsValue::List(items) => {
                let json_items: Vec<serde_json::Value> = items.iter()
                    .map(|v| match v {
                        HlsValue::String(s) => serde_json::Value::String(s.clone()),
                        HlsValue::Number(n) => serde_json::json!(n),
                        HlsValue::Boolean(b) => serde_json::Value::Bool(*b),
                        _ => serde_json::Value::Null,
                    })
                    .collect();
                serde_json::Value::Array(json_items)
            }
            HlsValue::Null => serde_json::Value::Null,
        }
    }
}

// =============================================================================
// PARSER
// =============================================================================

fn parse_program(input: &str) -> IResult<&str, Vec<HlsStatement>> {
    many0(preceded(multispace0, parse_statement))(input)
}

fn parse_statement(input: &str) -> IResult<&str, HlsStatement> {
    alt((
        parse_element_stmt,
        parse_let_stmt,
        parse_state_stmt,
        parse_if_stmt,
        parse_for_stmt,
        parse_while_stmt,
        parse_text_stmt,
        parse_emit_stmt,
        parse_comment,
    ))(input)
}

fn parse_element_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("element")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, tag_name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('{')(input)?;
    let (input, _) = multispace0(input)?;
    
    // Parse children and attributes inside the element
    let (input, children) = many0(preceded(multispace0, parse_element_child))(input)?;
    
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('}')(input)?;
    
    Ok((input, HlsStatement::Element {
        tag: tag_name.to_string(),
        attributes: Vec::new(),
        children,
        events: Vec::new(),
    }))
}

fn parse_element_child(input: &str) -> IResult<&str, HlsStatement> {
    alt((
        parse_element_stmt,
        parse_text_stmt,
        parse_if_stmt,
        parse_for_stmt,
    ))(input)
}

fn parse_let_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("let")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('=')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_expr(input)?;
    
    Ok((input, HlsStatement::Let {
        name: name.to_string(),
        value,
    }))
}

fn parse_state_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("state")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('=')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, initial) = parse_expr(input)?;
    
    Ok((input, HlsStatement::State {
        name: name.to_string(),
        initial,
    }))
}

fn parse_if_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("if")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, condition) = parse_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('{')(input)?;
    let (input, then_branch) = many0(preceded(multispace0, parse_statement))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('}')(input)?;
    
    // Optional else branch
    let (input, else_branch) = opt(preceded(
        tuple((multispace0, tag("else"), multispace0, nom_char('{'))),
        terminated(
            many0(preceded(multispace0, parse_statement)),
            preceded(multispace0, nom_char('}'))
        )
    ))(input)?;
    
    Ok((input, HlsStatement::If {
        condition,
        then_branch,
        else_branch,
    }))
}

fn parse_for_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("for")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, item) = parse_identifier(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag("in")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, list) = parse_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('{')(input)?;
    let (input, body) = many0(preceded(multispace0, parse_statement))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('}')(input)?;
    
    Ok((input, HlsStatement::For {
        item: item.to_string(),
        list,
        body,
    }))
}

fn parse_while_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("while")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, condition) = parse_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('{')(input)?;
    let (input, body) = many0(preceded(multispace0, parse_statement))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('}')(input)?;
    
    Ok((input, HlsStatement::While {
        condition,
        body,
    }))
}

fn parse_text_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("text")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, expr) = parse_expr(input)?;
    
    Ok((input, HlsStatement::Text(expr)))
}

fn parse_emit_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("emit")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, event) = parse_string_lit(input)?;
    let (input, _) = multispace0(input)?;
    let (input, payload) = opt(preceded(
        tuple((nom_char(','), multispace0)),
        parse_expr
    ))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char(')')(input)?;
    
    Ok((input, HlsStatement::Emit {
        event: event.to_string(),
        payload: payload.unwrap_or(HlsExpr::Object(Vec::new())),
    }))
}

fn parse_comment(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("//")(input)?;
    let (input, comment) = take_while1(|c| c != '\n')(input)?;
    
    Ok((input, HlsStatement::Comment(comment.to_string())))
}

// Expression parser
fn parse_expr(input: &str) -> IResult<&str, HlsExpr> {
    parse_or_expr(input)
}

fn parse_or_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_and_expr(input)?;
    let (input, rights) = many0(preceded(
        tuple((multispace0, tag("||"), multispace0)),
        parse_and_expr
    ))(input)?;
    
    Ok((input, rights.into_iter().fold(left, |acc, right| {
        HlsExpr::BinOp { left: Box::new(acc), op: BinOperator::Or, right: Box::new(right) }
    })))
}

fn parse_and_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_comparison_expr(input)?;
    let (input, rights) = many0(preceded(
        tuple((multispace0, tag("&&"), multispace0)),
        parse_comparison_expr
    ))(input)?;
    
    Ok((input, rights.into_iter().fold(left, |acc, right| {
        HlsExpr::BinOp { left: Box::new(acc), op: BinOperator::And, right: Box::new(right) }
    })))
}

fn parse_comparison_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_additive_expr(input)?;
    let (input, op_right) = opt(tuple((
        multispace0,
        alt((
            value(BinOperator::Eq, tag("==")),
            value(BinOperator::Ne, tag("!=")),
            value(BinOperator::Le, tag("<=")),
            value(BinOperator::Ge, tag(">=")),
            value(BinOperator::Lt, tag("<")),
            value(BinOperator::Gt, tag(">")),
        )),
        multispace0,
        parse_additive_expr
    )))(input)?;
    
    Ok((input, match op_right {
        Some((_, op, _, right)) => HlsExpr::BinOp { left: Box::new(left), op, right: Box::new(right) },
        None => left,
    }))
}

fn parse_additive_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_multiplicative_expr(input)?;
    let (input, rights) = many0(tuple((
        multispace0,
        alt((
            value(BinOperator::Add, tag("+")),
            value(BinOperator::Sub, tag("-")),
            value(BinOperator::Concat, tag("++")),
        )),
        multispace0,
        parse_multiplicative_expr
    )))(input)?;
    
    Ok((input, rights.into_iter().fold(left, |acc, (_, op, _, right)| {
        HlsExpr::BinOp { left: Box::new(acc), op, right: Box::new(right) }
    })))
}

fn parse_multiplicative_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_unary_expr(input)?;
    let (input, rights) = many0(tuple((
        multispace0,
        alt((
            value(BinOperator::Mul, tag("*")),
            value(BinOperator::Div, tag("/")),
            value(BinOperator::Mod, tag("%")),
        )),
        multispace0,
        parse_unary_expr
    )))(input)?;
    
    Ok((input, rights.into_iter().fold(left, |acc, (_, op, _, right)| {
        HlsExpr::BinOp { left: Box::new(acc), op, right: Box::new(right) }
    })))
}

fn parse_unary_expr(input: &str) -> IResult<&str, HlsExpr> {
    alt((
        map(preceded(tuple((tag("!"), multispace0)), parse_unary_expr), |e| {
            HlsExpr::UnaryOp { op: UnaryOperator::Not, expr: Box::new(e) }
        }),
        map(preceded(tuple((tag("-"), multispace0)), parse_unary_expr), |e| {
            HlsExpr::UnaryOp { op: UnaryOperator::Neg, expr: Box::new(e) }
        }),
        parse_primary_expr
    ))(input)
}

fn parse_primary_expr(input: &str) -> IResult<&str, HlsExpr> {
    alt((
        parse_string_expr,
        parse_number_expr,
        parse_bool_expr,
        parse_list_expr,
        parse_var_or_call_expr,
        parse_paren_expr,
    ))(input)
}

fn parse_string_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, s) = parse_string_lit(input)?;
    Ok((input, HlsExpr::StringLit(s.to_string())))
}

fn parse_string_lit(input: &str) -> IResult<&str, &str> {
    let (input, _) = nom_char('"')(input)?;
    let (input, content) = take_while1(|c| c != '"')(input)?;
    let (input, _) = nom_char('"')(input)?;
    Ok((input, content))
}

fn parse_number_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, num_str) = recognize(tuple((
        opt(nom_char('-')),
        digit1,
        opt(tuple((nom_char('.'), digit1)))
    )))(input)?;
    
    let num: f64 = num_str.parse().unwrap_or(0.0);
    Ok((input, HlsExpr::NumberLit(num)))
}

fn parse_bool_expr(input: &str) -> IResult<&str, HlsExpr> {
    alt((
        value(HlsExpr::BoolLit(true), tag("true")),
        value(HlsExpr::BoolLit(false), tag("false")),
    ))(input)
}

fn parse_list_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, _) = nom_char('[')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, items) = separated_list0(
        tuple((multispace0, nom_char(','), multispace0)),
        parse_expr
    )(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char(']')(input)?;
    
    Ok((input, HlsExpr::List(items)))
}

fn parse_var_or_call_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, name) = parse_identifier(input)?;
    let (input, call) = opt(tuple((
        multispace0,
        nom_char('('),
        multispace0,
        separated_list0(tuple((multispace0, nom_char(','), multispace0)), parse_expr),
        multispace0,
        nom_char(')')
    )))(input)?;
    
    Ok((input, match call {
        Some((_, _, _, args, _, _)) => HlsExpr::Call { name: name.to_string(), args },
        None => HlsExpr::Var(name.to_string()),
    }))
}

fn parse_paren_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, _) = nom_char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, expr) = parse_expr(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char(')')(input)?;
    Ok((input, expr))
}

fn parse_identifier(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c.is_alphanumeric() || c == '_')(input)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_simple_element() {
        let source = r#"element div {}"#;
        let binary = Compiler::compile(source).unwrap();
        
        assert!(!binary.instructions.is_empty());
        assert!(matches!(
            &binary.instructions[0],
            Instruction::DefineElement { tag, .. } if tag == "div"
        ));
    }

    #[test]
    fn test_compile_nested_elements() {
        let source = r#"
            element container {
                element child {}
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // Should have: DefineElement(container), DefineElement(child), AddChild
        assert!(binary.instructions.len() >= 3);
    }

    #[test]
    fn test_compile_with_text() {
        let source = r#"
            element div {
                text "Hello World"
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // Should create a text element with content attribute
        assert!(binary.instructions.iter().any(|i| matches!(i,
            Instruction::SetAttribute { key, value, .. } 
            if key == "content" && value == "Hello World"
        )));
    }

    #[test]
    fn test_compile_conditional() {
        let source = r#"
            let show = true
            if show {
                element visible {}
            } else {
                element hidden {}
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // Should only compile the "visible" branch since show is true
        assert!(binary.instructions.iter().any(|i| matches!(i,
            Instruction::DefineElement { tag, .. } if tag == "visible"
        )));
        assert!(!binary.instructions.iter().any(|i| matches!(i,
            Instruction::DefineElement { tag, .. } if tag == "hidden"
        )));
    }

    #[test]
    fn test_compile_for_loop() {
        let source = r#"
            for item in [1, 2, 3] {
                element item {}
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // Should create 3 "item" elements
        let item_count = binary.instructions.iter().filter(|i| matches!(i,
            Instruction::DefineElement { tag, .. } if tag == "item"
        )).count();
        assert_eq!(item_count, 3);
    }

    #[test]
    fn test_compile_expressions() {
        let source = r#"
            let a = 10
            let b = 20
            let sum = a + b
            if sum > 25 {
                element result {}
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // sum = 30, which is > 25, so "result" should be rendered
        assert!(binary.instructions.iter().any(|i| matches!(i,
            Instruction::DefineElement { tag, .. } if tag == "result"
        )));
    }

    #[test]
    fn test_parse_expression_operators() {
        // Test that expressions parse correctly
        let (_, expr) = parse_expr("1 + 2 * 3").unwrap();
        
        if let HlsExpr::BinOp { op, .. } = expr {
            // Addition should be at top level (lower precedence)
            assert!(matches!(op, BinOperator::Add));
        } else {
            panic!("Expected binary operation");
        }
    }

    #[test]
    fn test_compile_comment_ignored() {
        let source = r#"
            // This is a comment
            element div {}
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // Comment should not affect output
        assert!(!binary.instructions.is_empty());
    }
}
