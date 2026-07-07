use super::super::ir::{
    FieldRef, IrArithmeticOp, IrCompareOp, IrConst, IrInstr, IrParam, IrType, IrUnaryOp, MethodRef,
    TrapReason, ValueId,
};
use anyhow::{Context, Result, bail};

/// A value together with its static type, as tracked on the abstract frame
/// (operand stack + locals) during lowering. The type travels with the value
/// so block parameters can be typed when merge points are discovered.
#[derive(Clone, Debug)]
struct TypedValue {
    value: ValueId,
    ty: IrType,
}

/// The parameters a block receives from its predecessors (the phi equivalent).
///
/// A block's entry frame is the set of live locals plus the operand stack at
/// block entry. Each such slot becomes a positional block parameter; every
/// predecessor edge passes its own value for that slot as a branch argument.
#[derive(Clone, Debug)]
pub(super) struct BlockEntry {
    /// Local indices that are live at block entry, in ascending order. Parallel
    /// to the first `local_slots.len()` entries of `params`.
    local_slots: Vec<u16>,
    /// Operand stack depth at block entry. The trailing `stack_len` entries of
    /// `params` are the stack slots, bottom-to-top.
    stack_len: usize,
    /// Block parameters, ordered `[locals-by-index] ++ [stack bottom-to-top]`.
    params: Vec<IrParam>,
}

impl BlockEntry {
    pub(super) fn params(&self) -> &[IrParam] {
        &self.params
    }
}

#[derive(Debug)]
pub(super) struct LowerState {
    next_value: u32,
    locals: Vec<Option<TypedValue>>,
    stack: Vec<TypedValue>,
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

