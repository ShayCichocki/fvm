mod categories;

use super::declare_exported_function;
use super::error::CodegenError;
use super::symbols::{function_label, method_label};
use crate::fvm_aot::ir::{
    FunctionIr, IrArithmeticOp, IrConst, IrInstr, IrType, IrUnaryOp, ValueId,
};
use categories::instruction_category;
use cranelift_codegen::ir::{AbiParam, Signature, Value, types};
use cranelift_codegen::ir::{InstBuilder, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap};

pub(super) fn emit_functions(
    module: &mut ObjectModule,
    functions: &[&FunctionIr],
) -> Result<(), CodegenError> {
    let ids = declare_functions(module, functions)?;
    for function in functions {
        emit_function(module, function, &ids)?;
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
) -> Result<(), CodegenError> {
    let func_id = *ids
        .get(&function_label(function))
        .ok_or_else(|| backend_error(function, "function was not declared"))?;
    let mut context = module.make_context();
    context.func.signature = signature(module, function)?;
    context.func.name = UserFuncName::user(0, func_id.as_u32());

    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    for param in &function.params {
        builder.append_block_param(entry, clif_type(function, &param.ty)?);
    }
    builder.switch_to_block(entry);
    builder.seal_block(entry);

    let mut values = HashMap::new();
    for (param, value) in function
        .params
        .iter()
        .zip(builder.block_params(entry).iter())
    {
        values.insert(param.value, *value);
    }
    let [block] = function.blocks.as_slice() else {
        return unsupported(
            function,
            "Function",
            "T23 supports single-block static integer methods",
        );
    };
    for instr in &block.instrs {
        lower_instr(module, &mut builder, function, ids, &mut values, instr)?;
    }
    builder.finalize();

    module
        .define_function(func_id, &mut context)
        .map_err(|source| CodegenError::Backend {
            function: function_label(function),
            message: source.to_string(),
        })
}

fn lower_instr(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder<'_>,
    function: &FunctionIr,
    ids: &BTreeMap<String, FuncId>,
    values: &mut HashMap<ValueId, Value>,
    instr: &IrInstr,
) -> Result<(), CodegenError> {
    match instr {
        IrInstr::Param(..) => Ok(()),
        IrInstr::Constant(value, constant) => {
            let constant = clif_const(function, constant)?;
            values.insert(*value, builder.ins().iconst(types::I32, constant));
            Ok(())
        }
        IrInstr::Arithmetic(value, op, lhs, rhs) => {
            let lhs = require_value(function, values, *lhs)?;
            let rhs = require_value(function, values, *rhs)?;
            let result = match op {
                IrArithmeticOp::Add => builder.ins().iadd(lhs, rhs),
                IrArithmeticOp::Sub => builder.ins().isub(lhs, rhs),
                IrArithmeticOp::Mul => builder.ins().imul(lhs, rhs),
                IrArithmeticOp::Div => builder.ins().sdiv(lhs, rhs),
                IrArithmeticOp::Rem => builder.ins().srem(lhs, rhs),
            };
            values.insert(*value, result);
            Ok(())
        }
        IrInstr::Unary(value, IrUnaryOp::Neg, input) => {
            let input = require_value(function, values, *input)?;
            values.insert(*value, builder.ins().ineg(input));
            Ok(())
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
        IrInstr::NewObject(..) => unsupported(
            function,
            "NewObject",
            "runtime allocation is planned for milestone runtime-allocation",
        ),
        other => unsupported(
            function,
            instruction_category(other),
            "T23 supports int constants, arithmetic, returns, and direct static calls",
        ),
    }
}

fn clif_type(
    function: &FunctionIr,
    ty: &IrType,
) -> Result<cranelift_codegen::ir::Type, CodegenError> {
    match ty {
        IrType::Int | IrType::Boolean | IrType::Char => Ok(types::I32),
        IrType::Void | IrType::Object(_) | IrType::Array(_) | IrType::Unsupported(_) => {
            unsupported(
                function,
                "Function",
                "T23 supports only int-compatible static method values",
            )
        }
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
