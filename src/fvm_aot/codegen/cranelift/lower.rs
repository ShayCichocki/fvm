mod categories;

use super::declare_exported_function;
use super::error::CodegenError;
use super::symbols::{function_label, method_label};
use crate::fvm_aot::ir::{
    BasicBlockId, BranchEdge, FieldRef, FunctionIr, IrArithmeticOp, IrCompareOp, IrConst, IrInstr,
    IrType, IrUnaryOp, RuntimeHelper, TrapReason, ValueId,
};
use crate::fvm_aot::object_model::{
    ARRAY_ELEMENTS_OFFSET, ARRAY_LENGTH_OFFSET, CLASS_ID_OFFSET, ObjectModel, REFERENCE_BYTES,
};
use crate::fvm_aot::runtime_stub::{
    ALLOC_SYMBOL, PRINT_INT_RAW_SYMBOL, PRINT_INT_SYMBOL, PRINT_STRING_SYMBOL,
    PRINTLN_EMPTY_SYMBOL, PRINTLN_STRING_SYMBOL, SB_APPEND_INT_SYMBOL, SB_APPEND_STRING_SYMBOL,
    SB_FINISH_SYMBOL, SB_NEW_SYMBOL, TRAP_BOUNDS_SYMBOL, TRAP_DIVIDE_BY_ZERO_SYMBOL,
    TRAP_NEGATIVE_ARRAY_SIZE_SYMBOL, TRAP_NULL_SYMBOL,
};
use categories::instruction_category;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, Block, BlockArg, MemFlagsData, Signature, TrapCode, Type, Value, types,
};
use cranelift_codegen::ir::{InstBuilder, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap};

/// Interns compiled string literals as read-only data objects so identical
/// literals share one blob (a small step toward literal interning, P3.1).
type StringPool = HashMap<Vec<u8>, DataId>;

pub(super) fn emit_functions(
    module: &mut ObjectModule,
    functions: &[&FunctionIr],
    model: &ObjectModel,
) -> Result<(), CodegenError> {
    let ids = declare_functions(module, functions)?;
    let mut strings = StringPool::new();
    for function in functions {
        emit_function(module, function, &ids, model, &mut strings)?;
    }
    Ok(())
}

pub(super) fn signature(
    module: &mut ObjectModule,
    function: &FunctionIr,
) -> Result<Signature, CodegenError> {
    let mut signature = module.make_signature();
    for param in &function.params {
        signature
            .params
            .push(AbiParam::new(clif_type(function, &param.ty)?));
    }
    match &function.return_type {
        IrType::Void => {}
        ty => signature
            .returns
            .push(AbiParam::new(clif_type(function, ty)?)),
    }
    Ok(signature)
}

fn declare_functions(
    module: &mut ObjectModule,
    functions: &[&FunctionIr],
) -> Result<BTreeMap<String, FuncId>, CodegenError> {
    let mut ids = BTreeMap::new();
    for function in functions {
        ids.insert(
            function_label(function),
            declare_exported_function(module, function)?,
        );
    }
    Ok(ids)
}

fn emit_function(
    module: &mut ObjectModule,
    function: &FunctionIr,
    ids: &BTreeMap<String, FuncId>,
    model: &ObjectModel,
    strings: &mut StringPool,
) -> Result<(), CodegenError> {
    let func_id = *ids
        .get(&function_label(function))
        .ok_or_else(|| backend_error(function, "function was not declared"))?;
    let mut context = module.make_context();
    context.func.signature = signature(module, function)?;
    context.func.name = UserFuncName::user(0, func_id.as_u32());

    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);

    // Materialize one Cranelift block per IR block. The IR entry block (always
    // first) carries the function's parameters; every other block carries the
    // block parameters that thread values across edges (the phi equivalent).
    // Creating all blocks and their params up front means every branch target
    // and every cross-block value already exists before any body is lowered.
    let (entry_ir, rest) = match function.blocks.split_first() {
        Some(split) => split,
        None => return Err(backend_error(function, "function has no basic blocks")),
    };

    let mut clif_blocks: HashMap<BasicBlockId, Block> = HashMap::new();
    let mut values: HashMap<ValueId, Value> = HashMap::new();

    let entry = builder.create_block();
    for param in &function.params {
        let clif_value = builder.append_block_param(entry, clif_type(function, &param.ty)?);
        values.insert(param.value, clif_value);
    }
    clif_blocks.insert(entry_ir.id, entry);

    for block in rest {
        let clif_block = builder.create_block();
        for param in &block.params {
            let clif_value =
                builder.append_block_param(clif_block, clif_type(function, &param.ty)?);
            values.insert(param.value, clif_value);
        }
        clif_blocks.insert(block.id, clif_block);
    }

    for block in &function.blocks {
        let clif_block = require_block(function, &clif_blocks, block.id)?;
        builder.switch_to_block(clif_block);
        for instr in &block.instrs {
            lower_instr(
                module,
                &mut builder,
                function,
                ids,
                &clif_blocks,
                &mut values,
                model,
                strings,
                instr,
            )?;
        }
    }

    builder.seal_all_blocks();
    builder.finalize();

    module
        .define_function(func_id, &mut context)
        .map_err(|source| CodegenError::Backend {
            function: function_label(function),
            message: source.to_string(),
        })
}

