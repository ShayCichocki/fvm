use super::super::classfile::{ClassFile, Code};
use super::super::ir::{FieldRef, IrType, MethodRef};
use super::super::types::parse_method_descriptor;
use super::bytecode::read_u16;
use super::metadata::{field_ir_type, ir_type_for_jvm};
use super::state::LowerState;
use anyhow::{Context, Result, bail};

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

pub(super) fn lower_new(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let class = input
        .class_file
        .class_name(read_u16(&input.code.bytes, input.pc)?)?;
    input.state.push_new_object(class);
    Ok(())
}

pub(super) fn lower_getfield(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let field = resolve_field(input)?;
    input.state.push_field_get(field)
}

pub(super) fn lower_putfield(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let field = resolve_field(input)?;
    input.state.store_field_put(field)
}

fn resolve_field(input: &mut CallLowering<'_, '_>) -> Result<FieldRef> {
    let member = input
        .class_file
        .field_ref(read_u16(&input.code.bytes, input.pc)?)?;
    let ty = field_ir_type(&member.descriptor, input.method_label)?;
    Ok(FieldRef {
        class: member.class,
        name: member.name,
        ty,
    })
}

/// `invokespecial` covers constructors, `super` calls, and private methods. The
/// only such call we treat specially is `java/lang/Object.<init>`, which does
/// nothing — its receiver is simply dropped. Every other target is a direct call
/// with the receiver passed as the first argument.
pub(super) fn lower_invokespecial(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let method_ref = input
        .class_file
        .method_ref(read_u16(&input.code.bytes, input.pc)?)?;

    if method_ref.class == "java/lang/Object" && method_ref.name == "<init>" {
        let _ = input.state.pop_stack()?; // drop the receiver; Object.<init> is a no-op
        return Ok(());
    }

    let (param_types, return_type) = parse_method_descriptor(&method_ref.descriptor)?;
    let mut args = Vec::with_capacity(param_types.len() + 1);
    for param_type in param_types.iter().rev() {
        let _ = ir_type_for_jvm(param_type, "call parameter", input.method_label)?;
        args.push(input.state.pop_stack()?);
    }
    let receiver = input.state.pop_stack()?;
    args.push(receiver);
    args.reverse();
    let return_type = ir_type_for_jvm(&return_type, "call return", input.method_label)?;
    if return_type != IrType::Void {
        bail!(
            "fvm-aot lowerer only supports void invokespecial (constructors) today, not {}.{}{}",
            method_ref.class,
            method_ref.name,
            method_ref.descriptor
        );
    }
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
