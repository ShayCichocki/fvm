mod error;
mod lower;
mod symbols;

use super::super::ir::FunctionIr;
use cranelift_codegen::{isa, settings};
use cranelift_module::{Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
pub(in crate::fvm_aot) use error::CodegenError;
pub(in crate::fvm_aot) use symbols::exported_symbol;
use target_lexicon::HOST;

pub(in crate::fvm_aot) fn emit_object(function: &FunctionIr) -> Result<Vec<u8>, CodegenError> {
    emit_objects(&[function])
}

pub(in crate::fvm_aot) fn emit_objects(functions: &[&FunctionIr]) -> Result<Vec<u8>, CodegenError> {
    for function in functions {
        function.verify().map_err(|source| CodegenError::Verify {
            function: symbols::function_label(function),
            source,
        })?;
    }

    let mut module = object_module(functions)?;
    lower::emit_functions(&mut module, functions)?;
    module
        .finish()
        .emit()
        .map_err(|source| CodegenError::Backend {
            function: "<object>".to_string(),
            message: source.to_string(),
        })
}

fn object_module(functions: &[&FunctionIr]) -> Result<ObjectModule, CodegenError> {
    let flags = settings::Flags::new(settings::builder());
    let function = functions.first().map_or_else(
        || "<empty>".to_string(),
        |function| symbols::function_label(function),
    );
    let isa = isa::lookup(HOST).map_err(|source| CodegenError::Backend {
        function: function.clone(),
        message: source.to_string(),
    })?;
    let isa = isa.finish(flags).map_err(|source| CodegenError::Backend {
        function: function.clone(),
        message: source.to_string(),
    })?;
    let builder =
        ObjectBuilder::new(isa, "fvm-aot", default_libcall_names()).map_err(|source| {
            CodegenError::Backend {
                function,
                message: source.to_string(),
            }
        })?;
    Ok(ObjectModule::new(builder))
}

fn declare_exported_function(
    module: &mut ObjectModule,
    function: &FunctionIr,
) -> Result<cranelift_module::FuncId, CodegenError> {
    let signature = lower::signature(module, function)?;
    module
        .declare_function(
            &exported_symbol(&function.name),
            Linkage::Export,
            &signature,
        )
        .map_err(|source| CodegenError::Backend {
            function: symbols::function_label(function),
            message: source.to_string(),
        })
}