#[allow(clippy::too_many_arguments)]
fn lower_instr(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    ids: &BTreeMap<String, FuncId>,
    clif_blocks: &HashMap<BasicBlockId, Block>,
    values: &mut HashMap<ValueId, Value>,
    model: &ObjectModel,
    strings: &mut StringPool,
    instr: &IrInstr,
) -> Result<(), CodegenError> {
    match instr {
        IrInstr::Param(..) => Ok(()),
        IrInstr::Constant(value, IrConst::Null) => {
            // The null reference is a zero pointer.
            values.insert(*value, builder.ins().iconst(types::I64, 0));
            Ok(())
        }
        IrInstr::Constant(value, IrConst::String(bytes)) => {
            let pointer = lower_string_literal(module, builder, function, strings, bytes)?;
            values.insert(*value, pointer);
            Ok(())
        }
        IrInstr::Constant(value, constant) => {
            let constant = clif_const(function, constant)?;
            values.insert(*value, builder.ins().iconst(types::I32, constant));
            Ok(())
        }
        IrInstr::RuntimeCall(result, helper, args) => {
            lower_runtime_call(module, builder, function, values, *result, helper, args)
        }

        IrInstr::Compare(value, op, lhs, rhs) => {
            let result = lower_compare(function, builder, values, *op, *lhs, *rhs)?;
            values.insert(*value, result);
            Ok(())
        }
        IrInstr::Branch(edge) => {
            let block = require_block(function, clif_blocks, edge.block)?;
            let args = edge_args(function, values, edge)?;
            builder.ins().jump(block, &args);
            Ok(())
        }
        IrInstr::CondBranch(condition, then_edge, else_edge) => {
            let condition = require_value(function, values, *condition)?;
            let then_block = require_block(function, clif_blocks, then_edge.block)?;
            let then_args = edge_args(function, values, then_edge)?;
            let else_block = require_block(function, clif_blocks, else_edge.block)?;
            let else_args = edge_args(function, values, else_edge)?;
            builder
                .ins()
                .brif(condition, then_block, &then_args, else_block, &else_args);
            Ok(())
        }
        IrInstr::Switch(key, cases, default) => {
            lower_switch(builder, function, clif_blocks, values, *key, cases, default)
        }
        IrInstr::Arithmetic(value, op, lhs, rhs) => {
            let lhs = require_value(function, values, *lhs)?;
            let rhs = require_value(function, values, *rhs)?;
            let result = match op {
                IrArithmeticOp::Add => builder.ins().iadd(lhs, rhs),
                IrArithmeticOp::Sub => builder.ins().isub(lhs, rhs),
                IrArithmeticOp::Mul => builder.ins().imul(lhs, rhs),
                IrArithmeticOp::Div => checked_sdiv(builder, lhs, rhs),
                IrArithmeticOp::Rem => checked_srem(builder, lhs, rhs),
                IrArithmeticOp::Shl => {
                    let amount = shift_amount(builder, rhs);
                    builder.ins().ishl(lhs, amount)
                }
                IrArithmeticOp::Shr => {
                    let amount = shift_amount(builder, rhs);
                    builder.ins().sshr(lhs, amount)
                }
                IrArithmeticOp::UShr => {
                    let amount = shift_amount(builder, rhs);
                    builder.ins().ushr(lhs, amount)
                }
                IrArithmeticOp::And => builder.ins().band(lhs, rhs),
                IrArithmeticOp::Or => builder.ins().bor(lhs, rhs),
                IrArithmeticOp::Xor => builder.ins().bxor(lhs, rhs),
            };
            values.insert(*value, result);
            Ok(())
        }
        IrInstr::Unary(value, op, input) => {
            let input = require_value(function, values, *input)?;
            let result = match op {
                IrUnaryOp::Neg => builder.ins().ineg(input),
                IrUnaryOp::IntToByte => sign_extend_from(builder, input, types::I8),
                IrUnaryOp::IntToShort => sign_extend_from(builder, input, types::I16),
                IrUnaryOp::IntToChar => zero_extend_from(builder, input, types::I16),
            };
            values.insert(*value, result);
            Ok(())
        }
        IrInstr::ZeroCheck(value, reason) => {
            lower_zero_check(module, builder, function, values, *value, reason)
        }
        IrInstr::Call(value, method, args) => {
            let key = method_label(method);
            let func_id = *ids.get(&key).ok_or_else(|| CodegenError::Unsupported {
                function: function_label(function),
                category: "Call",
                detail: format!("T23 supports direct app-owned static calls only; missing {key}"),
            })?;
            let local = module.declare_func_in_func(func_id, builder.func);
            let args = args
                .iter()
                .map(|arg| require_value(function, values, *arg))
                .collect::<Result<Vec<_>, _>>()?;
            let call = builder.ins().call(local, &args);
            if let Some(value) = value {
                let result = builder
                    .inst_results(call)
                    .first()
                    .copied()
                    .ok_or_else(|| backend_error(function, "call returned no value"))?;
                values.insert(*value, result);
            }
            Ok(())
        }
        IrInstr::Return(Some(value)) => {
            let value = require_value(function, values, *value)?;
            builder.ins().return_(&[value]);
            Ok(())
        }
        IrInstr::Return(None) => {
            builder.ins().return_(&[]);
            Ok(())
        }
        IrInstr::NewObject(value, class) => {
            let object = lower_new_object(module, builder, function, model, class)?;
            values.insert(*value, object);
            Ok(())
        }
        IrInstr::FieldGet(value, field, Some(receiver)) => {
            let receiver = require_value(function, values, *receiver)?;
            let (offset, ty) = resolve_field(function, model, field)?;
            let loaded = builder.ins().load(
                ty,
                MemFlagsData::trusted(),
                receiver,
                offset_of(function, offset)?,
            );
            values.insert(*value, loaded);
            Ok(())
        }
        IrInstr::FieldPut(field, Some(receiver), value) => {
            let receiver = require_value(function, values, *receiver)?;
            let stored = require_value(function, values, *value)?;
            let (offset, _ty) = resolve_field(function, model, field)?;
            builder.ins().store(
                MemFlagsData::trusted(),
                stored,
                receiver,
                offset_of(function, offset)?,
            );
            Ok(())
        }
        IrInstr::NewArray(value, element, length) => {
            let length = require_value(function, values, *length)?;
            let array = lower_new_array(module, builder, function, element, length)?;
            values.insert(*value, array);
            Ok(())
        }
        IrInstr::ArrayLength(value, array) => {
            let array = require_value(function, values, *array)?;
            let length = builder.ins().load(
                types::I32,
                MemFlagsData::trusted(),
                array,
                offset_of(function, ARRAY_LENGTH_OFFSET)?,
            );
            values.insert(*value, length);
            Ok(())
        }
        IrInstr::ArrayLoad(value, array, index, element) => {
            let array = require_value(function, values, *array)?;
            let index = require_value(function, values, *index)?;
            let ty = clif_type(function, element)?;
            let addr = array_element_address(function, builder, array, index, element)?;
            let loaded = builder.ins().load(ty, MemFlagsData::trusted(), addr, 0);
            values.insert(*value, loaded);
            Ok(())
        }
        IrInstr::ArrayStore(array, index, value, element) => {
            let array = require_value(function, values, *array)?;
            let index = require_value(function, values, *index)?;
            let stored = require_value(function, values, *value)?;
            let addr = array_element_address(function, builder, array, index, element)?;
            builder
                .ins()
                .store(MemFlagsData::trusted(), stored, addr, 0);
            Ok(())
        }
        IrInstr::NullCheck(value, _reason) => {
            let reference = require_value(function, values, *value)?;
            let null = builder.ins().iconst(types::I64, 0);
            let is_null = builder.ins().icmp(IntCC::Equal, reference, null);
            trap_if(module, builder, function, is_null, TRAP_NULL_SYMBOL, &[])
        }
        IrInstr::BoundsCheck(index, length, _reason) => {
            let index = require_value(function, values, *index)?;
            let length = require_value(function, values, *length)?;
            // Unsigned compare folds `index < 0` and `index >= length` into one
            // test: a negative index is a huge unsigned value ≥ length.
            let out_of_bounds =
                builder
                    .ins()
                    .icmp(IntCC::UnsignedGreaterThanOrEqual, index, length);
            trap_if(
                module,
                builder,
                function,
                out_of_bounds,
                TRAP_BOUNDS_SYMBOL,
                &[index, length],
            )
        }
        IrInstr::FieldGet(_, _, None) | IrInstr::FieldPut(_, None, _) => unsupported(
            function,
            instruction_category(instr),
            "static fields (getstatic/putstatic) are not compiled yet",
        ),
        other => unsupported(
            function,
            instruction_category(other),
            "the compiler path supports int constants, arithmetic, compares, branches, returns, and direct static calls",
        ),
    }
}

