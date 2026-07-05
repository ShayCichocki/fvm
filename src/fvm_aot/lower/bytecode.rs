use super::super::classfile::Code;
use super::super::diagnostics::unsupported_opcode_message;
use super::super::ir::{BasicBlockId, IrCompareOp};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BlockRange {
    pub(super) id: BasicBlockId,
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Debug)]
pub(super) struct BlockPlan {
    blocks: Vec<BlockRange>,
    block_by_start: BTreeMap<usize, BasicBlockId>,
}

impl BlockPlan {
    pub(super) fn blocks(&self) -> &[BlockRange] {
        &self.blocks
    }

    pub(super) fn block_id_for_bci(&self, bci: usize) -> Result<BasicBlockId> {
        self.block_by_start
            .get(&bci)
            .copied()
            .with_context(|| format!("no basic block starts at bci {bci}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BranchOperands {
    IntZero(IrCompareOp),
    IntPair(IrCompareOp),
    RefPair(IrCompareOp),
    RefNull(IrCompareOp),
}

pub(super) fn branch_operands(opcode: u8) -> Option<BranchOperands> {
    match opcode {
        0x99 => Some(BranchOperands::IntZero(IrCompareOp::IntEq)),
        0x9a => Some(BranchOperands::IntZero(IrCompareOp::IntNe)),
        0x9b => Some(BranchOperands::IntZero(IrCompareOp::IntLt)),
        0x9c => Some(BranchOperands::IntZero(IrCompareOp::IntGe)),
        0x9d => Some(BranchOperands::IntZero(IrCompareOp::IntGt)),
        0x9e => Some(BranchOperands::IntZero(IrCompareOp::IntLe)),
        0x9f => Some(BranchOperands::IntPair(IrCompareOp::IntEq)),
        0xa0 => Some(BranchOperands::IntPair(IrCompareOp::IntNe)),
        0xa1 => Some(BranchOperands::IntPair(IrCompareOp::IntLt)),
        0xa2 => Some(BranchOperands::IntPair(IrCompareOp::IntGe)),
        0xa3 => Some(BranchOperands::IntPair(IrCompareOp::IntGt)),
        0xa4 => Some(BranchOperands::IntPair(IrCompareOp::IntLe)),
        0xa5 => Some(BranchOperands::RefPair(IrCompareOp::RefEqPlaceholder)),
        0xa6 => Some(BranchOperands::RefPair(IrCompareOp::RefNePlaceholder)),
        0xc6 => Some(BranchOperands::RefNull(IrCompareOp::RefIsNullPlaceholder)),
        0xc7 => Some(BranchOperands::RefNull(
            IrCompareOp::RefIsNonNullPlaceholder,
        )),
        _ => None,
    }
}

pub(super) fn plan_blocks(code: &Code, method_label: &str) -> Result<BlockPlan> {
    if code.bytes.is_empty() {
        bail!("fvm-aot lowerer method {method_label} has empty bytecode");
    }

    let mut leaders = BTreeSet::from([0_usize]);
    let mut instruction_starts = BTreeSet::new();
    let mut branch_targets = Vec::new();
    let mut pc = 0_usize;

    while pc < code.bytes.len() {
        let instruction = decode_instruction(&code.bytes, pc).with_context(|| {
            format!(
                "fvm-aot lowerer bytecode error in {method_label} at bci {pc} (opcode 0x{:02x})",
                code.bytes[pc]
            )
        })?;
        instruction_starts.insert(instruction.bci);
        match instruction.flow {
            ControlFlow::Normal | ControlFlow::Return => {}
            ControlFlow::Goto { target } => {
                leaders.insert(target);
                branch_targets.push(BranchTargetSite {
                    bci: instruction.bci,
                    opcode: instruction.opcode,
                    target,
                });
            }
            ControlFlow::Conditional {
                target,
                fallthrough,
            } => {
                if fallthrough >= code.bytes.len() {
                    bail!(
                        "fvm-aot lowerer bytecode error in {method_label} at bci {} (opcode 0x{:02x}): conditional branch fallthrough {fallthrough} out of range",
                        instruction.bci,
                        instruction.opcode
                    );
                }
                leaders.insert(target);
                leaders.insert(fallthrough);
                branch_targets.push(BranchTargetSite {
                    bci: instruction.bci,
                    opcode: instruction.opcode,
                    target,
                });
            }
        }
        pc = instruction.next_bci;
    }

    for site in branch_targets {
        if instruction_starts.contains(&site.target) {
            continue;
        }
        bail!(
            "fvm-aot lowerer bytecode error in {method_label} at bci {} (opcode 0x{:02x}): branch target {} is not an instruction boundary",
            site.bci,
            site.opcode,
            site.target
        );
    }

    for leader in &leaders {
        if instruction_starts.contains(leader) {
            continue;
        }
        bail!(
            "fvm-aot lowerer bytecode error in {method_label}: basic block leader bci {leader} is not an instruction boundary"
        );
    }

    let starts = leaders.into_iter().collect::<Vec<_>>();
    let mut block_by_start = BTreeMap::new();
    let mut blocks = Vec::with_capacity(starts.len());
    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(code.bytes.len());
        let raw_id = u32::try_from(index).context("fvm-aot lowerer basic block id exceeded u32")?;
        let id = BasicBlockId::new(raw_id);
        block_by_start.insert(start, id);
        blocks.push(BlockRange { id, start, end });
    }

    Ok(BlockPlan {
        blocks,
        block_by_start,
    })
}

pub(super) fn branch_target(opcode_pc: usize, offset: i16, code_len: usize) -> Result<usize> {
    let base = i64::try_from(opcode_pc).context("fvm-aot lowerer bci exceeded i64")?;
    let target = base + i64::from(offset);
    let len = i64::try_from(code_len).context("fvm-aot lowerer code length exceeded i64")?;
    if target < 0 || target >= len {
        bail!("fvm-aot lowerer branch target {target} out of range 0..{code_len}");
    }
    usize::try_from(target).context("fvm-aot lowerer branch target exceeded usize")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DecodedInstruction {
    bci: usize,
    opcode: u8,
    next_bci: usize,
    flow: ControlFlow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlFlow {
    Normal,
    Conditional { target: usize, fallthrough: usize },
    Goto { target: usize },
    Return,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BranchTargetSite {
    bci: usize,
    opcode: u8,
    target: usize,
}

fn decode_instruction(code: &[u8], bci: usize) -> Result<DecodedInstruction> {
    let mut pc = bci;
    let opcode = read_u8(code, &mut pc)?;
    let flow = match opcode {
        0x01..=0x08
        | 0x1a..=0x1d
        | 0x2a..=0x2d
        | 0x3b..=0x3e
        | 0x4b..=0x4e
        | 0x60
        | 0x64
        | 0x68
        | 0x6c
        | 0x70
        | 0x74 => ControlFlow::Normal,
        0x10 | 0x12 | 0x15 | 0x19 | 0x36 | 0x3a => {
            let _ = read_u8(code, &mut pc)?;
            ControlFlow::Normal
        }
        0x11 | 0x13 => {
            let _ = read_u16(code, &mut pc)?;
            ControlFlow::Normal
        }
        0x84 => {
            let _ = read_u8(code, &mut pc)?;
            let _ = read_u8(code, &mut pc)?;
            ControlFlow::Normal
        }
        0x99..=0xa6 | 0xc6 | 0xc7 => {
            let offset = read_i16(code, &mut pc)?;
            ControlFlow::Conditional {
                target: branch_target(bci, offset, code.len())?,
                fallthrough: pc,
            }
        }
        0xa7 => {
            let offset = read_i16(code, &mut pc)?;
            ControlFlow::Goto {
                target: branch_target(bci, offset, code.len())?,
            }
        }
        0xac | 0xb0 | 0xb1 => ControlFlow::Return,
        other => bail!("{}", unsupported_opcode_message(other)),
    };
    Ok(DecodedInstruction {
        bci,
        opcode,
        next_bci: pc,
        flow,
    })
}

pub(super) fn read_u8(code: &[u8], pc: &mut usize) -> Result<u8> {
    if *pc >= code.len() {
        bail!("truncated bytecode at pc {pc}");
    }
    let value = code[*pc];
    *pc += 1;
    Ok(value)
}

pub(super) fn read_u16(code: &[u8], pc: &mut usize) -> Result<u16> {
    let high = read_u8(code, pc)?;
    let low = read_u8(code, pc)?;
    Ok(u16::from_be_bytes([high, low]))
}

pub(super) fn read_i16(code: &[u8], pc: &mut usize) -> Result<i16> {
    Ok(i16::from_be_bytes(read_u16(code, pc)?.to_be_bytes()))
}
