use super::super::ir::{FunctionIr, IrConst, IrInstr, IrType};
use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_codegen::{isa, settings};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::error::Error;
use std::fmt;
use target_lexicon::HOST;

pub(in crate::fvm_aot) fn emit_object(function: &FunctionIr) -> Result<Vec<u8>, CodegenError> {
    function.verify().map_err(|source| CodegenError::Verify {
        function: function_label(function),
        source,
    })?;

    let constant = constant_return(function)?;
    let mut module = object_module(function)?;
    let symbol = exported_symbol(&function.name);
    let mut signature = module.make_signature();
    signature.returns.push(AbiParam::new(types::I32));
    let func_id = module
        .declare_function(&symbol, Linkage::Export, &signature)
        .map_err(|source| CodegenError::Backend {
            function: function_label(function),
            message: source.to_string(),
        })?;

    let mut context = module.make_context();
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry = builder.create_block();
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let value = builder.ins().iconst(types::I32, i64::from(constant));
    builder.ins().return_(&[value]);
    builder.finalize();

    module
        .define_function(func_id, &mut context)
        .map_err(|source| CodegenError::Backend {
            function: function_label(function),
            message: source.to_string(),
        })?;
    module
        .finish()
        .emit()
        .map_err(|source| CodegenError::Backend {
            function: function_label(function),
            message: source.to_string(),
        })
}

#[derive(Debug)]
pub(in crate::fvm_aot) enum CodegenError {
    Verify {
        function: String,
        source: anyhow::Error,
    },
    Unsupported {
        function: String,
        category: &'static str,
        detail: String,
    },
    Backend {
        function: String,
        message: String,
    },
}

impl fmt::Display for CodegenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verify { function, source } => {
                write!(
                    formatter,
                    "phase=verify function={function} message={source}"
                )
            }
            Self::Unsupported {
                function,
                category,
                detail,
            } => write!(
                formatter,
                "phase=unsupported-codegen function={function} instruction={category} message={detail}"
            ),
            Self::Backend { function, message } => {
                write!(
                    formatter,
                    "phase=cranelift-object function={function} message={message}"
                )
            }
        }
    }
}

impl Error for CodegenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Verify { source, .. } => Some(source.as_ref()),
            Self::Unsupported { .. } | Self::Backend { .. } => None,
        }
    }
}

fn constant_return(function: &FunctionIr) -> Result<i32, CodegenError> {
    match (
        &function.return_type,
        function.params.as_slice(),
        function.blocks.as_slice(),
    ) {
        (IrType::Int, [], [block]) => match block.instrs.as_slice() {
            [
                IrInstr::Constant(value, IrConst::Int(constant)),
                IrInstr::Return(Some(returned)),
            ] if value == returned => Ok(*constant),
            [IrInstr::Constant(..), IrInstr::Return(Some(_))] => unsupported(
                function,
                "Return",
                "T20 supports returning the same int constant value only",
            ),
            [] => unsupported(function, "Block", "T20 requires a non-empty entry block"),
            instrs => unsupported(
                function,
                unsupported_instruction_category(instrs),
                "T20 supports exactly `int constant; return constant`",
            ),
        },
        (IrType::Void, _, _)
        | (IrType::Boolean, _, _)
        | (IrType::Char, _, _)
        | (IrType::Object(_), _, _)
        | (IrType::Array(_), _, _)
        | (IrType::Unsupported(_), _, _) => unsupported(
            function,
            "Function",
            "T20 supports only zero-parameter functions returning int",
        ),
        (IrType::Int, [_, ..], _) => unsupported(
            function,
            "Function",
            "T20 supports only zero-parameter functions returning int",
        ),
        (IrType::Int, [], []) | (IrType::Int, [], [_, _, ..]) => {
            unsupported(function, "Function", "T20 supports exactly one basic block")
        }
    }
}

fn object_module(function: &FunctionIr) -> Result<ObjectModule, CodegenError> {
    let flags = settings::Flags::new(settings::builder());
    let isa = isa::lookup(HOST).map_err(|source| CodegenError::Backend {
        function: function_label(function),
        message: source.to_string(),
    })?;
    let isa = isa.finish(flags).map_err(|source| CodegenError::Backend {
        function: function_label(function),
        message: source.to_string(),
    })?;
    let builder =
        ObjectBuilder::new(isa, "fvm-aot", default_libcall_names()).map_err(|source| {
            CodegenError::Backend {
                function: function_label(function),
                message: source.to_string(),
            }
        })?;
    Ok(ObjectModule::new(builder))
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

fn function_label(function: &FunctionIr) -> String {
    format!("{}{}", function.name, function.descriptor)
}

fn exported_symbol(function_name: &str) -> String {
    let mut symbol = String::from("fvm_aot_");
    for character in function_name.chars() {
        match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' => symbol.push(character),
            '.' | '/' | '$' | '<' | '>' | '-' => symbol.push('_'),
            _ => symbol.push('_'),
        }
    }
    symbol
}

fn instruction_category(instr: &IrInstr) -> &'static str {
    match instr {
        IrInstr::Param(..) => "Param",
        IrInstr::Constant(..) => "Constant",
        IrInstr::Compare(..) => "Compare",
        IrInstr::Arithmetic(..) => "Arithmetic",
        IrInstr::Unary(..) => "Unary",
        IrInstr::Branch(..) => "Branch",
        IrInstr::CondBranch(..) => "CondBranch",
        IrInstr::Call(..) => "Call",
        IrInstr::RuntimeCall(..) => "RuntimeCall",
        IrInstr::Return(..) => "Return",
        IrInstr::FieldGet(..) => "FieldGet",
        IrInstr::FieldPut(..) => "FieldPut",
        IrInstr::ArrayLoad(..) => "ArrayLoad",
        IrInstr::ArrayStore(..) => "ArrayStore",
        IrInstr::ArrayLength(..) => "ArrayLength",
        IrInstr::NewObject(..) => "NewObject",
        IrInstr::NewArray(..) => "NewArray",
        IrInstr::ZeroCheck(..) => "ZeroCheck",
        IrInstr::NullCheck(..) => "NullCheck",
        IrInstr::BoundsCheck(..) => "BoundsCheck",
        IrInstr::ExceptionEdge(..) => "ExceptionEdge",
        IrInstr::Trap(..) => "Trap",
    }
}

fn unsupported_instruction_category(instrs: &[IrInstr]) -> &'static str {
    for instr in instrs {
        match instr {
            IrInstr::Constant(_, IrConst::Int(_)) | IrInstr::Return(..) => {}
            IrInstr::Constant(..)
            | IrInstr::Param(..)
            | IrInstr::Compare(..)
            | IrInstr::Arithmetic(..)
            | IrInstr::Unary(..)
            | IrInstr::Branch(..)
            | IrInstr::CondBranch(..)
            | IrInstr::Call(..)
            | IrInstr::RuntimeCall(..)
            | IrInstr::FieldGet(..)
            | IrInstr::FieldPut(..)
            | IrInstr::ArrayLoad(..)
            | IrInstr::ArrayStore(..)
            | IrInstr::ArrayLength(..)
            | IrInstr::NewObject(..)
            | IrInstr::NewArray(..)
            | IrInstr::ZeroCheck(..)
            | IrInstr::NullCheck(..)
            | IrInstr::BoundsCheck(..)
            | IrInstr::ExceptionEdge(..)
            | IrInstr::Trap(..) => return instruction_category(instr),
        }
    }
    "Function"
}