/// Lower an integer `Compare` to an `icmp` and normalize the boolean result to
/// an `i32` (0/1), matching Java's representation of booleans. Reference
/// comparisons are not reachable here: reference-typed operands are rejected by
/// [`clif_type`] before any block is built.
fn lower_compare(
    function: &FunctionIr,
    builder: &mut FunctionBuilder<'_>,
    values: &HashMap<ValueId, Value>,
    op: IrCompareOp,
    lhs: ValueId,
    rhs: Option<ValueId>,
) -> Result<Value, CodegenError> {
    let condition = match op {
        IrCompareOp::IntEq => IntCC::Equal,
        IrCompareOp::IntNe => IntCC::NotEqual,
        IrCompareOp::IntLt => IntCC::SignedLessThan,
        IrCompareOp::IntGe => IntCC::SignedGreaterThanOrEqual,
        IrCompareOp::IntGt => IntCC::SignedGreaterThan,
        IrCompareOp::IntLe => IntCC::SignedLessThanOrEqual,
        IrCompareOp::RefEqPlaceholder
        | IrCompareOp::RefNePlaceholder
        | IrCompareOp::RefIsNullPlaceholder
        | IrCompareOp::RefIsNonNullPlaceholder => {
            return unsupported(
                function,
                "Compare",
                "reference comparisons await the runtime object model",
            );
        }
    };
    let lhs = require_value(function, values, lhs)?;
    let rhs =
        rhs.ok_or_else(|| backend_error(function, "integer compare is missing an operand"))?;
    let rhs = require_value(function, values, rhs)?;
    let compared = builder.ins().icmp(condition, lhs, rhs);
    if builder.func.dfg.value_type(compared) == types::I32 {
        Ok(compared)
    } else {
        Ok(builder.ins().uextend(types::I32, compared))
    }
}

