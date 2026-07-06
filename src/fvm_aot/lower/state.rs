use super::super::ir::{
    IrArithmeticOp, IrCompareOp, IrConst, IrInstr, IrType, IrUnaryOp, MethodRef, TrapReason,
    ValueId,
};
use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub(super) struct FrameSnapshot {
    locals: Vec<Option<ValueId>>,
    stack: Vec<ValueId>,
}

#[derive(Debug)]
pub(super) struct LowerState {
    next_value: u32,
    locals: Vec<Option<ValueId>>,
    stack: Vec<ValueId>,
    instrs: Vec<IrInstr>,
}

impl LowerState {
    pub(super) fn new(max_locals: u16) -> Self {
        Self {
            next_value: 0,
            locals: vec![None; usize::from(max_locals)],
            stack: Vec::new(),
            instrs: Vec::new(),
        }
    }

    pub(super) fn finish_block(&mut self) -> Vec<IrInstr> {
        std::mem::take(&mut self.instrs)
    }

    pub(super) fn snapshot(&self) -> FrameSnapshot {
        FrameSnapshot {
            locals: self.locals.clone(),
            stack: self.stack.clone(),
        }
    }

    pub(super) fn restore(&mut self, snapshot: &FrameSnapshot) {
        self.locals.clone_from(&snapshot.locals);
        self.stack.clone_from(&snapshot.stack);
    }

    pub(super) fn emit(&mut self, instr: IrInstr) {
        self.instrs.push(instr);
    }

    pub(super) fn push_param(&mut self, local: u16, ty: IrType) -> ValueId {
        let value = self.new_value();
        self.store_local(local, value);
        self.instrs.push(IrInstr::Param(value, local, ty));
        value
    }

    pub(super) fn push_constant(&mut self, constant: IrConst) -> ValueId {
        let value = self.emit_constant(constant);
        self.stack.push(value);
        value
    }

    pub(super) fn emit_constant(&mut self, constant: IrConst) -> ValueId {
        let value = self.new_value();
        self.instrs.push(IrInstr::Constant(value, constant));
        value
    }

    pub(super) fn push_loaded_local(&mut self, index: u16) -> Result<()> {
        let value = self.load_local(index)?;
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn store_popped_local(&mut self, index: u16) -> Result<()> {
        let value = self.pop_stack()?;
        self.store_local(index, value);
        Ok(())
    }

    pub(super) fn increment_local(&mut self, index: u16, delta: i32) -> Result<()> {
        let lhs = self.load_local(index)?;
        let rhs = self.new_value();
        self.instrs
            .push(IrInstr::Constant(rhs, IrConst::Int(delta)));
        let value = self.new_value();
        self.instrs
            .push(IrInstr::Arithmetic(value, IrArithmeticOp::Add, lhs, rhs));
        self.store_local(index, value);
        Ok(())
    }

    pub(super) fn push_binary(&mut self, op: IrArithmeticOp) -> Result<()> {
        let rhs = self.pop_stack()?;
        let lhs = self.pop_stack()?;
        let value = self.new_value();
        self.instrs.push(IrInstr::Arithmetic(value, op, lhs, rhs));
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn push_checked_binary(&mut self, op: IrArithmeticOp) -> Result<()> {
        let rhs = self.pop_stack()?;
        let lhs = self.pop_stack()?;
        self.instrs
            .push(IrInstr::ZeroCheck(rhs, TrapReason::DivideByZero));
        let value = self.new_value();
        self.instrs.push(IrInstr::Arithmetic(value, op, lhs, rhs));
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn push_unary_neg(&mut self) -> Result<()> {
        let input = self.pop_stack()?;
        let value = self.new_value();
        self.instrs
            .push(IrInstr::Unary(value, IrUnaryOp::Neg, input));
        self.stack.push(value);
        Ok(())
    }

    pub(super) fn pop_stack(&mut self) -> Result<ValueId> {
        self.stack.pop().context("fvm-aot lowerer stack underflow")
    }

    pub(super) fn push_compare(
        &mut self,
        op: IrCompareOp,
        lhs: ValueId,
        rhs: Option<ValueId>,
    ) -> ValueId {
        let value = self.new_value();
        self.instrs.push(IrInstr::Compare(value, op, lhs, rhs));
        value
    }

    pub(super) fn push_static_call(
        &mut self,
        method: MethodRef,
        args: Vec<ValueId>,
        return_type: IrType,
    ) {
        match return_type {
            IrType::Void => self.instrs.push(IrInstr::Call(None, method, args)),
            _ => {
                let value = self.new_value();
                self.instrs.push(IrInstr::Call(Some(value), method, args));
                self.stack.push(value);
            }
        }
    }

    fn load_local(&self, index: u16) -> Result<ValueId> {
        self.locals
            .get(usize::from(index))
            .and_then(|value| *value)
            .with_context(|| format!("fvm-aot lowerer read uninitialized local {index}"))
    }

    fn store_local(&mut self, index: u16, value: ValueId) {
        let index = usize::from(index);
        if index >= self.locals.len() {
            self.locals.resize(index + 1, None);
        }
        self.locals[index] = Some(value);
    }

    fn new_value(&mut self) -> ValueId {
        let value = ValueId::new(self.next_value);
        self.next_value += 1;
        value
    }
}
