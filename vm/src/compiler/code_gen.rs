use parser::{
    BinOper, CodeRange, Expr, ExprNode, Index, LValue, ListContent, LogicalOper, Slice, Stmt,
    StmtNode, Stmts, UnOper,
};

use super::{Chunk, CompRes, Compiler, OpCode};
use crate::value::Value;

mod conditionals;
mod function;

impl Compiler<'_> {
    pub fn compile_statement(&mut self, statement: &StmtNode, chunk: &mut Chunk, output: bool) {
        let StmtNode {
            node,
            start_loc,
            end_loc,
        } = statement;
        let range = CodeRange::from_locs(*start_loc, *end_loc);

        let res = match node.as_ref() {
            Stmt::Decl(lvalue, expr) => {
                self.compile_declaration(lvalue, expr.as_ref(), range.clone(), chunk)
            }
            Stmt::Expr(expr) => {
                let res = self.compile_expression(expr, chunk);
                if !output {
                    // Keep the value if the statement should keep output
                    chunk.push_opcode(OpCode::Discard, range.clone());
                }
                res
            }
            Stmt::Invalid => todo!(),
        };

        if let Err(reason) = res {
            // TODO: Push some garbage opcode?
            eprintln!("COMPILER ERROR: [{range}] {reason}");
            self.had_error = true;
        }
    }

    fn compile_declaration(
        &mut self,
        lvalue: &LValue,
        expr: Option<&ExprNode>,
        range: CodeRange,
        chunk: &mut Chunk,
    ) -> CompRes {
        if !self.is_global() {
            self.declare_local(lvalue, range.clone(), chunk, true)?;
        }
        if let Some(expr) = expr {
            self.compile_lvalue_assignment(lvalue, expr, range.clone(), chunk)?;
            chunk.push_opcode(OpCode::Discard, range);
        }
        Ok(())
    }

    /// Declares a local
    ///
    /// Mostly no codegen, but it assigns pointers to NIL for declared pointers.
    /// lvalue_top specifies if this is the topmost lvalue in a declaration
    fn declare_local(
        &mut self,
        lvalue: &LValue,
        range: CodeRange,
        chunk: &mut Chunk,
        lvalue_top: bool,
    ) -> CompRes {
        match lvalue {
            LValue::Index(_, _) => todo!(),
            LValue::Var(name) => {
                if self.attributes.is_upvalue(name) {
                    // Declares the local as a pointer insteal of a flat value
                    let offset = self.locals.add_local(name.to_owned(), true);

                    // TODO: THis could be done with eg semantic analysis help in the first assignment
                    // which would save computation, and remove codegen from this function.

                    // Assign it a new empty pointer
                    chunk.push_opcode(OpCode::EmptyPointer, range.clone());
                    chunk.push_opcode(OpCode::AssignLocal, range.clone());
                    chunk.push_u8_offset(offset);
                    chunk.push_opcode(OpCode::Discard, range.clone()); // TODO: This is not that nice
                } else {
                    self.locals.add_local(name.to_owned(), false);
                }
            }
            LValue::Tuple(_) => todo!(),
            LValue::Constant(_) if lvalue_top => return Err(format!("Cannot declare a constant")),
            LValue::Constant(_) => (),
        }
        Ok(())
    }

    pub fn declare_global(&mut self, name: &str) -> usize {
        let len = self.globals.len();
        *self.globals.entry(name.to_string()).or_insert(len)
    }

    // TODO: Variable resolution
    fn compile_expression(&mut self, expr: &ExprNode, chunk: &mut Chunk) -> CompRes {
        let ExprNode {
            node,
            start_loc,
            end_loc,
        } = expr;
        let range = CodeRange::from_locs(*start_loc, *end_loc);

        match node.as_ref() {
            Expr::Call(func, args) => self.compile_call(func, args, range, chunk)?,
            Expr::IndexInto(base, index) => self.compile_index_into(base, index, range, chunk)?,
            Expr::Binary(x, binop, y) => {
                self.compile_expression(x, chunk)?;
                self.compile_expression(y, chunk)?;
                let opcode = binop_opcode_conv(binop);
                chunk.push_opcode(opcode, range);
            }
            Expr::Unary(unop, x) => {
                self.compile_expression(x, chunk)?;
                let opcode = unop_opcode_conv(unop);
                chunk.push_opcode(opcode, range);
            }
            Expr::Logical(lhs, LogicalOper::And, rhs) => {
                self.compile_and(lhs, rhs, range, chunk)?
            }
            Expr::Logical(lhs, LogicalOper::Or, rhs) => self.compile_or(lhs, rhs, range, chunk)?,
            Expr::Assign(lvalue, expr) => {
                self.compile_lvalue_assignment(lvalue, expr, range, chunk)?;
            }
            Expr::Var(name) => self.compile_var(name, range, chunk)?,
            Expr::Int(x) => chunk.push_constant_plus(Value::Int(*x), range),
            Expr::Float(x) => chunk.push_constant_plus(Value::Float(*x), range),
            Expr::Bool(x) => chunk.push_constant_plus(Value::Bool(*x), range),
            Expr::String(_) => todo!(),
            Expr::Block(stmts) => self.compile_block(stmts, range, chunk),
            Expr::If(pred, then, otherwise) => {
                self.compile_if(pred, then, otherwise.as_ref(), range, chunk)?
            }
            Expr::While(pred, body) => self.compile_while(pred, body, range, chunk)?,
            Expr::For(lvalue, collection, body) => {
                self.compile_for(lvalue, collection, body, range, chunk)?
            }
            Expr::Break => self.compile_break(range, chunk)?,
            Expr::Continue => self.compile_continue(range, chunk)?,
            Expr::Return(opt_expr) => self.compile_return(opt_expr.as_ref(), range, chunk)?,
            Expr::Nil => chunk.push_constant_plus(Value::Nil, range),
            Expr::List(list) => self.compile_list(list, range, chunk)?,
            Expr::Tuple(_) => todo!(),
            Expr::FunctionDefinition(name, params, body) => {
                let upvalues = self.attributes.upvalue_names(node.as_ref()).unwrap_or(&[]);
                let rec_name = self.attributes.rec_name(node.as_ref());
                let nbr_locals = self
                    .attributes
                    .local_count(node.as_ref())
                    .expect("Function must have local count"); // TODO: DO this in compilation phase?

                self.compile_function_def(
                    name, rec_name, params, body, upvalues, nbr_locals, range, chunk,
                )?;
            }
            Expr::Match(_, _) => todo!(),
        };

        Ok(())
    }

    fn compile_lvalue_assignment(
        &mut self,
        lvalue: &LValue,
        expr: &ExprNode,
        range: CodeRange,
        chunk: &mut Chunk,
    ) -> CompRes {
        self.compile_expression(expr, chunk)?;
        self.compile_lvalue_stack_assign(lvalue, range, chunk)
    }

    /// Assigns the top value on the temp stack to the lvalue
    fn compile_lvalue_stack_assign(
        &mut self,
        lvalue: &LValue,
        range: CodeRange,
        chunk: &mut Chunk,
    ) -> CompRes {
        match lvalue {
            LValue::Index(collection, index) => {
                self.compile_assign_index(collection, index, range, chunk)
            }
            LValue::Var(name) => self.compile_assign(name, range, chunk),
            LValue::Tuple(_) => todo!(),
            LValue::Constant(_) => todo!(),
        }
    }

    /// Compiles code to assign the top-most temp stack value into an indexed value such as list[index]
    fn compile_assign_index(
        &mut self,
        collection: &ExprNode,
        index: &Index,
        range: CodeRange,
        chunk: &mut Chunk,
    ) -> CompRes {
        self.compile_expression(collection, chunk)?;

        match index {
            Index::At(expr_at) => {
                self.compile_expression(expr_at, chunk)?;
                chunk.push_opcode(OpCode::AssignAtIndex, range);
            }
            Index::Slice(_slice) => {
                todo!("Implement assigning into slice (light pattern matching)")
            }
        }

        Ok(())
    }

    /// Assigns the topmost temp value to the named variable
    fn compile_assign(&mut self, name: &str, range: CodeRange, chunk: &mut Chunk) -> CompRes {
        // First checks if it is local
        if let Some((offset, pointer)) = self.locals.get_local(name) {
            if !pointer {
                chunk.push_opcode(OpCode::AssignLocal, range);
            } else {
                chunk.push_opcode(OpCode::AssignPointer, range);
            }
            chunk.push_u8_offset(offset);
            Ok(())
        } else if let Some(offset) = self.locals.get_upvalue(name) {
            chunk.push_opcode(OpCode::AssignUpValue, range);
            chunk.push_u8_offset(offset as u8);
            Ok(())
        } else if let Some(&offset) = self.globals.get(name) {
            chunk.push_opcode(OpCode::AssignGlobal, range); // Maybe bad range choice
            chunk.push_u8_offset(offset as u8);
            Ok(())
        } else {
            Err(format!("Global var '{name}' is not declared"))
        }
    }

    /// Compiles the expression, or just push Nil (without location) if no expression
    fn compile_opt_expression(&mut self, expr: Option<&ExprNode>, chunk: &mut Chunk) -> CompRes {
        // Maybe we should be smarter and never push such a Nil value
        match expr {
            Some(expr) => self.compile_expression(expr, chunk)?,
            None => chunk.push_constant_plus(Value::Nil, CodeRange::from_ints(0, 0, 0, 0, 0, 0)),
        };
        Ok(())
    }

    /// Compiles the read of a var.
    fn compile_var(&mut self, name: &str, range: CodeRange, chunk: &mut Chunk) -> CompRes {
        if let Some((offset, pointer)) = self.locals.get_local(name) {
            if !pointer {
                chunk.push_opcode(OpCode::ReadLocal, range);
            } else {
                chunk.push_opcode(OpCode::ReadPointer, range);
            }
            chunk.push_u8_offset(offset);
            Ok(())
        } else if let Some(offset) = self.locals.get_upvalue(name) {
            chunk.push_opcode(OpCode::ReadUpValue, range);
            chunk.push_u8_offset(offset as u8);
            Ok(())
        } else if let Some(offset) = self.globals.get(name) {
            chunk.push_opcode(OpCode::ReadGlobal, range);
            chunk.push_u8_offset(*offset as u8);
            Ok(())
        } else {
            // ERROR: Compile error!
            Err(format!("Var '{name}' is not declared"))
        }
    }

    /// Declare all top-level declarations, so as to use late-binding
    pub fn declare_globals(&mut self, stmts: &Stmts) {
        for stmt in stmts.stmts.iter() {
            if let Stmt::Decl(lvalue, _) = stmt.node.as_ref() {
                self.declare_global_lvalue(lvalue);
            }
        }
    }

    fn declare_global_lvalue(&mut self, lvalue: &LValue) {
        match lvalue {
            LValue::Index(_, _) => (),
            LValue::Var(name) => {
                self.declare_global(name);
            }
            LValue::Tuple(lvalues) => {
                for lvalue in lvalues.iter() {
                    self.declare_global_lvalue(lvalue);
                }
            }
            LValue::Constant(_) => (),
        }
    }

    /// Compiles the block of statements. Does not throw, as errors are printed and escaped within.
    fn compile_block(&mut self, stmts: &Stmts, range: CodeRange, chunk: &mut Chunk) {
        self.locals.enter();
        self.compile_stmts(stmts, chunk);
        if !stmts.output || stmts.stmts.is_empty() {
            chunk.push_opcode(OpCode::Nil, range.clone());
        }

        let pointer_offsets = self.locals.exit();
        self.drop_pointers(&pointer_offsets, range, chunk);
    }

    /// Explicitly drops pointers at the specified offsets from rbp
    fn drop_pointers(&mut self, offsets: &[u8], range: CodeRange, chunk: &mut Chunk) {
        for &offset in offsets {
            chunk.push_opcode(OpCode::Drop, range.clone());
            chunk.push_u8_offset(offset);
        }
    }

    /// Compiles a list constant
    fn compile_list(&mut self, list: &ListContent, range: CodeRange, chunk: &mut Chunk) -> CompRes {
        match list {
            ListContent::Exprs(exprs) => {
                if exprs.len() > 255 {
                    // As we store the length in a byte we cannot store too many
                    return Err(format!(
                        "Cannot init list with over 255 values :( This one is {} long",
                        exprs.len()
                    ));
                }

                for expr in exprs {
                    self.compile_expression(expr, chunk)?;
                }
                chunk.push_opcode(OpCode::ListFromValues, range);
                chunk.push_u8_offset(exprs.len() as u8);
            }
            ListContent::Range(slice) => {
                self.compile_slice(slice, chunk)?;
                chunk.push_opcode(OpCode::ListFromSlice, range);
            }
        }
        Ok(())
    }

    /// Compiles computations for the three parts of the slice
    ///
    /// If any of the fields are omitted, a NIL is pushed instead
    fn compile_slice(&mut self, slice: &Slice, chunk: &mut Chunk) -> CompRes {
        self.compile_opt_expression(slice.start.as_ref(), chunk)?;
        self.compile_opt_expression(slice.stop.as_ref(), chunk)?;
        self.compile_opt_expression(slice.step.as_ref(), chunk)
    }

    /// Compiles the indexing into a list
    fn compile_index_into(
        &mut self,
        base: &ExprNode,
        index: &Index,
        range: CodeRange,
        chunk: &mut Chunk,
    ) -> CompRes {
        self.compile_expression(base, chunk)?;
        match index {
            Index::At(at) => {
                self.compile_expression(at, chunk)?;
                chunk.push_opcode(OpCode::ReadAtIndex, range);
            }
            Index::Slice(slice) => {
                self.compile_slice(slice, chunk)?;
                chunk.push_opcode(OpCode::ReadAtSlice, range);
            }
        }
        Ok(())
    }
}

fn binop_opcode_conv(binop: &BinOper) -> OpCode {
    match binop {
        BinOper::Add => OpCode::Add,
        BinOper::Sub => OpCode::Subtract,
        BinOper::Div => OpCode::Divide,
        BinOper::Mult => OpCode::Multiply,
        BinOper::Mod => OpCode::Modulo,
        BinOper::Pow => OpCode::Power,
        BinOper::Eq => OpCode::Equality,
        BinOper::Neq => OpCode::NonEquality,
        BinOper::Lt => OpCode::LessThan,
        BinOper::Leq => OpCode::LessEqual,
        BinOper::Gt => OpCode::GreaterThan,
        BinOper::Geq => OpCode::GreaterEqual,
        BinOper::Append => todo!(),
    }
}

fn unop_opcode_conv(unop: &UnOper) -> OpCode {
    match unop {
        UnOper::Not => OpCode::Not,
        UnOper::Sub => OpCode::Negate,
    }
}
