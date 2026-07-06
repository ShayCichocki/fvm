use super::super::classfile::{ClassFile, Code, Method};
use super::super::diagnostics::unsupported_opcode_message;
use super::super::ir::{BasicBlockIr, FunctionIr, IrArithmeticOp, IrConst, IrInstr, IrParam};
use super::super::types::{JvmType, parse_method_descriptor};
use super::branches::{BranchLowering, lower_conditional_branch, lower_goto};
use super::bytecode::{BlockPlan, plan_blocks, read_i16, read_u8, read_u16};
use super::calls::{CallLowering, lower_invokestatic, push_int_constant};
use super::metadata::{ir_name, ir_type_for_jvm, method_label};
use super::state::{FrameSnapshot, LowerState};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

pub(in crate::fvm_aot) fn lower_method_to_ir(
    class_file: &ClassFile,
    method: &Method,
) -> Result<FunctionIr> {
    if method.access_flags & 0x0008 == 0 {
        bail!(
            "fvm-aot lowerer needs static method: {}",
            method_label(class_file, method)
        );
    }
    let Some(code) = method.code.as_ref() else {
        bail!(
            "fvm-aot lowerer requires Code for {}",
            method_label(class_file, method)
        );
    };
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
        entry_states: BTreeMap::new(),
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
    entry_states: BTreeMap<usize, FrameSnapshot>,
    state: LowerState,
}

impl<'a> MethodLowerer<'a> {
    fn lower(mut self, param_types: &[JvmType], return_type: &JvmType) -> Result<FunctionIr> {
        let params = self.lower_params(param_types)?;
        let return_type = ir_type_for_jvm(return_type, "return", &self.method_label)?;
        self.record_entry_state(0);
        let planned_blocks = self.plan.blocks().to_vec();
        let mut blocks = Vec::with_capacity(planned_blocks.len());
        for block in planned_blocks {
            self.pc = block.start;
            let snapshot = self
                .entry_states
                .get(&block.start)
                .cloned()
                .with_context(|| {
                    format!(
                        "fvm-aot lowerer block {} at bci {} has no entry state",
                        block.id, block.start
                    )
                })?;
            self.state.restore(&snapshot);
            let mut terminated = false;
            while self.pc < block.end && !terminated {
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
                if block.end >= self.code.bytes.len() {
                    bail!(
                        "fvm-aot lowerer method {} ended without return",
                        self.method_label
                    );
                }
                let fallthrough = self.plan.block_id_for_bci(block.end)?;
                self.record_entry_state(block.end);
                self.state.emit(IrInstr::Branch(fallthrough));
            }
            blocks.push(BasicBlockIr {
                id: block.id,
                instrs: self.state.finish_block(),
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

    fn lower_params(&mut self, param_types: &[JvmType]) -> Result<Vec<IrParam>> {
        let mut params = Vec::with_capacity(param_types.len());
        for (local, ty) in param_types.iter().enumerate() {
            let ty = ir_type_for_jvm(ty, "parameter", &self.method_label)?;
            let local = u16::try_from(local).context("fvm-aot lowerer local index exceeded u16")?;
            let value = self.state.push_param(local, ty.clone());
            params.push(IrParam { value, ty });
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
                push_int_constant(&mut self.call_lowering(), index)?;
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
            0x60 => self.state.push_binary(IrArithmeticOp::Add)?,
            0x64 => self.state.push_binary(IrArithmeticOp::Sub)?,
            0x68 => self.state.push_binary(IrArithmeticOp::Mul)?,
            0x6c => self.state.push_checked_binary(IrArithmeticOp::Div)?,
            0x70 => self.state.push_checked_binary(IrArithmeticOp::Rem)?,
            0x74 => self.state.push_unary_neg()?,
            0x84 => {
                let index = u16::from(read_u8(&self.code.bytes, &mut self.pc)?);
                let delta = i32::from(i8::from_be_bytes([read_u8(
                    &self.code.bytes,
                    &mut self.pc,
                )?]));
                self.state.increment_local(index, delta)?;
            }
            0x99..=0xa6 | 0xc6 | 0xc7 => {
                return lower_conditional_branch(&mut self.branch_lowering(), opcode, opcode_pc);
            }
            0xa7 => return lower_goto(&mut self.branch_lowering(), opcode_pc),
            0xb8 => lower_invokestatic(&mut self.call_lowering())?,
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

    fn record_entry_state(&mut self, bci: usize) {
        if self.entry_states.contains_key(&bci) {
            return;
        }
        self.entry_states.insert(bci, self.state.snapshot());
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

    fn branch_lowering(&mut self) -> BranchLowering<'_, '_> {
        BranchLowering {
            code: self.code,
            plan: &self.plan,
            pc: &mut self.pc,
            state: &mut self.state,
            entry_states: &mut self.entry_states,
        }
    }
}
