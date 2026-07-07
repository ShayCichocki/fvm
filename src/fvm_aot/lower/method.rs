use super::super::classfile::{ClassFile, Code, Method};
use super::super::diagnostics::unsupported_opcode_message;
use super::super::ir::{
    BasicBlockId, BasicBlockIr, BranchEdge, FunctionIr, IrArithmeticOp, IrConst, IrInstr, IrParam,
    IrType, IrUnaryOp,
};
use super::super::types::{JvmType, parse_method_descriptor};
use super::bytecode::{
    BlockPlan, BranchOperands, branch_operands, branch_target, parse_switch, plan_blocks, read_i16,
    read_u8, read_u16,
};
use super::calls::{
    CallLowering, lower_getfield, lower_getstatic, lower_invokedynamic, lower_invokespecial,
    lower_invokestatic, lower_invokevirtual, lower_new, lower_putfield, push_ldc_constant,
};
use super::metadata::{ir_name, ir_type_for_jvm, method_label};
use super::state::{BlockEntry, LowerState};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

pub(in crate::fvm_aot) fn lower_method_to_ir(
    class_file: &ClassFile,
    method: &Method,
) -> Result<FunctionIr> {
    let Some(code) = method.code.as_ref() else {
        bail!(
            "fvm-aot lowerer requires Code for {}",
            method_label(class_file, method)
        );
    };
    if code.has_exception_table {
        bail!(
            "fvm-aot lowerer cannot compile {}: it declares exception handlers (try/catch/finally); \
             required feature: exception handlers; planned milestone: exceptions (see docs/PUNCHLIST.md Phase 5)",
            method_label(class_file, method)
        );
    }
    let (param_types, return_type) = parse_method_descriptor(&method.descriptor)?;
    let method_label = method_label(class_file, method);
    let plan = plan_blocks(code, &method_label)?;
    MethodLowerer {
        class_file,
        method,
        code,
        method_label,
        plan,
        pc: 0,
        block_entries: BTreeMap::new(),
        state: LowerState::new(code.max_locals),
    }
    .lower(&param_types, &return_type)
}

struct MethodLowerer<'a> {
    class_file: &'a ClassFile,
    method: &'a Method,
    code: &'a Code,
    method_label: String,
    plan: BlockPlan,
    pc: usize,
    block_entries: BTreeMap<BasicBlockId, BlockEntry>,
    state: LowerState,
}

impl<'a> MethodLowerer<'a> {
    fn lower(mut self, param_types: &[JvmType], return_type: &JvmType) -> Result<FunctionIr> {
        let return_type = ir_type_for_jvm(return_type, "return", &self.method_label)?;
        let entry = self.plan.entry();
        self.reject_entry_backedge(entry)?;

        let mut function_params = None;
        let mut block_instrs: BTreeMap<BasicBlockId, Vec<IrInstr>> = BTreeMap::new();
        let mut block_params: BTreeMap<BasicBlockId, Vec<IrParam>> = BTreeMap::new();

        for block_id in self.plan.reverse_postorder() {
            let range = self.plan.range(block_id)?;
            self.pc = range.start;
            if block_id == entry {
                function_params = Some(self.seed_entry_frame(param_types)?);
            } else {
                let block_entry = self.block_entries.get(&block_id).cloned().with_context(|| {
                    format!(
                        "fvm-aot lowerer block {block_id} was reached before its entry frame was defined (irreducible control flow is unsupported)"
                    )
                })?;
                block_params.insert(block_id, block_entry.params().to_vec());
                self.state.restore_block_entry(&block_entry);
            }

            let mut terminated = false;
            while self.pc < range.end && !terminated {
                let opcode_pc = self.pc;
                let opcode = read_u8(&self.code.bytes, &mut self.pc)?;
                terminated = self.lower_opcode(opcode, opcode_pc).with_context(|| {
                    format!(
                        "fvm-aot lowerer bytecode error in {} at bci {} (opcode 0x{:02x})",
                        self.method_label, opcode_pc, opcode
                    )
                })?;
            }
            if !terminated {
                if range.end >= self.code.bytes.len() {
                    bail!(
                        "fvm-aot lowerer method {} ended without return",
                        self.method_label
                    );
                }
                let fallthrough = self.plan.block_id_for_bci(range.end)?;
                let edge = self.edge_to(fallthrough)?;
                self.state.emit(IrInstr::Branch(edge));
            }
            block_instrs.insert(block_id, self.state.finish_block());
        }

        let params = function_params.unwrap_or_default();
        let mut blocks = Vec::with_capacity(block_instrs.len());
        for block in self.plan.blocks() {
            let Some(instrs) = block_instrs.remove(&block.id) else {
                continue; // unreachable block: not part of the reachable graph
            };
            blocks.push(BasicBlockIr {
                id: block.id,
                params: block_params.remove(&block.id).unwrap_or_default(),
                instrs,
            });
        }

        let ir = FunctionIr {
            name: ir_name(self.class_file, self.method),
            descriptor: self.method.descriptor.clone(),
            params,
            return_type,
            blocks,
        };
        ir.verify()?;
        Ok(ir)
    }

