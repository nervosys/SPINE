use nom::{
    bytes::complete::{tag, take_while1},
    character::complete::{multispace0, multispace1, digit1, char as nom_char},
    sequence::{preceded, tuple, terminated},
    branch::alt,
    combinator::{opt, map, recognize, value},
    multi::{many0, separated_list0},
    IResult,
};
use hyperlight_protocol::{HyperlightBinary, Instruction, ProtocolBinOp, ProtocolUnaryOp};
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
    /// Exported function entry points
    pub exported_functions: HashMap<String, usize>,
    /// Main render entry point
    pub render_start: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HlsType {
    String,
    Number,
    Boolean,
    Element,
    List(Box<HlsType>),
    Object,
    Null,
    Any,
}

#[derive(Debug, Clone)]
pub enum HlsValue {
    String(String),
    Number(f64),
    Boolean(bool),
    List(Vec<HlsValue>),
    Object(HashMap<String, HlsValue>),
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
    BinOp { left: Box<HlsExpr>, op: ProtocolBinOp, right: Box<HlsExpr> },
    /// Unary operation
    UnaryOp { op: ProtocolUnaryOp, expr: Box<HlsExpr> },
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
        
        // Type checking pass
        let mut type_ctx = CompilerContext::default();
        for stmt in &statements {
            Self::check_types(&mut type_ctx, stmt)?;
        }
        
        // Optimization pass
        let optimized_statements = Self::optimize(statements);
        
        // Compilation pass
        for stmt in optimized_statements {
            Self::compile_statement(&mut ctx, &stmt)?;
        }
        
        Ok(HyperlightBinary {
            instructions: ctx.instructions,
            data: ctx.string_pool.join("\0").into_bytes(),
            render_start: ctx.render_start,
            exported_functions: ctx.exported_functions,
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
                    Self::compile_expr(ctx, value)?;
                    ctx.instructions.push(Instruction::SetAttributeFromStack {
                        id,
                        key: key.clone(),
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
                    // Events are compiled as separate functions or blocks
                    // For now, we'll just emit them as EmitEventFromStack if they are simple
                    for handler_stmt in &event.handler {
                        if let HlsStatement::Emit { event: evt_name, payload } = handler_stmt {
                            Self::compile_expr(ctx, payload)?;
                            ctx.instructions.push(Instruction::EmitEventFromStack {
                                name: evt_name.clone(),
                            });
                        }
                    }
                }
                
                ctx.element_stack.pop();
            }
            
            HlsStatement::Text(expr) => {
                Self::compile_expr(ctx, expr)?;
                ctx.instructions.push(Instruction::DefineTextFromStack);
                if let Some(&parent_id) = ctx.element_stack.last() {
                    // Text nodes are children too
                    // We need a way to refer to the text node ID, but for now let's assume
                    // DefineTextFromStack handles the attachment or we need a new instruction
                }
            }
            HlsStatement::Let { name, value, .. } => {
                Self::compile_expr(ctx, value)?;
                ctx.instructions.push(Instruction::Store(name.clone()));
            }
            
            HlsStatement::State { name, initial, .. } => {
                Self::compile_expr(ctx, initial)?;
                ctx.instructions.push(Instruction::DeclareStateFromStack {
                    name: name.clone(),
                });
            }
            
            HlsStatement::FnDef { name, params: _, body, .. } => {
                let jump_over_idx = ctx.instructions.len();
                ctx.instructions.push(Instruction::Jump(0)); // Placeholder
                
                let func_start = ctx.instructions.len();
                ctx.exported_functions.insert(name.clone(), func_start);
                
                // Compile body
                for stmt in body {
                    Self::compile_statement(ctx, stmt)?;
                }
                ctx.instructions.push(Instruction::Return);
                
                let end_idx = ctx.instructions.len();
                ctx.instructions[jump_over_idx] = Instruction::Jump(end_idx);
            }
            
            HlsStatement::If { condition, then_branch, else_branch } => {
                Self::compile_expr(ctx, condition)?;
                
                let jump_if_idx = ctx.instructions.len();
                ctx.instructions.push(Instruction::JumpIf(0)); // Placeholder
                
                // Else branch (if condition is false, we continue here)
                if let Some(else_stmts) = else_branch {
                    for stmt in else_stmts {
                        Self::compile_statement(ctx, stmt)?;
                    }
                }
                
                let jump_end_idx = ctx.instructions.len();
                ctx.instructions.push(Instruction::Jump(0)); // Placeholder
                
                // Then branch
                let then_start = ctx.instructions.len();
                for stmt in then_branch {
                    Self::compile_statement(ctx, stmt)?;
                }
                let end_idx = ctx.instructions.len();
                
                // Patch jumps
                ctx.instructions[jump_if_idx] = Instruction::JumpIf(then_start);
                ctx.instructions[jump_end_idx] = Instruction::Jump(end_idx);
            }
            
            HlsStatement::For { item, list, body } => {
                // If list is a literal list, we can unroll it
                if let HlsExpr::List(items) = list {
                    for val in items {
                        // Substitute 'item' with 'val' in body
                        let mut param_map = HashMap::new();
                        param_map.insert(item.clone(), val.clone());
                        let unrolled_body = Self::substitute_params_internal(body.clone(), &param_map);
                        for stmt in unrolled_body {
                            Self::compile_statement(ctx, &stmt)?;
                        }
                    }
                } else {
                    // For loops are complex to compile to bytecode without a proper iterator
                    // For now, let's just support static evaluation if possible, or skip
                }
            }
            
            HlsStatement::While { condition, body } => {
                let start_idx = ctx.instructions.len();
                Self::compile_expr(ctx, condition)?;
                
                let jump_out_idx = ctx.instructions.len();
                ctx.instructions.push(Instruction::JumpIfNot(0)); // Placeholder
                
                for stmt in body {
                    Self::compile_statement(ctx, stmt)?;
                }
                
                ctx.instructions.push(Instruction::Jump(start_idx));
                let end_idx = ctx.instructions.len();
                
                ctx.instructions[jump_out_idx] = Instruction::JumpIfNot(end_idx);
            }
            
            HlsStatement::Emit { event, payload } => {
                Self::compile_expr(ctx, payload)?;
                ctx.instructions.push(Instruction::EmitEventFromStack {
                    name: event.clone(),
                });
            }
            
            HlsStatement::Assign { name, value } => {
                Self::compile_expr(ctx, value)?;
                ctx.instructions.push(Instruction::UpdateStateFromStack {
                    name: name.clone(),
                });
            }
            
            HlsStatement::Call { name, args } => {
                for arg in args {
                    Self::compile_expr(ctx, arg)?;
                }
                // Check if it's a local function
                if let Some(&target) = ctx.exported_functions.get(name) {
                    ctx.instructions.push(Instruction::CallTarget(target));
                } else {
                    ctx.instructions.push(Instruction::Call { name: name.clone(), num_args: args.len() });
                }
            }
            
            HlsStatement::Return(expr) => {
                if let Some(e) = expr {
                    Self::compile_expr(ctx, e)?;
                }
                ctx.instructions.push(Instruction::Return);
            }
            
            HlsStatement::Comment(_) => {}
        }
        Ok(())
    }
    