/// Lower a `Switch` to a linear chain of equality tests. `br_table` would be
/// denser for `tableswitch`, but it cannot pass block arguments, and our targets
/// carry block parameters (the live frame) — so each case is a `brif` into the
/// target (with its edge args), falling through to the next test, and the tail
/// jumps to the default. Correct for both `tableswitch` and `lookupswitch`.
fn lower_switch(
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    clif_blocks: &HashMap<BasicBlockId, Block>,
    values: &HashMap<ValueId, Value>,
    key: ValueId,
    cases: &[(i32, BranchEdge)],
    default: &BranchEdge,
) -> Result<(), CodegenError> {
    let key = require_value(function, values, key)?;
    for (match_value, edge) in cases {
        let target = require_block(function, clif_blocks, edge.block)?;
        let target_args = edge_args(function, values, edge)?;
        let match_const = builder.ins().iconst(types::I32, i64::from(*match_value));
        let is_match = builder.ins().icmp(IntCC::Equal, key, match_const);
        let next = builder.create_block();
        builder
            .ins()
            .brif(is_match, target, &target_args, next, &[]);
        builder.switch_to_block(next);
    }
    let default_target = require_block(function, clif_blocks, default.block)?;
    let default_args = edge_args(function, values, default)?;
    builder.ins().jump(default_target, &default_args);
    Ok(())
}

/// Java masks an int shift count to its low 5 bits (`amount & 0x1f`) before
/// shifting, so e.g. `x << 32 == x << 0`. Applying the mask explicitly keeps the
/// semantics correct regardless of the backend's own shift-count handling.
fn shift_amount(builder: &mut FunctionBuilder<'_>, raw: Value) -> Value {
    let mask = builder.ins().iconst(types::I32, 0x1f);
    builder.ins().band(raw, mask)
}

