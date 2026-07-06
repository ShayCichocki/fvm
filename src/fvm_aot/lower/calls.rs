use super::super::classfile::{ClassFile, Code};
use super::super::ir::MethodRef;
use super::super::types::parse_method_descriptor;
use super::bytecode::read_u16;
use super::metadata::ir_type_for_jvm;
use super::state::LowerState;
use anyhow::{Context, Result};

pub(super) struct CallLowering<'a, 'b> {
    pub(super) class_file: &'a ClassFile,
    pub(super) code: &'a Code,
    pub(super) pc: &'b mut usize,
    pub(super) method_label: &'a str,
    pub(super) state: &'b mut LowerState,
}

pub(super) fn push_int_constant(input: &mut CallLowering<'_, '_>, index: u16) -> Result<()> {
    let value = input.class_file.int_constant(index).with_context(|| {
        format!("fvm-aot lowerer only supports integer ldc constants at index {index}")
    })?;
    let _ = input
        .state
        .push_constant(super::super::ir::IrConst::Int(value));
    Ok(())
}

pub(super) fn lower_invokestatic(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let method_ref = input
        .class_file
        .method_ref(read_u16(&input.code.bytes, input.pc)?)?;
    let (param_types, return_type) = parse_method_descriptor(&method_ref.descriptor)?;
    let mut args = Vec::with_capacity(param_types.len());
    for param_type in param_types.iter().rev() {
        let _ = ir_type_for_jvm(param_type, "call parameter", input.method_label)?;
        args.push(input.state.pop_stack()?);
    }
    args.reverse();
    let return_type = ir_type_for_jvm(&return_type, "call return", input.method_label)?;
    input.state.push_static_call(
        MethodRef {
            class: method_ref.class,
            name: method_ref.name,
            descriptor: method_ref.descriptor,
        },
        args,
        return_type,
    );
    Ok(())
}