    /// Capture the current frame as a fresh set of block parameters for a
    /// successor block. Each live local and stack slot gets a new parameter
    /// value; predecessors later pass their own values via [`Self::branch_args`].
    pub(super) fn capture_block_entry(&mut self) -> Result<BlockEntry> {
        let defined_locals = self
            .locals
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|typed| (index, typed.ty.clone())))
            .collect::<Vec<_>>();
        let stack_types = self
            .stack
            .iter()
            .map(|typed| typed.ty.clone())
            .collect::<Vec<_>>();

        let mut local_slots = Vec::with_capacity(defined_locals.len());
        let mut params = Vec::with_capacity(defined_locals.len() + stack_types.len());
        for (index, ty) in defined_locals {
            let index = u16::try_from(index).context("fvm-aot lowerer local index exceeded u16")?;
            let value = self.new_value();
            local_slots.push(index);
            params.push(IrParam { value, ty });
        }
        let stack_len = stack_types.len();
        for ty in stack_types {
            let value = self.new_value();
            params.push(IrParam { value, ty });
        }
        Ok(BlockEntry {
            local_slots,
            stack_len,
            params,
        })
    }

    /// Gather the current frame values to pass along an edge into a block with
    /// the given entry shape. Fails loudly when the frame does not supply every
    /// parameter slot (a merge where a local is defined on one path but not
    /// another) rather than silently miscompiling.
    pub(super) fn branch_args(&self, entry: &BlockEntry) -> Result<Vec<ValueId>> {
        let mut args = Vec::with_capacity(entry.params.len());
        for &local in &entry.local_slots {
            let typed = self
                .locals
                .get(usize::from(local))
                .and_then(|slot| slot.as_ref())
                .with_context(|| {
                    format!(
                        "fvm-aot lowerer cannot merge frames: local {local} is undefined on this path"
                    )
                })?;
            args.push(typed.value);
        }
        if self.stack.len() != entry.stack_len {
            bail!(
                "fvm-aot lowerer cannot merge frames: operand stack depth {} does not match block entry depth {}",
                self.stack.len(),
                entry.stack_len
            );
        }
        for slot in &self.stack {
            args.push(slot.value);
        }
        Ok(args)
    }

    /// Reinitialize the frame from a block's own parameters before lowering its
    /// body.
    pub(super) fn restore_block_entry(&mut self, entry: &BlockEntry) {
        for slot in &mut self.locals {
            *slot = None;
        }
        self.stack.clear();
        for (index, &local) in entry.local_slots.iter().enumerate() {
            let param = &entry.params[index];
            self.store_local(
                local,
                TypedValue {
                    value: param.value,
                    ty: param.ty.clone(),
                },
            );
        }
        for slot in 0..entry.stack_len {
            let param = &entry.params[entry.local_slots.len() + slot];
            self.stack.push(TypedValue {
                value: param.value,
                ty: param.ty.clone(),
            });
        }
    }

    pub(super) fn emit(&mut self, instr: IrInstr) {
        self.instrs.push(instr);
    }

    pub(super) fn push_param(&mut self, local: u16, ty: IrType) -> ValueId {
        let value = self.new_value();
        self.store_local(
            local,
            TypedValue {
                value,
                ty: ty.clone(),
            },
        );
        self.instrs.push(IrInstr::Param(value, local, ty));
        value
    }

    pub(super) fn push_constant(&mut self, constant: IrConst) -> ValueId {
        let ty = constant_type(&constant);
        let value = self.emit_constant(constant);
        self.stack.push(TypedValue { value, ty });
        value
    }

    pub(super) fn emit_constant(&mut self, constant: IrConst) -> ValueId {
        let value = self.new_value();
        self.instrs.push(IrInstr::Constant(value, constant));
        value
    }

    pub(super) fn push_loaded_local(&mut self, index: u16) -> Result<()> {
        let typed = self.load_local(index)?;
        self.stack.push(typed);
        Ok(())
    }

    pub(super) fn store_popped_local(&mut self, index: u16) -> Result<()> {
        let typed = self.pop_typed()?;
        self.store_local(index, typed);
        Ok(())
    }

    pub(super) fn increment_local(&mut self, index: u16, delta: i32) -> Result<()> {
        let lhs = self.load_local(index)?.value;
        let rhs = self.new_value();
        self.instrs
            .push(IrInstr::Constant(rhs, IrConst::Int(delta)));
        let value = self.new_value();
        self.instrs
            .push(IrInstr::Arithmetic(value, IrArithmeticOp::Add, lhs, rhs));
        self.store_local(
            index,
            TypedValue {
                value,
                ty: IrType::Int,
            },
        );
        Ok(())
    }

    pub(super) fn push_binary(&mut self, op: IrArithmeticOp) -> Result<()> {
        let rhs = self.pop_stack()?;
        let lhs = self.pop_stack()?;
        let value = self.new_value();
        self.instrs.push(IrInstr::Arithmetic(value, op, lhs, rhs));
        self.stack.push(TypedValue {
            value,
            ty: IrType::Int,
        });
        Ok(())
    }

    pub(super) fn push_checked_binary(&mut self, op: IrArithmeticOp) -> Result<()> {
        let rhs = self.pop_stack()?;
        let lhs = self.pop_stack()?;
        self.instrs
            .push(IrInstr::ZeroCheck(rhs, TrapReason::DivideByZero));
        let value = self.new_value();
        self.instrs.push(IrInstr::Arithmetic(value, op, lhs, rhs));
        self.stack.push(TypedValue {
            value,
            ty: IrType::Int,
        });
        Ok(())
    }

    pub(super) fn push_unary(&mut self, op: IrUnaryOp) -> Result<()> {
        let input = self.pop_stack()?;
        let value = self.new_value();
        self.instrs.push(IrInstr::Unary(value, op, input));
        self.stack.push(TypedValue {
            value,
            ty: IrType::Int,
        });
        Ok(())
    }

    pub(super) fn pop_stack(&mut self) -> Result<ValueId> {
        self.pop_typed().map(|typed| typed.value)
    }

    /// The JVM stack-manipulation opcodes. Every value we model today occupies a
    /// single slot (int/boolean/char/reference — category 1; long/double are
    /// rejected before reaching here), so the category-2 forms collapse into the
    /// category-1 shapes below. Duplication is free in SSA: a slot only holds a
    /// value id, so a "copy" reuses the same id.
    pub(super) fn pop2(&mut self) -> Result<()> {
        self.pop_typed()?;
        self.pop_typed()?;
        Ok(())
    }

    pub(super) fn dup(&mut self) -> Result<()> {
        let top = self.peek(0)?;
        self.stack.push(top);
        Ok(())
    }

    pub(super) fn dup_x1(&mut self) -> Result<()> {
        let v1 = self.pop_typed()?;
        let v2 = self.pop_typed()?;
        self.stack.push(v1.clone());
        self.stack.push(v2);
        self.stack.push(v1);
        Ok(())
    }

    pub(super) fn dup_x2(&mut self) -> Result<()> {
        let v1 = self.pop_typed()?;
        let v2 = self.pop_typed()?;
        let v3 = self.pop_typed()?;
        self.stack.push(v1.clone());
        self.stack.push(v3);
        self.stack.push(v2);
        self.stack.push(v1);
        Ok(())
    }

    pub(super) fn dup2(&mut self) -> Result<()> {
        let v1 = self.peek(0)?;
        let v2 = self.peek(1)?;
        self.stack.push(v2);
        self.stack.push(v1);
        Ok(())
    }

    pub(super) fn dup2_x1(&mut self) -> Result<()> {
        let v1 = self.pop_typed()?;
        let v2 = self.pop_typed()?;
        let v3 = self.pop_typed()?;
        self.stack.push(v2.clone());
        self.stack.push(v1.clone());
        self.stack.push(v3);
        self.stack.push(v2);
        self.stack.push(v1);
        Ok(())
    }

    pub(super) fn dup2_x2(&mut self) -> Result<()> {
        let v1 = self.pop_typed()?;
        let v2 = self.pop_typed()?;
        let v3 = self.pop_typed()?;
        let v4 = self.pop_typed()?;
        self.stack.push(v2.clone());
        self.stack.push(v1.clone());
        self.stack.push(v4);
        self.stack.push(v3);
        self.stack.push(v2);
        self.stack.push(v1);
        Ok(())
    }

    pub(super) fn swap(&mut self) -> Result<()> {
        let len = self.stack.len();
        if len < 2 {
            bail!("fvm-aot lowerer swap needs two operands, stack depth {len}");
        }
        self.stack.swap(len - 1, len - 2);
        Ok(())
    }

    /// Clone the stack slot `depth` below the top (0 = top).
    fn peek(&self, depth: usize) -> Result<TypedValue> {
        let len = self.stack.len();
        len.checked_sub(depth + 1)
            .and_then(|index| self.stack.get(index))
            .cloned()
            .with_context(|| {
                format!("fvm-aot lowerer stack underflow reading depth {depth} of {len}")
            })
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

    pub(super) fn push_new_object(&mut self, class: String) {
        let value = self.new_value();
        self.instrs.push(IrInstr::NewObject(value, class.clone()));
        self.stack.push(TypedValue {
            value,
            ty: IrType::Object(class),
        });
    }

    pub(super) fn push_field_get(&mut self, field: FieldRef) -> Result<()> {
        let receiver = self.pop_typed()?;
        let value = self.new_value();
        let ty = field.ty.clone();
        self.instrs
            .push(IrInstr::FieldGet(value, field, Some(receiver.value)));
        self.stack.push(TypedValue { value, ty });
        Ok(())
    }

    pub(super) fn store_field_put(&mut self, field: FieldRef) -> Result<()> {
        let value = self.pop_typed()?;
        let receiver = self.pop_typed()?;
        self.instrs
            .push(IrInstr::FieldPut(field, Some(receiver.value), value.value));
        Ok(())
    }

    pub(super) fn push_new_array(&mut self, element: IrType) -> Result<()> {
        let length = self.pop_stack()?;
        let value = self.new_value();
        self.instrs
            .push(IrInstr::NewArray(value, element.clone(), length));
        self.stack.push(TypedValue {
            value,
            ty: IrType::Array(Box::new(element)),
        });
        Ok(())
    }

    pub(super) fn push_array_length(&mut self) -> Result<()> {
        let array = self.pop_stack()?;
        let value = self.new_value();
        self.instrs.push(IrInstr::ArrayLength(value, array));
        self.stack.push(TypedValue {
            value,
            ty: IrType::Int,
        });
        Ok(())
    }

    pub(super) fn push_array_load(&mut self, element: IrType) -> Result<()> {
        let index = self.pop_stack()?;
        let array = self.pop_stack()?;
        let value = self.new_value();
        self.instrs
            .push(IrInstr::ArrayLoad(value, array, index, element.clone()));
        self.stack.push(TypedValue { value, ty: element });
        Ok(())
    }

    pub(super) fn store_array_store(&mut self, element: IrType) -> Result<()> {
        let value = self.pop_stack()?;
        let index = self.pop_stack()?;
        let array = self.pop_stack()?;
        self.instrs
            .push(IrInstr::ArrayStore(array, index, value, element));
        Ok(())
    }

    pub(super) fn push_static_call(
        &mut self,
        method: MethodRef,
        args: Vec<ValueId>,
        return_type: IrType,
    ) {
        match return_type {
            IrType::Void => self.instrs.push(IrInstr::Call(None, method, args)),
            ty => {
                let value = self.new_value();
                self.instrs.push(IrInstr::Call(Some(value), method, args));
                self.stack.push(TypedValue { value, ty });
            }
        }
    }

    fn pop_typed(&mut self) -> Result<TypedValue> {
        self.stack.pop().context("fvm-aot lowerer stack underflow")
    }

    fn load_local(&self, index: u16) -> Result<TypedValue> {
        self.locals
            .get(usize::from(index))
            .and_then(|slot| slot.clone())
            .with_context(|| format!("fvm-aot lowerer read uninitialized local {index}"))
    }

    fn store_local(&mut self, index: u16, typed: TypedValue) {
        let index = usize::from(index);
        if index >= self.locals.len() {
            self.locals.resize(index + 1, None);
        }
        self.locals[index] = Some(typed);
    }

    fn new_value(&mut self) -> ValueId {
        let value = ValueId::new(self.next_value);
        self.next_value += 1;
        value
    }
}

fn constant_type(constant: &IrConst) -> IrType {
    match constant {
        IrConst::Int(_) => IrType::Int,
        IrConst::Boolean(_) => IrType::Boolean,
        IrConst::Char(_) => IrType::Char,
        IrConst::Null => IrType::Object("null".to_string()),
        IrConst::String(_) => IrType::Object("java/lang/String".to_string()),
    }
}