    fn reject_entry_backedge(&self, entry: BasicBlockId) -> Result<()> {
        for block in self.plan.blocks() {
            if self.plan.successors(block.id).contains(&entry) {
                bail!(
                    "fvm-aot lowerer method {} branches back into its entry block; \
                     loop headers at method entry are unsupported (see docs/PUNCHLIST.md Phase 1)",
                    self.method_label
                );
            }
        }
        Ok(())
    }

    fn seed_entry_frame(&mut self, param_types: &[JvmType]) -> Result<Vec<IrParam>> {
        let mut params = Vec::with_capacity(param_types.len() + 1);
        let mut local: u16 = 0;
        // Instance methods (including constructors) receive `this` as local 0.
        if self.method.access_flags & 0x0008 == 0 {
            let this_ty = IrType::Object(self.class_file.this_name.clone());
            let value = self.state.push_param(local, this_ty.clone());
            params.push(IrParam { value, ty: this_ty });
            local += 1;
        }
        for ty in param_types {
            let ty = ir_type_for_jvm(ty, "parameter", &self.method_label)?;
            let value = self.state.push_param(local, ty.clone());
            params.push(IrParam { value, ty });
            local = local
                .checked_add(1)
                .context("fvm-aot lowerer local index exceeded u16")?;
        }
        Ok(params)
    }

