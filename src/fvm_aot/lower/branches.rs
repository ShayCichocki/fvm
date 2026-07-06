use super::super::classfile::Code;
use super::super::ir::IrInstr;
use super::bytecode::{BlockPlan, BranchOperands, branch_operands, branch_target, read_i16};
use super::state::{FrameSnapshot, LowerState};
use anyhow::{Context, Result};
use std::collections::BTreeMap;

pub(super) struct BranchLowering<'a, 'b> {
    pub(super) code: &'a Code,
    pub(super) plan: &'a BlockPlan,
    pub(super) pc: &'b mut usize,
    pub(super) state: &'b mut LowerState,
    pub(super) entry_states: &'b mut BTreeMap<usize, FrameSnapshot>,
}

pub(super) fn lower_conditional_branch(
    input: &mut BranchLowering<'_, '_>,
    opcode: u8,
    opcode_pc: usize,
) -> Result<bool> {
    let offset = read_i16(&input.code.bytes, input.pc)?;
    let target_bci = branch_target(opcode_pc, offset, input.code.bytes.len())?;
    let target = input.plan.block_id_for_bci(target_bci)?;
    let fallthrough = input.plan.block_id_for_bci(*input.pc)?;
    let operands = branch_operands(opcode)
        .with_context(|| format!("branch opcode 0x{opcode:02x} had no operand model"))?;
    let condition = match operands {
        BranchOperands::IntZero(op) => {
            let lhs = input.state.pop_stack()?;
            let rhs = input.state.emit_constant(super::super::ir::IrConst::Int(0));
            input.state.push_compare(op, lhs, Some(rhs))
        }
        BranchOperands::IntPair(op) | BranchOperands::RefPair(op) => {
            let rhs = input.state.pop_stack()?;
            let lhs = input.state.pop_stack()?;
            input.state.push_compare(op, lhs, Some(rhs))
        }
        BranchOperands::RefNull(op) => {
            let lhs = input.state.pop_stack()?;
            input.state.push_compare(op, lhs, None)
        }
    };
    record_entry_state(input, target_bci);
    record_entry_state(input, *input.pc);
    input
        .state
        .emit(IrInstr::CondBranch(condition, target, fallthrough));
    Ok(true)
}

pub(super) fn lower_goto(input: &mut BranchLowering<'_, '_>, opcode_pc: usize) -> Result<bool> {
    let offset = read_i16(&input.code.bytes, input.pc)?;
    let target_bci = branch_target(opcode_pc, offset, input.code.bytes.len())?;
    let target = input.plan.block_id_for_bci(target_bci)?;
    record_entry_state(input, target_bci);
    input.state.emit(IrInstr::Branch(target));
    Ok(true)
}

fn record_entry_state(input: &mut BranchLowering<'_, '_>, bci: usize) {
    if input.entry_states.contains_key(&bci) {
        return;
    }
    input.entry_states.insert(bci, input.state.snapshot());
}