/// `i2b`/`i2s`: narrow to the low byte/half-word, then sign-extend back to i32.
fn sign_extend_from(
    builder: &mut FunctionBuilder<'_>,
    input: Value,
    narrow: cranelift_codegen::ir::Type,
) -> Value {
    let narrowed = builder.ins().ireduce(narrow, input);
    builder.ins().sextend(types::I32, narrowed)
}

/// `i2c`: narrow to the low half-word, then zero-extend (chars are unsigned).
fn zero_extend_from(
    builder: &mut FunctionBuilder<'_>,
    input: Value,
    narrow: cranelift_codegen::ir::Type,
) -> Value {
    let narrowed = builder.ins().ireduce(narrow, input);
    builder.ins().uextend(types::I32, narrowed)
}

/// Java integer division wraps `Integer.MIN_VALUE / -1` to `MIN_VALUE` instead
/// of overflowing; Cranelift's `sdiv` would trap on that one case. The divisor
/// is already known non-zero here (a `ZeroCheck` precedes every division), so we
/// only special-case `b == -1`: divide by 1 instead (never traps) and substitute
/// the correct `-a` result. Branchless, so no extra blocks.
fn checked_sdiv(builder: &mut FunctionBuilder<'_>, lhs: Value, rhs: Value) -> Value {
    let neg_one = builder.ins().iconst(types::I32, -1);
    let is_neg_one = builder.ins().icmp(IntCC::Equal, rhs, neg_one);
    let one = builder.ins().iconst(types::I32, 1);
    let safe_divisor = builder.ins().select(is_neg_one, one, rhs);
    let quotient = builder.ins().sdiv(lhs, safe_divisor);
    let negated = builder.ins().ineg(lhs);
    builder.ins().select(is_neg_one, negated, quotient)
}

/// `Integer.MIN_VALUE % -1` is `0` in Java, where Cranelift's `srem` would trap.
/// Dividing by 1 when `b == -1` yields `a % 1 == 0` — exactly Java's answer — so
/// no result fix-up is needed beyond swapping the divisor.
fn checked_srem(builder: &mut FunctionBuilder<'_>, lhs: Value, rhs: Value) -> Value {
    let neg_one = builder.ins().iconst(types::I32, -1);
    let is_neg_one = builder.ins().icmp(IntCC::Equal, rhs, neg_one);
    let one = builder.ins().iconst(types::I32, 1);
    let safe_divisor = builder.ins().select(is_neg_one, one, rhs);
    builder.ins().srem(lhs, safe_divisor)
}

/// Lower a `ZeroCheck` to an explicit guard: split the current block so control
/// continues only when the checked value is non-zero, and divert the zero case
/// to a runtime abort helper that prints a Java-shaped message and exits
/// deterministically. This is the trap mechanism Phase 2 reuses for null and
/// bounds checks — a branch to a runtime helper, not a bare CPU trap (which
/// would give no message and a signal-shaped exit).
fn lower_zero_check(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    values: &HashMap<ValueId, Value>,
    value: ValueId,
    reason: &TrapReason,
) -> Result<(), CodegenError> {
    let symbol = match reason {
        TrapReason::DivideByZero => TRAP_DIVIDE_BY_ZERO_SYMBOL,
        TrapReason::NullReference | TrapReason::Bounds | TrapReason::Unsupported(_) => {
            return unsupported(
                function,
                "ZeroCheck",
                "only divide-by-zero checks have a runtime trap helper today",
            );
        }
    };
    let checked = require_value(function, values, value)?;
    let zero = builder.ins().iconst(types::I32, 0);
    let is_zero = builder.ins().icmp(IntCC::Equal, checked, zero);
    trap_if(module, builder, function, is_zero, symbol, &[])
}