    fn lower_opcode(&mut self, opcode: u8, opcode_pc: usize) -> Result<bool> {
        match opcode {
            0x01 => {
                let _ = self.state.push_constant(IrConst::Null);
            }
            0x02 => {
                let _ = self.state.push_constant(IrConst::Int(-1));
            }
            0x03..=0x08 => {
                let _ = self
                    .state
                    .push_constant(IrConst::Int(i32::from(opcode - 0x03)));
            }
            0x10 => {
                let value = i32::from(i8::from_be_bytes([read_u8(
                    &self.code.bytes,
                    &mut self.pc,
                )?]));
                let _ = self.state.push_constant(IrConst::Int(value));
            }
            0x11 => {
                let value = i32::from(read_i16(&self.code.bytes, &mut self.pc)?);
                let _ = self.state.push_constant(IrConst::Int(value));
            }
            0x12 | 0x13 => {
                let index = if opcode == 0x12 {
                    u16::from(read_u8(&self.code.bytes, &mut self.pc)?)
                } else {
                    read_u16(&self.code.bytes, &mut self.pc)?
                };
                push_ldc_constant(&mut self.call_lowering(), index)?;
            }
            0x15 | 0x19 | 0x1a..=0x1d | 0x2a..=0x2d => {
                let index = match opcode {
                    0x15 | 0x19 => u16::from(read_u8(&self.code.bytes, &mut self.pc)?),
                    0x1a..=0x1d => u16::from(opcode - 0x1a),
                    0x2a..=0x2d => u16::from(opcode - 0x2a),
                    _ => bail!("{}", unsupported_opcode_message(opcode)),
                };
                self.state.push_loaded_local(index)?;
            }
            0x36 | 0x3a | 0x3b..=0x3e | 0x4b..=0x4e => {
                let index = match opcode {
                    0x36 | 0x3a => u16::from(read_u8(&self.code.bytes, &mut self.pc)?),
                    0x3b..=0x3e => u16::from(opcode - 0x3b),
                    0x4b..=0x4e => u16::from(opcode - 0x4b),
                    _ => bail!("{}", unsupported_opcode_message(opcode)),
                };
                self.state.store_popped_local(index)?;
            }
            0x57 => {
                let _ = self.state.pop_stack()?;
            }
            0x58 => self.state.pop2()?,
            0x59 => self.state.dup()?,
            0x5a => self.state.dup_x1()?,
            0x5b => self.state.dup_x2()?,
            0x5c => self.state.dup2()?,
            0x5d => self.state.dup2_x1()?,
            0x5e => self.state.dup2_x2()?,
            0x5f => self.state.swap()?,
            0x60 => self.state.push_binary(IrArithmeticOp::Add)?,
            0x64 => self.state.push_binary(IrArithmeticOp::Sub)?,
            0x68 => self.state.push_binary(IrArithmeticOp::Mul)?,
            0x6c => self.state.push_checked_binary(IrArithmeticOp::Div)?,
            0x70 => self.state.push_checked_binary(IrArithmeticOp::Rem)?,
            0x74 => self.state.push_unary(IrUnaryOp::Neg)?,
            0x78 => self.state.push_binary(IrArithmeticOp::Shl)?,
            0x7a => self.state.push_binary(IrArithmeticOp::Shr)?,
            0x7c => self.state.push_binary(IrArithmeticOp::UShr)?,
            0x7e => self.state.push_binary(IrArithmeticOp::And)?,
            0x80 => self.state.push_binary(IrArithmeticOp::Or)?,
            0x82 => self.state.push_binary(IrArithmeticOp::Xor)?,
            0x91 => self.state.push_unary(IrUnaryOp::IntToByte)?,
            0x92 => self.state.push_unary(IrUnaryOp::IntToChar)?,
            0x93 => self.state.push_unary(IrUnaryOp::IntToShort)?,
            0x84 => {
                let index = u16::from(read_u8(&self.code.bytes, &mut self.pc)?);
                let delta = i32::from(i8::from_be_bytes([read_u8(
                    &self.code.bytes,
                    &mut self.pc,
                )?]));
                self.state.increment_local(index, delta)?;
            }
            0x2e => self.state.push_array_load(IrType::Int)?,
            0x32 => self.state.push_reference_array_load()?,
            0x4f => self.state.store_array_store(IrType::Int)?,
            0x53 => self.state.store_reference_array_store()?,
            0xbe => self.state.push_array_length()?,
            0xbd => {
                let component = self
                    .class_file
                    .class_name(read_u16(&self.code.bytes, &mut self.pc)?)?;
                if component.starts_with('[') {
                    bail!(
                        "fvm-aot lowerer does not support multidimensional arrays (anewarray of {component}) in {}; required feature: multidimensional arrays; planned milestone: primitive-completeness",
                        self.method_label
                    );
                }
                self.state.push_new_array(IrType::Object(component))?;
            }
            0xbc => {
                let atype = read_u8(&self.code.bytes, &mut self.pc)?;
                let element = match atype {
                    10 => IrType::Int,
                    other => bail!(
                        "fvm-aot lowerer only supports int arrays (newarray atype 10) today, got atype {other} in {}; required feature: primitive arrays; planned milestone: primitive-completeness",
                        self.method_label
                    ),
                };
                self.state.push_new_array(element)?;
            }
            0x99..=0xa6 | 0xc6 | 0xc7 => {
                return self.lower_conditional_branch(opcode, opcode_pc);
            }
            0xa7 => return self.lower_goto(opcode_pc),
            0xaa | 0xab => return self.lower_switch(opcode_pc),
            0xc4 => self.lower_wide()?,
            0xbb => lower_new(&mut self.call_lowering())?,
            0xb2 => lower_getstatic(&mut self.call_lowering())?,
            0xb4 => lower_getfield(&mut self.call_lowering())?,
            0xb5 => lower_putfield(&mut self.call_lowering())?,
            0xb6 => lower_invokevirtual(&mut self.call_lowering())?,
            0xb7 => lower_invokespecial(&mut self.call_lowering())?,
            0xb8 => lower_invokestatic(&mut self.call_lowering())?,
            0xba => lower_invokedynamic(&mut self.call_lowering())?,
            0xac | 0xb0 | 0xb1 => {
                let value = if opcode == 0xb1 {
                    None
                } else {
                    Some(self.state.pop_stack()?)
                };
                self.state.emit(IrInstr::Return(value));
                return Ok(true);
            }
            other => bail!("{}", unsupported_opcode_message(other)),
        }
        Ok(false)
    }