    fn compile_expr(ctx: &mut CompilerContext, expr: &HlsExpr) -> anyhow::Result<()> {
        match expr {
            HlsExpr::StringLit(s) => {
                ctx.instructions.push(Instruction::Push(serde_json::Value::String(s.clone())));
            }
            HlsExpr::NumberLit(n) => {
                ctx.instructions.push(Instruction::Push(serde_json::json!(n)));
            }
            HlsExpr::BoolLit(b) => {
                ctx.instructions.push(Instruction::Push(serde_json::Value::Bool(*b)));
            }
            HlsExpr::Var(name) => {
                ctx.instructions.push(Instruction::Load(name.clone()));
            }
            HlsExpr::BinOp { left, op, right } => {
                Self::compile_expr(ctx, left)?;
                Self::compile_expr(ctx, right)?;
                ctx.instructions.push(Instruction::BinOp(*op));
            }
            HlsExpr::UnaryOp { op, expr } => {
                Self::compile_expr(ctx, expr)?;
                ctx.instructions.push(Instruction::UnaryOp(*op));
            }
            HlsExpr::Call { name, args } => {
                for arg in args {
                    Self::compile_expr(ctx, arg)?;
                }
                ctx.instructions.push(Instruction::Call { name: name.clone(), num_args: args.len() });
            }
            HlsExpr::List(items) => {
                for item in items {
                    Self::compile_expr(ctx, item)?;
                }
                ctx.instructions.push(Instruction::Call { name: "list".to_string(), num_args: items.len() });
            }
            HlsExpr::Object(props) => {
                for (key, val) in props {
                    ctx.instructions.push(Instruction::Push(serde_json::Value::String(key.clone())));
                    Self::compile_expr(ctx, val)?;
                }
                ctx.instructions.push(Instruction::Call { name: "object".to_string(), num_args: props.len() * 2 });
            }
            _ => {}
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
                    ProtocolUnaryOp::Not => match v {
                        HlsValue::Boolean(b) => HlsValue::Boolean(!b),
                        _ => HlsValue::Boolean(false),
                    },
                    ProtocolUnaryOp::Neg => match v {
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
                let mut map = HashMap::new();
                for (k, v) in pairs {
                    map.insert(k.clone(), Self::eval_expr(ctx, v));
                }
                HlsValue::Object(map)
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
    
    fn eval_binop(left: HlsValue, op: ProtocolBinOp, right: HlsValue) -> HlsValue {
        match op {
            ProtocolBinOp::Add => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Number(l + r),
                (HlsValue::String(l), HlsValue::String(r)) => HlsValue::String(format!("{}{}", l, r)),
                _ => HlsValue::Null,
            },
            ProtocolBinOp::Sub => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Number(l - r),
                _ => HlsValue::Null,
            },
            ProtocolBinOp::Mul => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Number(l * r),
                _ => HlsValue::Null,
            },
            ProtocolBinOp::Div => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) if r != 0.0 => HlsValue::Number(l / r),
                _ => HlsValue::Null,
            },
            ProtocolBinOp::Mod => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) if r != 0.0 => HlsValue::Number(l % r),
                _ => HlsValue::Null,
            },
            ProtocolBinOp::Eq => HlsValue::Boolean(Self::values_equal(&left, &right)),
            ProtocolBinOp::Ne => HlsValue::Boolean(!Self::values_equal(&left, &right)),
            ProtocolBinOp::Lt => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l < r),
                _ => HlsValue::Boolean(false),
            },
            ProtocolBinOp::Le => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l <= r),
                _ => HlsValue::Boolean(false),
            },
            ProtocolBinOp::Gt => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l > r),
                _ => HlsValue::Boolean(false),
            },
            ProtocolBinOp::Ge => match (left, right) {
                (HlsValue::Number(l), HlsValue::Number(r)) => HlsValue::Boolean(l >= r),
                _ => HlsValue::Boolean(false),
            },
            ProtocolBinOp::And => match (left, right) {
                (HlsValue::Boolean(l), HlsValue::Boolean(r)) => HlsValue::Boolean(l && r),
                _ => HlsValue::Boolean(false),
            },
            ProtocolBinOp::Or => match (left, right) {
                (HlsValue::Boolean(l), HlsValue::Boolean(r)) => HlsValue::Boolean(l || r),
                _ => HlsValue::Boolean(false),
            },
            ProtocolBinOp::Concat => {
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
            HlsValue::Object(map) => {
                let pairs: Vec<String> = map.iter()
                    .map(|(k, v)| format!("{}: {}", k, Self::value_to_string(v)))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }
            HlsValue::Null => "null".to_string(),
        }
    }
    
    fn infer_type(val: &HlsValue) -> HlsType {
        match val {
            HlsValue::String(_) => HlsType::String,
            HlsValue::Number(_) => HlsType::Number,
            HlsValue::Boolean(_) => HlsType::Boolean,
            HlsValue::List(items) => {
                if items.is_empty() {
                    HlsType::List(Box::new(HlsType::Any))
                } else {
                    let inner = Self::infer_type(&items[0]);
                    HlsType::List(Box::new(inner))
                }
            }
            HlsValue::Object(_) => HlsType::Object,
            HlsValue::Null => HlsType::Null,
        }
    }
    
    fn infer_expr_type(ctx: &CompilerContext, expr: &HlsExpr) -> anyhow::Result<HlsType> {
        match expr {
            HlsExpr::StringLit(_) => Ok(HlsType::String),
            HlsExpr::NumberLit(_) => Ok(HlsType::Number),
            HlsExpr::BoolLit(_) => Ok(HlsType::Boolean),
            HlsExpr::Var(name) => {
                ctx.variables.get(name)
                    .map(|(t, _)| t.clone())
                    .ok_or_else(|| anyhow::anyhow!("Undefined variable '{}'", name))
            }
            HlsExpr::BinOp { left, op, right } => {
                let lt = Self::infer_expr_type(ctx, left)?;
                let rt = Self::infer_expr_type(ctx, right)?;
                match op {
                    ProtocolBinOp::Add | ProtocolBinOp::Sub | ProtocolBinOp::Mul | ProtocolBinOp::Div | ProtocolBinOp::Mod => {
                        if (lt == HlsType::Number || lt == HlsType::Any) && (rt == HlsType::Number || rt == HlsType::Any) {
                            Ok(HlsType::Number)
                        } else {
                            Err(anyhow::anyhow!("Arithmetic operators require numbers"))
                        }
                    }
                    ProtocolBinOp::Eq | ProtocolBinOp::Ne | ProtocolBinOp::Lt | ProtocolBinOp::Le | ProtocolBinOp::Gt | ProtocolBinOp::Ge => {
                        Ok(HlsType::Boolean)
                    }
                    ProtocolBinOp::And | ProtocolBinOp::Or => {
                        Ok(HlsType::Boolean)
                    }
                    ProtocolBinOp::Concat => Ok(HlsType::String),
                }
            }
            HlsExpr::UnaryOp { op, expr } => {
                let t = Self::infer_expr_type(ctx, expr)?;
                match op {
                    ProtocolUnaryOp::Not => Ok(HlsType::Boolean),
                    ProtocolUnaryOp::Neg => {
                        if t == HlsType::Number { Ok(HlsType::Number) }
                        else { Err(anyhow::anyhow!("Negation requires a number")) }
                    }
                }
            }
            HlsExpr::List(items) => {
                if items.is_empty() {
                    Ok(HlsType::List(Box::new(HlsType::Any)))
                } else {
                    let inner = Self::infer_expr_type(ctx, &items[0])?;
                    Ok(HlsType::List(Box::new(inner)))
                }
            }
            HlsExpr::Object(_) => Ok(HlsType::Object),
            HlsExpr::Call { name, args: _ } => {
                if let Some(func) = ctx.functions.get(name) {
                    Ok(func.return_type.clone())
                } else {
                    // Built-ins
                    match name.as_str() {
                        "len" => Ok(HlsType::Number),
                        "str" => Ok(HlsType::String),
                        "num" => Ok(HlsType::Number),
                        _ => Ok(HlsType::Any),
                    }
                }
            }
            HlsExpr::Property { .. } => Ok(HlsType::Any),
            HlsExpr::Index { object, .. } => {
                let ot = Self::infer_expr_type(ctx, object)?;
                match ot {
                    HlsType::List(inner) => Ok(*inner),
                    _ => Ok(HlsType::Any),
                }
            }
            HlsExpr::Ternary { then_expr, .. } => Self::infer_expr_type(ctx, then_expr),
        }
    }

    fn types_match(expected: &HlsType, actual: &HlsType) -> bool {
        match (expected, actual) {
            (HlsType::Any, _) | (_, HlsType::Any) => true,
            (HlsType::List(e), HlsType::List(a)) => Self::types_match(e, a),
            (e, a) => e == a,
        }
    }

    fn check_types(ctx: &mut CompilerContext, stmt: &HlsStatement) -> anyhow::Result<()> {
        match stmt {
            HlsStatement::Let { name, value, type_annotation } => {
                let val_type = Self::infer_expr_type(ctx, value)?;
                let final_type = if let Some(annotated) = type_annotation {
                    if !Self::types_match(annotated, &val_type) {
                        return Err(anyhow::anyhow!("Type mismatch for variable '{}': expected {:?}, found {:?}", name, annotated, val_type));
                    }
                    annotated.clone()
                } else {
                    val_type
                };
                ctx.variables.insert(name.clone(), (final_type, HlsValue::Null));
            }
            HlsStatement::State { name, initial, type_annotation } => {
                let val_type = Self::infer_expr_type(ctx, initial)?;
                let final_type = if let Some(annotated) = type_annotation {
                    if !Self::types_match(annotated, &val_type) {
                        return Err(anyhow::anyhow!("Type mismatch for state '{}': expected {:?}, found {:?}", name, annotated, val_type));
                    }
                    annotated.clone()
                } else {
                    val_type
                };
                ctx.variables.insert(name.clone(), (final_type, HlsValue::Null));
            }
            HlsStatement::Assign { name, value } => {
                let val_type = Self::infer_expr_type(ctx, value)?;
                if let Some((expected_type, _)) = ctx.variables.get(name) {
                    if !Self::types_match(expected_type, &val_type) {
                        return Err(anyhow::anyhow!("Type mismatch in assignment to '{}': expected {:?}, found {:?}", name, expected_type, val_type));
                    }
                } else {
                    return Err(anyhow::anyhow!("Undefined variable '{}'", name));
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
                ctx.functions.insert(name.clone(), HlsFunction {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    return_type: r_type.clone(),
                });

                for s in body {
                    Self::check_types(&mut fn_ctx, s)?;
                }
            }
            HlsStatement::If { condition, then_branch, else_branch } => {
                let _ = Self::infer_expr_type(ctx, condition)?;
                for s in then_branch { Self::check_types(ctx, s)?; }
                if let Some(eb) = else_branch {
                    for s in eb { Self::check_types(ctx, s)?; }
                }
            }
            HlsStatement::For { item, list, body } => {
                let lt = Self::infer_expr_type(ctx, list)?;
                let inner_type = match lt {
                    HlsType::List(inner) => *inner,
                    _ => HlsType::Any,
                };
                ctx.variables.insert(item.clone(), (inner_type, HlsValue::Null));
                for s in body { Self::check_types(ctx, s)?; }
                ctx.variables.remove(item);
            }
            HlsStatement::Element { children, .. } => {
                for s in children { Self::check_types(ctx, s)?; }
            }
            HlsStatement::Call { name, args } => {
                if let Some(func) = ctx.functions.get(name) {
                    if func.params.len() != args.len() {
                        return Err(anyhow::anyhow!("Function '{}' expected {} arguments, found {}", name, func.params.len(), args.len()));
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let arg_type = Self::infer_expr_type(ctx, arg)?;
                        if !Self::types_match(&func.params[i].1, &arg_type) {
                            return Err(anyhow::anyhow!("Type mismatch in call to '{}' for argument {}: expected {:?}, found {:?}", name, i, func.params[i].1, arg_type));
                        }
                    }
                }
            }
            HlsStatement::Return(expr) => {
                if let Some(e) = expr {
                    let _ = Self::infer_expr_type(ctx, e)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn optimize(statements: Vec<HlsStatement>) -> Vec<HlsStatement> {
        let mut functions = HashMap::new();
        // First pass: collect and optimize functions for inlining
        for stmt in &statements {
            if let HlsStatement::FnDef { name, params, body, return_type } = stmt {
                // We need a temporary functions map for recursive functions, but for now let's just use empty
                let optimized_body = Self::optimize_internal(body.clone(), &HashMap::new());
                functions.insert(name.clone(), HlsFunction {
                    name: name.clone(),
                    params: params.clone(),
                    body: optimized_body,
                    return_type: return_type.clone().unwrap_or(HlsType::Any),
                });
            }
        }

        let mut optimized = Vec::new();
        for stmt in statements {
            optimized.extend(Self::optimize_statement(stmt, &functions));
        }
        optimized
    }

    fn optimize_statement(stmt: HlsStatement, functions: &HashMap<String, HlsFunction>) -> Vec<HlsStatement> {
        match stmt {
            HlsStatement::FnDef { name, params, body, return_type } => vec![HlsStatement::FnDef {
                name,
                params,
                body: Self::optimize_internal(body, functions),
                return_type,
            }],
            HlsStatement::Let { name, value, type_annotation } => vec![HlsStatement::Let { 
                name, 
                value: Self::optimize_expr(value, functions), 
                type_annotation 
            }],
            HlsStatement::State { name, initial, type_annotation } => vec![HlsStatement::State { 
                name, 
                initial: Self::optimize_expr(initial, functions), 
                type_annotation 
            }],
            HlsStatement::Assign { name, value } => vec![HlsStatement::Assign { 
                name, 
                value: Self::optimize_expr(value, functions) 
            }],
            HlsStatement::If { condition, then_branch, else_branch } => {
                let opt_cond = Self::optimize_expr(condition, functions);
                match opt_cond {
                    HlsExpr::BoolLit(true) => Self::optimize_internal(then_branch, functions),
                    HlsExpr::BoolLit(false) => else_branch.map(|b| Self::optimize_internal(b, functions)).unwrap_or_default(),
                    _ => vec![HlsStatement::If { 
                        condition: opt_cond, 
                        then_branch: Self::optimize_internal(then_branch, functions), 
                        else_branch: else_branch.map(|b| Self::optimize_internal(b, functions)) 
                    }]
                }
            }
            HlsStatement::For { item, list, body } => vec![HlsStatement::For { 
                item, 
                list: Self::optimize_expr(list, functions), 
                body: Self::optimize_internal(body, functions) 
            }],
            HlsStatement::While { condition, body } => {
                let opt_cond = Self::optimize_expr(condition, functions);
                match opt_cond {
                    HlsExpr::BoolLit(false) => vec![], // While false is dead code
                    _ => vec![HlsStatement::While { 
                        condition: opt_cond, 
                        body: Self::optimize_internal(body, functions) 
                    }]
                }
            }
            HlsStatement::Element { tag, attributes, children, events } => vec![HlsStatement::Element {
                tag,
                attributes: attributes.into_iter().map(|(k, v)| (k, Self::optimize_expr(v, functions))).collect(),
                children: Self::optimize_internal(children, functions),
                events: events.into_iter().map(|e| HlsEvent {
                    event_type: e.event_type,
                    handler: Self::optimize_internal(e.handler, functions),
                }).collect(),
            }],
            HlsStatement::Text(expr) => vec![HlsStatement::Text(Self::optimize_expr(expr, functions))],
            HlsStatement::Call { name, args } => {
                // Inlining check
                if let Some(func) = functions.get(&name) {
                    if func.body.len() <= 3 && !func.body.iter().any(|s| matches!(s, HlsStatement::FnDef { .. })) {
                        // Inline small function
                        let mut inlined = Vec::new();
                        // Map params to args
                        let mut param_map = HashMap::new();
                        for (i, (p_name, _)) in func.params.iter().enumerate() {
                            if i < args.len() {
                                param_map.insert(p_name.clone(), args[i].clone());
                            }
                        }
                        
                        for s in &func.body {
                            inlined.extend(Self::substitute_params_stmt(s.clone(), &param_map));
                        }
                        return Self::optimize_internal(inlined, functions);
                    }
                }
                vec![HlsStatement::Call { 
                    name, 
                    args: args.into_iter().map(|a| Self::optimize_expr(a, functions)).collect() 
                }]
            },
            HlsStatement::Emit { event, payload } => vec![HlsStatement::Emit { 
                event, 
                payload: Self::optimize_expr(payload, functions) 
            }],
            HlsStatement::Return(expr) => vec![HlsStatement::Return(expr.map(|e| Self::optimize_expr(e, functions)))],
            HlsStatement::Comment(_) => vec![], // Strip comments in optimization
            _ => vec![stmt],
        }
    }

    fn optimize_internal(statements: Vec<HlsStatement>, functions: &HashMap<String, HlsFunction>) -> Vec<HlsStatement> {
        let mut optimized = Vec::new();
        for stmt in statements {
            optimized.extend(Self::optimize_statement(stmt, functions));
        }
        optimized
    }

    fn optimize_expr(expr: HlsExpr, functions: &HashMap<String, HlsFunction>) -> HlsExpr {
        match expr {
            HlsExpr::Call { name, args } => {
                // Inlining check for expressions
                if let Some(func) = functions.get(&name) {
                    if func.body.len() == 1 {
                        if let HlsStatement::Return(Some(ret_expr)) = &func.body[0] {
                            // Map params to args
                            let mut param_map = HashMap::new();
                            for (i, (p_name, _)) in func.params.iter().enumerate() {
                                if i < args.len() {
                                    param_map.insert(p_name.clone(), args[i].clone());
                                }
                            }
                            let inlined = Self::substitute_params_expr(ret_expr.clone(), &param_map);
                            return Self::optimize_expr(inlined, functions);
                        }
                    }
                }
                HlsExpr::Call { 
                    name, 
                    args: args.into_iter().map(|a| Self::optimize_expr(a, functions)).collect() 
                }
            }
            HlsExpr::BinOp { left, op, right } => {
                let l = Self::optimize_expr(*left, functions);
                let r = Self::optimize_expr(*right, functions);
                
                // Constant folding
                match (l, op, r) {
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Add, HlsExpr::NumberLit(b)) => HlsExpr::NumberLit(a + b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Sub, HlsExpr::NumberLit(b)) => HlsExpr::NumberLit(a - b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Mul, HlsExpr::NumberLit(b)) => HlsExpr::NumberLit(a * b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Div, HlsExpr::NumberLit(b)) => HlsExpr::NumberLit(a / b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Mod, HlsExpr::NumberLit(b)) => HlsExpr::NumberLit(a % b),
                    
                    (HlsExpr::BoolLit(a), ProtocolBinOp::And, HlsExpr::BoolLit(b)) => HlsExpr::BoolLit(a && b),
                    (HlsExpr::BoolLit(a), ProtocolBinOp::Or, HlsExpr::BoolLit(b)) => HlsExpr::BoolLit(a || b),
                    
                    (HlsExpr::StringLit(a), ProtocolBinOp::Concat, HlsExpr::StringLit(b)) => HlsExpr::StringLit(format!("{}{}", a, b)),
                    
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Eq, HlsExpr::NumberLit(b)) => HlsExpr::BoolLit(a == b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Ne, HlsExpr::NumberLit(b)) => HlsExpr::BoolLit(a != b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Lt, HlsExpr::NumberLit(b)) => HlsExpr::BoolLit(a < b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Le, HlsExpr::NumberLit(b)) => HlsExpr::BoolLit(a <= b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Gt, HlsExpr::NumberLit(b)) => HlsExpr::BoolLit(a > b),
                    (HlsExpr::NumberLit(a), ProtocolBinOp::Ge, HlsExpr::NumberLit(b)) => HlsExpr::BoolLit(a >= b),
                    
                    (l, op, r) => HlsExpr::BinOp { 
                        left: Box::new(l), 
                        op, 
                        right: Box::new(r) 
                    }
                }
            }
            HlsExpr::UnaryOp { op, expr } => {
                let e = Self::optimize_expr(*expr, functions);
                match (op, e) {
                    (ProtocolUnaryOp::Not, HlsExpr::BoolLit(b)) => HlsExpr::BoolLit(!b),
                    (ProtocolUnaryOp::Neg, HlsExpr::NumberLit(n)) => HlsExpr::NumberLit(-n),
                    (op, e) => HlsExpr::UnaryOp { op, expr: Box::new(e) }
                }
            }
            HlsExpr::Ternary { condition, then_expr, else_expr } => {
                let cond = Self::optimize_expr(*condition, functions);
                match cond {
                    HlsExpr::BoolLit(true) => Self::optimize_expr(*then_expr, functions),
                    HlsExpr::BoolLit(false) => Self::optimize_expr(*else_expr, functions),
                    _ => HlsExpr::Ternary { 
                        condition: Box::new(cond), 
                        then_expr: Box::new(Self::optimize_expr(*then_expr, functions)), 
                        else_expr: Box::new(Self::optimize_expr(*else_expr, functions)) 
                    }
                }
            }
            HlsExpr::List(items) => HlsExpr::List(items.into_iter().map(|i| Self::optimize_expr(i, functions)).collect()),
            HlsExpr::Object(pairs) => HlsExpr::Object(pairs.into_iter().map(|(k, v)| (k, Self::optimize_expr(v, functions))).collect()),
            _ => expr,
        }
    }

    fn substitute_params_internal(statements: Vec<HlsStatement>, param_map: &HashMap<String, HlsExpr>) -> Vec<HlsStatement> {
        let mut substituted = Vec::new();
        for stmt in statements {
            substituted.extend(Self::substitute_params_stmt(stmt, param_map));
        }
        substituted
    }

    fn substitute_params_stmt(stmt: HlsStatement, param_map: &HashMap<String, HlsExpr>) -> Vec<HlsStatement> {
        match stmt {
            HlsStatement::Let { name, value, type_annotation } => vec![HlsStatement::Let { 
                name, 
                value: Self::substitute_params_expr(value, param_map), 
                type_annotation 
            }],
            HlsStatement::Assign { name, value } => vec![HlsStatement::Assign { 
                name, 
                value: Self::substitute_params_expr(value, param_map) 
            }],
            HlsStatement::If { condition, then_branch, else_branch } => vec![HlsStatement::If {
                condition: Self::substitute_params_expr(condition, param_map),
                then_branch: then_branch.into_iter().flat_map(|s| Self::substitute_params_stmt(s, param_map)).collect(),
                else_branch: else_branch.map(|b| b.into_iter().flat_map(|s| Self::substitute_params_stmt(s, param_map)).collect()),
            }],
            HlsStatement::Call { name, args } => vec![HlsStatement::Call {
                name,
                args: args.into_iter().map(|a| Self::substitute_params_expr(a, param_map)).collect(),
            }],
            HlsStatement::Text(expr) => vec![HlsStatement::Text(Self::substitute_params_expr(expr, param_map))],
            HlsStatement::Emit { event, payload } => vec![HlsStatement::Emit {
                event,
                payload: Self::substitute_params_expr(payload, param_map),
            }],
            HlsStatement::Return(expr) => vec![HlsStatement::Return(expr.map(|e| Self::substitute_params_expr(e, param_map)))],
            _ => vec![stmt],
        }
    }

    fn substitute_params_expr(expr: HlsExpr, param_map: &HashMap<String, HlsExpr>) -> HlsExpr {
        match expr {
            HlsExpr::Var(name) => {
                if let Some(sub) = param_map.get(&name) {
                    sub.clone()
                } else {
                    HlsExpr::Var(name)
                }
            }
            HlsExpr::BinOp { left, op, right } => HlsExpr::BinOp {
                left: Box::new(Self::substitute_params_expr(*left, param_map)),
                op,
                right: Box::new(Self::substitute_params_expr(*right, param_map)),
            },
            HlsExpr::UnaryOp { op, expr } => HlsExpr::UnaryOp {
                op,
                expr: Box::new(Self::substitute_params_expr(*expr, param_map)),
            },
            HlsExpr::Call { name, args } => HlsExpr::Call {
                name,
                args: args.into_iter().map(|a| Self::substitute_params_expr(a, param_map)).collect(),
            },
            HlsExpr::Ternary { condition, then_expr, else_expr } => HlsExpr::Ternary {
                condition: Box::new(Self::substitute_params_expr(*condition, param_map)),
                then_expr: Box::new(Self::substitute_params_expr(*then_expr, param_map)),
                else_expr: Box::new(Self::substitute_params_expr(*else_expr, param_map)),
            },
            HlsExpr::List(items) => HlsExpr::List(items.into_iter().map(|i| Self::substitute_params_expr(i, param_map)).collect()),
            HlsExpr::Object(pairs) => HlsExpr::Object(pairs.into_iter().map(|(k, v)| (k, Self::substitute_params_expr(v, param_map))).collect()),
            _ => expr,
        }
    }
    
    fn expr_to_json(ctx: &CompilerContext, expr: &HlsExpr) -> serde_json::Value {
        Self::value_to_json(&Self::eval_expr(ctx, expr))
    }

    fn value_to_json(val: &HlsValue) -> serde_json::Value {
        match val {
            HlsValue::String(s) => serde_json::Value::String(s.clone()),
            HlsValue::Number(n) => serde_json::json!(n),
            HlsValue::Boolean(b) => serde_json::Value::Bool(*b),
            HlsValue::List(items) => {
                let json_items: Vec<serde_json::Value> = items.iter()
                    .map(Self::value_to_json)
                    .collect();
                serde_json::Value::Array(json_items)
            }
            HlsValue::Object(map) => {
                let mut json_obj = serde_json::Map::new();
                for (k, v) in map {
                    json_obj.insert(k.clone(), Self::value_to_json(v));
                }
                serde_json::Value::Object(json_obj)
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
        parse_fn_def_stmt,
        parse_let_stmt,
        parse_state_stmt,
        parse_if_stmt,
        parse_for_stmt,
        parse_while_stmt,
        parse_assign_stmt,
        parse_call_stmt,
        parse_text_stmt,
        parse_emit_stmt,
        parse_return_stmt,
        parse_comment,
    ))(input)
}

fn parse_return_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("return")(input)?;
    let (input, expr) = opt(preceded(multispace1, parse_expr))(input)?;
    Ok((input, HlsStatement::Return(expr)))
}

fn parse_fn_def_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("fn")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('(')(input)?;
    let (input, params) = separated_list0(
        tuple((multispace0, nom_char(','), multispace0)),
        parse_param
    )(input)?;
    let (input, _) = nom_char(')')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, return_type) = opt(preceded(
        tuple((tag("->"), multispace0)),
        parse_type
    ))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('{')(input)?;
    let (input, body) = many0(preceded(multispace0, parse_statement))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('}')(input)?;

    Ok((input, HlsStatement::FnDef {
        name: name.to_string(),
        params,
        body,
        return_type,
    }))
}