/// Split the current block so control continues only when `condition` is false;
/// when true, divert to a runtime abort helper (called with `args`) that prints
/// a Java-shaped message and exits deterministically. This is the shared trap
/// mechanism behind divide-by-zero, null, bounds, and negative-array-size
/// checks — a branch to a runtime helper, not a bare CPU trap.
fn trap_if(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    condition: Value,
    symbol: &str,
    args: &[Value],
) -> Result<(), CodegenError> {
    let trap_block = builder.create_block();
    let continue_block = builder.create_block();
    builder
        .ins()
        .brif(condition, trap_block, &[], continue_block, &[]);

    builder.switch_to_block(trap_block);
    let params: Vec<Type> = args
        .iter()
        .map(|arg| builder.func.dfg.value_type(*arg))
        .collect();
    let helper = declare_runtime_helper(module, function, symbol, &params, None)?;
    let local = module.declare_func_in_func(helper, builder.func);
    builder.ins().call(local, args);
    // The helper never returns; this terminator is unreachable but keeps the
    // block well-formed.
    builder.ins().trap(TrapCode::INTEGER_DIVISION_BY_ZERO);

    builder.switch_to_block(continue_block);
    Ok(())
}

/// Declare an imported runtime helper (linked in from the C runtime stub) with
/// the given parameter and return Cranelift types. Re-declaring the same symbol
/// returns the same id.
fn declare_runtime_helper(
    module: &mut ObjectModule,
    function: &FunctionIr,
    symbol: &str,
    params: &[Type],
    returns: Option<Type>,
) -> Result<FuncId, CodegenError> {
    let mut signature = module.make_signature();
    for param in params {
        signature.params.push(AbiParam::new(*param));
    }
    if let Some(returns) = returns {
        signature.returns.push(AbiParam::new(returns));
    }
    module
        .declare_function(symbol, Linkage::Import, &signature)
        .map_err(|source| CodegenError::Backend {
            function: function_label(function),
            message: source.to_string(),
        })
}

/// Materialize a string literal as a read-only, length-prefixed UTF-8 blob
/// (`[i32 byte-length][bytes]`) and return a pointer to it. Identical literals
/// share one blob. This is the minimal self-describing `String` the runtime
/// print helpers consume; the full heap `String` object grows from it in P3.1.
fn lower_string_literal(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    strings: &mut StringPool,
    bytes: &[u8],
) -> Result<Value, CodegenError> {
    let data_id = match strings.get(bytes) {
        Some(id) => *id,
        None => {
            let name = format!("fvm_str_{}", strings.len());
            let id = module
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|source| backend_message(function, source.to_string()))?;
            let length = i32::try_from(bytes.len())
                .map_err(|_| backend_error(function, "string literal exceeds i32 length"))?;
            let mut blob = Vec::with_capacity(4 + bytes.len());
            blob.extend_from_slice(&length.to_le_bytes());
            blob.extend_from_slice(bytes);
            let mut description = DataDescription::new();
            description.define(blob.into_boxed_slice());
            module
                .define_data(id, &description)
                .map_err(|source| backend_message(function, source.to_string()))?;
            strings.insert(bytes.to_vec(), id);
            id
        }
    };
    let global = module.declare_data_in_func(data_id, builder.func);
    Ok(builder.ins().symbol_value(types::I64, global))
}

/// Lower a runtime-library call: the `System.out` print family (void) and the
/// string-concat builder helpers (`sb_new`/`sb_finish` return a pointer).
fn lower_runtime_call(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    values: &mut HashMap<ValueId, Value>,
    result: Option<ValueId>,
    helper: &RuntimeHelper,
    args: &[ValueId],
) -> Result<(), CodegenError> {
    let (symbol, returns) = match helper {
        RuntimeHelper::PrintlnInt => (PRINT_INT_SYMBOL, None),
        RuntimeHelper::PrintInt => (PRINT_INT_RAW_SYMBOL, None),
        RuntimeHelper::PrintlnString => (PRINTLN_STRING_SYMBOL, None),
        RuntimeHelper::PrintString => (PRINT_STRING_SYMBOL, None),
        RuntimeHelper::PrintlnEmpty => (PRINTLN_EMPTY_SYMBOL, None),
        RuntimeHelper::StringBuilderNew => (SB_NEW_SYMBOL, Some(types::I64)),
        RuntimeHelper::StringBuilderAppendInt => (SB_APPEND_INT_SYMBOL, None),
        RuntimeHelper::StringBuilderAppendString => (SB_APPEND_STRING_SYMBOL, None),
        RuntimeHelper::StringBuilderFinish => (SB_FINISH_SYMBOL, Some(types::I64)),
        RuntimeHelper::Println
        | RuntimeHelper::HttpRespond
        | RuntimeHelper::StringConcat
        | RuntimeHelper::ArrayClone
        | RuntimeHelper::ObjectHashCode => {
            return unsupported(
                function,
                "RuntimeCall",
                "only the print and string-concat runtime helpers are compiled today",
            );
        }
    };
    if result.is_some() != returns.is_some() {
        return unsupported(
            function,
            "RuntimeCall",
            "runtime helper result arity does not match its signature",
        );
    }
    let arg_values = args
        .iter()
        .map(|arg| require_value(function, values, *arg))
        .collect::<Result<Vec<_>, _>>()?;
    let param_types: Vec<Type> = arg_values
        .iter()
        .map(|value| builder.func.dfg.value_type(*value))
        .collect();
    let helper_id = declare_runtime_helper(module, function, symbol, &param_types, returns)?;
    let local = module.declare_func_in_func(helper_id, builder.func);
    let call = builder.ins().call(local, &arg_values);
    if let Some(result) = result {
        let value = builder
            .inst_results(call)
            .first()
            .copied()
            .ok_or_else(|| backend_error(function, "runtime helper returned no value"))?;
        values.insert(result, value);
    }
    Ok(())
}