    fn lower_conditional_branch(&mut self, opcode: u8, opcode_pc: usize) -> Result<bool> {
        let offset = read_i16(&self.code.bytes, &mut self.pc)?;
        let target_bci = branch_target(opcode_pc, offset, self.code.bytes.len())?;
        let target = self.plan.block_id_for_bci(target_bci)?;
        let fallthrough = self.plan.block_id_for_bci(self.pc)?;
        let operands = branch_operands(opcode)
            .with_context(|| format!("branch opcode 0x{opcode:02x} had no operand model"))?;
        let condition = match operands {
            BranchOperands::IntZero(op) => {
                let lhs = self.state.pop_stack()?;
                let rhs = self.state.emit_constant(IrConst::Int(0));
                self.state.push_compare(op, lhs, Some(rhs))
            }
            BranchOperands::IntPair(op) | BranchOperands::RefPair(op) => {
                let rhs = self.state.pop_stack()?;
                let lhs = self.state.pop_stack()?;
                self.state.push_compare(op, lhs, Some(rhs))
            }
            BranchOperands::RefNull(op) => {
                let lhs = self.state.pop_stack()?;
                self.state.push_compare(op, lhs, None)
            }
        };
        let then_edge = self.edge_to(target)?;
        let else_edge = self.edge_to(fallthrough)?;
        self.state
            .emit(IrInstr::CondBranch(condition, then_edge, else_edge));
        Ok(true)
    }

    /// Lower a `wide`-prefixed instruction: the same local load/store/`iinc`
    /// handling as the narrow forms, but with a 16-bit local index (and a 16-bit
    /// `iinc` delta).
    fn lower_wide(&mut self) -> Result<()> {
        let widened = read_u8(&self.code.bytes, &mut self.pc)?;
        match widened {
            0x15 | 0x19 => {
                let index = read_u16(&self.code.bytes, &mut self.pc)?;
                self.state.push_loaded_local(index)?;
            }
            0x36 | 0x3a => {
                let index = read_u16(&self.code.bytes, &mut self.pc)?;
                self.state.store_popped_local(index)?;
            }
            0x84 => {
                let index = read_u16(&self.code.bytes, &mut self.pc)?;
                let delta = i32::from(read_i16(&self.code.bytes, &mut self.pc)?);
                self.state.increment_local(index, delta)?;
            }
            other => bail!("{}", unsupported_opcode_message(other)),
        }
        Ok(())
    }

    fn lower_switch(&mut self, opcode_pc: usize) -> Result<bool> {
        let table = parse_switch(&self.code.bytes, opcode_pc)?;
        self.pc = table.next_bci;
        // Pop the key first so the frame the targets capture no longer holds it.
        let key = self.state.pop_stack()?;
        let mut cases = Vec::with_capacity(table.cases.len());
        for (match_value, target_bci) in &table.cases {
            let target = self.plan.block_id_for_bci(*target_bci)?;
            let edge = self.edge_to(target)?;
            cases.push((*match_value, edge));
        }
        let default = self.plan.block_id_for_bci(table.default_bci)?;
        let default_edge = self.edge_to(default)?;
        self.state.emit(IrInstr::Switch(key, cases, default_edge));
        Ok(true)
    }

    fn lower_goto(&mut self, opcode_pc: usize) -> Result<bool> {
        let offset = read_i16(&self.code.bytes, &mut self.pc)?;
        let target_bci = branch_target(opcode_pc, offset, self.code.bytes.len())?;
        let target = self.plan.block_id_for_bci(target_bci)?;
        let edge = self.edge_to(target)?;
        self.state.emit(IrInstr::Branch(edge));
        Ok(true)
    }

    /// Build a control-flow edge into `target`, allocating the target's block
    /// parameters the first time an edge reaches it and passing the current
    /// frame's live values as arguments.
    fn edge_to(&mut self, target: BasicBlockId) -> Result<BranchEdge> {
        if !self.block_entries.contains_key(&target) {
            let entry = self.state.capture_block_entry()?;
            self.block_entries.insert(target, entry);
        }
        let entry = self
            .block_entries
            .get(&target)
            .expect("entry inserted above");
        let args = self.state.branch_args(entry)?;
        Ok(BranchEdge::new(target, args))
    }

    fn call_lowering(&mut self) -> CallLowering<'_, '_> {
        CallLowering {
            class_file: self.class_file,
            code: self.code,
            pc: &mut self.pc,
            method_label: &self.method_label,
            state: &mut self.state,
        }
    }
}
