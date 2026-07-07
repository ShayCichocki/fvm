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
    successors: BTreeMap<BasicBlockId, Vec<BasicBlockId>>,
}

impl BlockPlan {
    pub(super) fn blocks(&self) -> &[BlockRange] {
        &self.blocks
    }

    pub(super) fn entry(&self) -> BasicBlockId {
        self.blocks[0].id
    }

    pub(super) fn range(&self, id: BasicBlockId) -> Result<BlockRange> {
        self.blocks
            .iter()
            .copied()
            .find(|block| block.id == id)
            .with_context(|| format!("no basic block with id {id}"))
    }

    pub(super) fn successors(&self, id: BasicBlockId) -> &[BasicBlockId] {
        self.successors
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Blocks reachable from entry in reverse-postorder. A forward predecessor
    /// always precedes its successor, so block parameters created when the
    /// first (forward) edge is lowered are available before the block body and
    /// its back-edges are visited.
    pub(super) fn reverse_postorder(&self) -> Vec<BasicBlockId> {
        let mut postorder = Vec::new();
        let mut visited = std::collections::BTreeSet::new();
        let mut stack = vec![(self.entry(), 0_usize)];
        visited.insert(self.entry());
        while let Some((block, next_child)) = stack.pop() {
            let successors = self.successors(block);
            if next_child < successors.len() {
                stack.push((block, next_child + 1));
                let child = successors[next_child];
                if visited.insert(child) {
                    stack.push((child, 0));
                }
            } else {
                postorder.push(block);
            }
        }
        postorder.reverse();
        postorder
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
        match &instruction.flow {
            ControlFlow::Normal | ControlFlow::Return => {}
            ControlFlow::Goto { target } => {
                leaders.insert(*target);
                branch_targets.push(BranchTargetSite {
                    bci: instruction.bci,
                    opcode: instruction.opcode,
                    target: *target,
                });
            }
            ControlFlow::Conditional {
                target,
                fallthrough,
            } => {
                if *fallthrough >= code.bytes.len() {
                    bail!(
                        "fvm-aot lowerer bytecode error in {method_label} at bci {} (opcode 0x{:02x}): conditional branch fallthrough {fallthrough} out of range",
                        instruction.bci,
                        instruction.opcode
                    );
                }
                leaders.insert(*target);
                leaders.insert(*fallthrough);
                branch_targets.push(BranchTargetSite {
                    bci: instruction.bci,
                    opcode: instruction.opcode,
                    target: *target,
                });
            }
            ControlFlow::Switch { targets } => {
                for &target in targets {
                    leaders.insert(target);
                    branch_targets.push(BranchTargetSite {
                        bci: instruction.bci,
                        opcode: instruction.opcode,
                        target,
                    });
                }
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

    let mut successors = BTreeMap::new();
    for block in &blocks {
        successors.insert(block.id, block_successors(code, *block, &block_by_start)?);
    }

    Ok(BlockPlan {
        blocks,
        block_by_start,
        successors,
    })
}

/// Determine a block's control-flow successors from its terminating
/// instruction (or fallthrough into the next leader).
fn block_successors(
    code: &Code,
    block: BlockRange,
    block_by_start: &BTreeMap<usize, BasicBlockId>,
) -> Result<Vec<BasicBlockId>> {
    let mut pc = block.start;
    let mut last = decode_instruction(&code.bytes, pc)?;
    pc = last.next_bci;
    while pc < block.end {
        last = decode_instruction(&code.bytes, pc)?;
        pc = last.next_bci;
    }
    let id_for = |bci: usize| -> Result<BasicBlockId> {
        block_by_start
            .get(&bci)
            .copied()
            .with_context(|| format!("no basic block starts at bci {bci}"))
    };
    match &last.flow {
        ControlFlow::Return => Ok(Vec::new()),
        ControlFlow::Goto { target } => Ok(vec![id_for(*target)?]),
        ControlFlow::Conditional {
            target,
            fallthrough,
        } => Ok(vec![id_for(*target)?, id_for(*fallthrough)?]),
        ControlFlow::Switch { targets } => {
            let mut successors = Vec::new();
            for &target in targets {
                let id = id_for(target)?;
                if !successors.contains(&id) {
                    successors.push(id);
                }
            }
            Ok(successors)
        }
        ControlFlow::Normal => Ok(vec![id_for(block.end)?]),
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecodedInstruction {
    bci: usize,
    opcode: u8,
    next_bci: usize,
    flow: ControlFlow,
}

/// The decoded body of a `tableswitch`/`lookupswitch`, shared by block planning
/// and lowering so both agree on targets and the post-switch bci.
pub(super) struct SwitchTable {
    pub(super) default_bci: usize,
    /// `(match value, target bci)` pairs. For `tableswitch` the match values are
    /// the contiguous `low..=high` range; for `lookupswitch` they are explicit.
    pub(super) cases: Vec<(i32, usize)>,
    pub(super) next_bci: usize,
}

/// Parse a switch instruction at `opcode_pc` (its 4-byte-aligned operands come
/// after 0–3 padding bytes measured from the start of the code array).
pub(super) fn parse_switch(code: &[u8], opcode_pc: usize) -> Result<SwitchTable> {
    let mut pc = opcode_pc;
    let opcode = read_u8(code, &mut pc)?;
    while !pc.is_multiple_of(4) {
        let _ = read_u8(code, &mut pc)?;
    }
    let default_offset = read_i32(code, &mut pc)?;
    let default_bci = switch_target(opcode_pc, default_offset, code.len())?;
    let mut cases = Vec::new();
    match opcode {
        0xaa => {
            let low = read_i32(code, &mut pc)?;
            let high = read_i32(code, &mut pc)?;
            if high < low {
                bail!("fvm-aot lowerer tableswitch at bci {opcode_pc} has high {high} < low {low}");
            }
            let count = i64::from(high) - i64::from(low) + 1;
            if count > code.len() as i64 {
                bail!(
                    "fvm-aot lowerer tableswitch at bci {opcode_pc} declares {count} entries, larger than the code",
                );
            }
            for index in 0..count {
                let match_value = (i64::from(low) + index) as i32;
                let offset = read_i32(code, &mut pc)?;
                cases.push((match_value, switch_target(opcode_pc, offset, code.len())?));
            }
        }
        0xab => {
            let pairs = read_i32(code, &mut pc)?;
            if pairs < 0 || i64::from(pairs) > code.len() as i64 {
                bail!(
                    "fvm-aot lowerer lookupswitch at bci {opcode_pc} declares an implausible pair count {pairs}",
                );
            }
            for _ in 0..pairs {
                let match_value = read_i32(code, &mut pc)?;
                let offset = read_i32(code, &mut pc)?;
                cases.push((match_value, switch_target(opcode_pc, offset, code.len())?));
            }
        }
        other => bail!("fvm-aot lowerer parse_switch on non-switch opcode 0x{other:02x}"),
    }
    Ok(SwitchTable {
        default_bci,
        cases,
        next_bci: pc,
    })
}

fn switch_target(opcode_pc: usize, offset: i32, code_len: usize) -> Result<usize> {
    let base = i64::try_from(opcode_pc).context("fvm-aot lowerer bci exceeded i64")?;
    let target = base + i64::from(offset);
    if target < 0 || target >= code_len as i64 {
        bail!("fvm-aot lowerer switch target {target} out of range 0..{code_len}");
    }
    usize::try_from(target).context("fvm-aot lowerer switch target exceeded usize")
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ControlFlow {
    Normal,
    Conditional {
        target: usize,
        fallthrough: usize,
    },
    Goto {
        target: usize,
    },
    /// A multi-way branch (`tableswitch`/`lookupswitch`): every reachable target
    /// bci, default included. There is no fallthrough — control always transfers
    /// to one of these.
    Switch {
        targets: Vec<usize>,
    },
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
        | 0x2a..=0x2e
        | 0x3b..=0x3e
        | 0x4b..=0x4f
        | 0x57
        | 0x58..=0x5f
        | 0x60
        | 0x64
        | 0x68
        | 0x6c
        | 0x70
        | 0x74
        | 0x78
        | 0x7a
        | 0x7c
        | 0x7e
        | 0x80
        | 0x82
        | 0x91..=0x93 => ControlFlow::Normal,
        0x10 | 0x12 | 0x15 | 0x19 | 0x36 | 0x3a | 0xbc => {
            let _ = read_u8(code, &mut pc)?;
            ControlFlow::Normal
        }
        0xbe => ControlFlow::Normal,
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
        0xaa | 0xab => {
            let table = parse_switch(code, bci)?;
            pc = table.next_bci;
            let mut targets = Vec::with_capacity(table.cases.len() + 1);
            targets.push(table.default_bci);
            targets.extend(table.cases.iter().map(|(_, target)| *target));
            ControlFlow::Switch { targets }
        }
        0xb4 | 0xb5 | 0xb7 | 0xb8 | 0xbb => {
            // getfield / putfield / invokespecial / invokestatic / new — each
            // takes a two-byte constant-pool index.
            let _ = read_u16(code, &mut pc)?;
            ControlFlow::Normal
        }
        0xc4 => {
            // wide: a 0xc4 prefix widens the following instruction's local index
            // (u2). `iinc` additionally carries a widened s2 constant.
            let widened = read_u8(code, &mut pc)?;
            let _index = read_u16(code, &mut pc)?;
            if widened == 0x84 {
                let _ = read_u16(code, &mut pc)?;
            }
            ControlFlow::Normal
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

fn read_i32(code: &[u8], pc: &mut usize) -> Result<i32> {
    let high = read_u16(code, pc)?;
    let low = read_u16(code, pc)?;
    Ok(i32::from_be_bytes([
        high.to_be_bytes()[0],
        high.to_be_bytes()[1],
        low.to_be_bytes()[0],
        low.to_be_bytes()[1],
    ]))
}