/// Allocate an object: call `fvm_rt_alloc(instance_size)` for zeroed memory and
/// stamp the class id into the header. Fields are already zero (Java's default
/// field values) courtesy of the zero-initialized heap.
fn lower_new_object(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    model: &ObjectModel,
    class: &str,
) -> Result<Value, CodegenError> {
    let layout = model
        .class(class)
        .ok_or_else(|| CodegenError::Unsupported {
            function: function_label(function),
            category: "NewObject",
            detail: format!("no object layout for class {class}"),
        })?;
    let alloc = declare_runtime_helper(
        module,
        function,
        ALLOC_SYMBOL,
        &[types::I64],
        Some(types::I64),
    )?;
    let local = module.declare_func_in_func(alloc, builder.func);
    let size = builder
        .ins()
        .iconst(types::I64, i64::from(layout.instance_size));
    let call = builder.ins().call(local, &[size]);
    let object = builder
        .inst_results(call)
        .first()
        .copied()
        .ok_or_else(|| backend_error(function, "runtime allocator returned no value"))?;
    let class_id = builder.ins().iconst(types::I32, i64::from(layout.class_id));
    builder.ins().store(
        MemFlagsData::trusted(),
        class_id,
        object,
        offset_of(function, CLASS_ID_OFFSET)?,
    );
    Ok(object)
}

/// Allocate an array: `elements_offset + length * stride` bytes (the allocator
/// rounds up and zero-fills), then store the length into the header. Element
/// storage is left zeroed — Java's default element values.
fn lower_new_array(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    element: &IrType,
    length: Value,
) -> Result<Value, CodegenError> {
    let stride = element_stride(function, element)?;
    // Java throws NegativeArraySizeException before allocating.
    let zero = builder.ins().iconst(types::I32, 0);
    let negative = builder.ins().icmp(IntCC::SignedLessThan, length, zero);
    trap_if(
        module,
        builder,
        function,
        negative,
        TRAP_NEGATIVE_ARRAY_SIZE_SYMBOL,
        &[length],
    )?;
    // length is now known non-negative; widen to i64 for the size arithmetic.
    let length_i64 = builder.ins().sextend(types::I64, length);
    let stride_value = builder.ins().iconst(types::I64, i64::from(stride));
    let payload = builder.ins().imul(length_i64, stride_value);
    let header = builder
        .ins()
        .iconst(types::I64, i64::from(ARRAY_ELEMENTS_OFFSET));
    let size = builder.ins().iadd(payload, header);

    let alloc = declare_runtime_helper(
        module,
        function,
        ALLOC_SYMBOL,
        &[types::I64],
        Some(types::I64),
    )?;
    let local = module.declare_func_in_func(alloc, builder.func);
    let call = builder.ins().call(local, &[size]);
    let array = builder
        .inst_results(call)
        .first()
        .copied()
        .ok_or_else(|| backend_error(function, "runtime allocator returned no value"))?;
    builder.ins().store(
        MemFlagsData::trusted(),
        length,
        array,
        offset_of(function, ARRAY_LENGTH_OFFSET)?,
    );
    Ok(array)
}

/// Byte address of `array[index]`: `array + elements_offset + index * stride`.
fn array_element_address(
    function: &FunctionIr,
    builder: &mut FunctionBuilder<'_>,
    array: Value,
    index: Value,
    element: &IrType,
) -> Result<Value, CodegenError> {
    let stride = element_stride(function, element)?;
    let index_i64 = builder.ins().sextend(types::I64, index);
    let stride_value = builder.ins().iconst(types::I64, i64::from(stride));
    let byte_offset = builder.ins().imul(index_i64, stride_value);
    let base = builder
        .ins()
        .iadd_imm(array, i64::from(ARRAY_ELEMENTS_OFFSET));
    Ok(builder.ins().iadd(base, byte_offset))
}