fn parse_param(input: &str) -> IResult<&str, (String, HlsType)> {
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, typ) = opt(preceded(
        tuple((nom_char(':'), multispace0)),
        parse_type
    ))(input)?;
    Ok((input, (name.to_string(), typ.unwrap_or(HlsType::Any))))
}

fn parse_type(input: &str) -> IResult<&str, HlsType> {
    alt((
        value(HlsType::String, tag("string")),
        value(HlsType::Number, tag("number")),
        value(HlsType::Boolean, tag("boolean")),
        value(HlsType::Element, tag("element")),
        map(preceded(tag("list<"), terminated(parse_type, tag(">"))), |t| HlsType::List(Box::new(t))),
        value(HlsType::Any, tag("any")),
    ))(input)
}

fn parse_assign_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('=')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_expr(input)?;
    Ok((input, HlsStatement::Assign { name: name.to_string(), value }))
}

fn parse_call_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('(')(input)?;
    let (input, args) = separated_list0(
        tuple((multispace0, nom_char(','), multispace0)),
        parse_expr
    )(input)?;
    let (input, _) = nom_char(')')(input)?;
    Ok((input, HlsStatement::Call { name: name.to_string(), args }))
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
    let (input, type_annotation) = opt(preceded(
        tuple((nom_char(':'), multispace0)),
        parse_type
    ))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('=')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_expr(input)?;
    
    Ok((input, HlsStatement::Let {
        name: name.to_string(),
        value,
        type_annotation,
    }))
}

