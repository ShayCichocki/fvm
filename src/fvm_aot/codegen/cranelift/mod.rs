mod error;
mod lower;
mod symbols;

use super::super::ir::FunctionIr;
use super::super::object_model::ObjectModel;
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::{isa, settings};
use cranelift_module::{Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
pub(in crate::fvm_aot) use error::CodegenError;
pub(in crate::fvm_aot) use symbols::exported_symbol;
use target_lexicon::HOST;

pub(in crate::fvm_aot) fn emit_object(function: &FunctionIr) -> Result<Vec<u8>, CodegenError> {
    emit_objects(&[function], &ObjectModel::empty())
}

pub(in crate::fvm_aot) fn emit_objects(
    functions: &[&FunctionIr],
    model: &ObjectModel,
) -> Result<Vec<u8>, CodegenError> {
    for function in functions {
        function.verify().map_err(|source| CodegenError::Verify {
            function: symbols::function_label(function),
            source,
        })?;
    }

    let mut module = object_module(functions)?;
    lower::emit_functions(&mut module, functions, model)?;
    module
        .finish()
        .emit()
        .map_err(|source| CodegenError::Backend {
            function: "<object>".to_string(),
            message: source.to_string(),
        })
}

fn object_module(functions: &[&FunctionIr]) -> Result<ObjectModule, CodegenError> {
    let function = functions.first().map_or_else(
        || "<empty>".to_string(),
        |function| symbols::function_label(function),
    );
    // Position-independent code so calls into the separately-linked C runtime
    // stub (e.g. trap helpers) go through the GOT/PLT. Without this, macOS's
    // linker rejects the absolute text-relocations Cranelift would otherwise
    // emit for external symbols.
    let mut flag_builder = settings::builder();
    flag_builder
        .set("is_pic", "true")
        .map_err(|source| CodegenError::Backend {
            function: function.clone(),
            message: source.to_string(),
        })?;
    let flags = settings::Flags::new(flag_builder);
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
            &exported_symbol(&function.name, &function.descriptor),
            Linkage::Export,
            &signature,
        )
        .map_err(|source| CodegenError::Backend {
            function: symbols::function_label(function),
            message: source.to_string(),
        })
}