/// Element storage width in bytes. Only int-like (4) and reference (8) elements
/// are supported today; sub-word arrays (byte/char/short) arrive later.
fn element_stride(function: &FunctionIr, element: &IrType) -> Result<u32, CodegenError> {
    match element {
        IrType::Int => Ok(4),
        IrType::Object(_) | IrType::Array(_) => Ok(REFERENCE_BYTES),
        IrType::Boolean | IrType::Char | IrType::Void | IrType::Unsupported(_) => unsupported(
            function,
            "Array",
            "only int and reference array elements are supported today",
        ),
    }
}

/// Resolve a field reference to its byte offset and Cranelift load/store type
/// from the object layout.
fn resolve_field(
    function: &FunctionIr,
    model: &ObjectModel,
    field: &FieldRef,
) -> Result<(u32, Type), CodegenError> {
    let layout = model
        .class(&field.class)
        .ok_or_else(|| CodegenError::Unsupported {
            function: function_label(function),
            category: "Field",
            detail: format!("no object layout for class {}", field.class),
        })?;
    let slot = layout
        .field(&field.name)
        .ok_or_else(|| CodegenError::Unsupported {
            function: function_label(function),
            category: "Field",
            detail: format!("class {} has no field {}", field.class, field.name),
        })?;
    Ok((slot.offset, clif_type(function, &slot.ty)?))
}

fn offset_of(function: &FunctionIr, offset: u32) -> Result<i32, CodegenError> {
    i32::try_from(offset).map_err(|_| backend_error(function, "field offset exceeded i32"))
}

/// Resolve a branch edge's arguments to the Cranelift values threaded into the
/// target block's parameters.
fn edge_args(
    function: &FunctionIr,
    values: &HashMap<ValueId, Value>,
    edge: &BranchEdge,
) -> Result<Vec<BlockArg>, CodegenError> {
    edge.args
        .iter()
        .map(|arg| require_value(function, values, *arg).map(BlockArg::from))
        .collect()
}

fn require_block(
    function: &FunctionIr,
    clif_blocks: &HashMap<BasicBlockId, Block>,
    block: BasicBlockId,
) -> Result<Block, CodegenError> {
    clif_blocks
        .get(&block)
        .copied()
        .ok_or_else(|| backend_error(function, "branch target block was not declared"))
}

fn clif_type(function: &FunctionIr, ty: &IrType) -> Result<Type, CodegenError> {
    match ty {
        IrType::Int | IrType::Boolean | IrType::Char => Ok(types::I32),
        // References are raw pointers.
        IrType::Object(_) | IrType::Array(_) => Ok(types::I64),
        IrType::Void | IrType::Unsupported(_) => unsupported(
            function,
            "Function",
            "the compiler path supports int-compatible and reference values",
        ),
    }
}

fn clif_const(function: &FunctionIr, constant: &IrConst) -> Result<i64, CodegenError> {
    match constant {
        IrConst::Int(value) => Ok(i64::from(*value)),
        IrConst::Boolean(value) => Ok(i64::from(u8::from(*value))),
        IrConst::Char(value) => Ok(i64::from(u32::from(*value))),
        IrConst::Null | IrConst::String(_) => unsupported(
            function,
            "Constant",
            "T23 supports only int-compatible constants",
        ),
    }
}

fn require_value(
    function: &FunctionIr,
    values: &HashMap<ValueId, Value>,
    value: ValueId,
) -> Result<Value, CodegenError> {
    values
        .get(&value)
        .copied()
        .ok_or_else(|| backend_error(function, "IR value was not lowered"))
}

fn backend_error(function: &FunctionIr, message: &str) -> CodegenError {
    CodegenError::Backend {
        function: function_label(function),
        message: message.to_string(),
    }
}

fn backend_message(function: &FunctionIr, message: String) -> CodegenError {
    CodegenError::Backend {
        function: function_label(function),
        message,
    }
}

fn unsupported<T>(
    function: &FunctionIr,
    category: &'static str,
    detail: &str,
) -> Result<T, CodegenError> {
    Err(CodegenError::Unsupported {
        function: function_label(function),
        category,
        detail: detail.to_string(),
    })
}