fn parse_state_stmt(input: &str) -> IResult<&str, HlsStatement> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("state")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, name) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, type_annotation) = opt(preceded(
        tuple((nom_char(':'), multispace0)),
        parse_type
    ))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = nom_char('=')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, initial) = parse_expr(input)?;
    
    Ok((input, HlsStatement::State {
        name: name.to_string(),
        initial,
        type_annotation,
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
        HlsExpr::BinOp { left: Box::new(acc), op: ProtocolBinOp::Or, right: Box::new(right) }
    })))
}

fn parse_and_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_comparison_expr(input)?;
    let (input, rights) = many0(preceded(
        tuple((multispace0, tag("&&"), multispace0)),
        parse_comparison_expr
    ))(input)?;
    
    Ok((input, rights.into_iter().fold(left, |acc, right| {
        HlsExpr::BinOp { left: Box::new(acc), op: ProtocolBinOp::And, right: Box::new(right) }
    })))
}

fn parse_comparison_expr(input: &str) -> IResult<&str, HlsExpr> {
    let (input, left) = parse_additive_expr(input)?;
    let (input, op_right) = opt(tuple((
        multispace0,
        alt((
            value(ProtocolBinOp::Eq, tag("==")),
            value(ProtocolBinOp::Ne, tag("!=")),
            value(ProtocolBinOp::Le, tag("<=")),
            value(ProtocolBinOp::Ge, tag(">=")),
            value(ProtocolBinOp::Lt, tag("<")),
            value(ProtocolBinOp::Gt, tag(">")),
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
            value(ProtocolBinOp::Add, tag("+")),
            value(ProtocolBinOp::Sub, tag("-")),
            value(ProtocolBinOp::Concat, tag("++")),
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
            value(ProtocolBinOp::Mul, tag("*")),
            value(ProtocolBinOp::Div, tag("/")),
            value(ProtocolBinOp::Mod, tag("%")),
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
            HlsExpr::UnaryOp { op: ProtocolUnaryOp::Not, expr: Box::new(e) }
        }),
        map(preceded(tuple((tag("-"), multispace0)), parse_unary_expr), |e| {
            HlsExpr::UnaryOp { op: ProtocolUnaryOp::Neg, expr: Box::new(e) }
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
        
        // Should push the string and then call DefineTextFromStack
        assert!(binary.instructions.iter().any(|i| matches!(i,
            Instruction::Push(val) if val.as_str() == Some("Hello World")
        )));
        assert!(binary.instructions.iter().any(|i| matches!(i,
            Instruction::DefineTextFromStack
        )));
    }

    #[test]
    fn test_compile_conditional() {
        let source = r#"
            if true {
                element visible {}
            } else {
                element hidden {}
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // Should only compile the "visible" branch since condition is true
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
            assert!(matches!(op, ProtocolBinOp::Add));
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

    #[test]
    fn test_function_inlining() {
        let source = r#"
            fn add(a, b) {
                return a + b
            }
            element div {
                text add(10, 20)
            }
        "#;
        let binary = Compiler::compile(source).unwrap();
        
        // add(10, 20) should be inlined to 10 + 20 and then folded to 30
        assert!(binary.instructions.iter().any(|i| matches!(i,
            Instruction::Push(val) if val.as_f64() == Some(30.0)
        )));
        
        // Also check that there's no Call instruction for "add" if it was inlined
        assert!(!binary.instructions.iter().any(|i| matches!(i,
            Instruction::Call { .. }
        )));
    }
}
