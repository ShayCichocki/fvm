use super::super::classfile::{ClassFile, Method};
use super::super::ir::IrType;
use super::super::types::{JvmType, array_component_descriptor, descriptor_to_class};
use anyhow::{Result, bail};

pub(super) fn method_label(class_file: &ClassFile, method: &Method) -> String {
    format!(
        "{}.{}{}",
        class_file.this_name.replace('/', "."),
        method.name,
        method.descriptor
    )
}

pub(super) fn ir_name(class_file: &ClassFile, method: &Method) -> String {
    format!("{}.{}", class_file.this_name.replace('/', "."), method.name)
}

pub(super) fn ir_type_for_jvm(ty: &JvmType, role: &str, method_label: &str) -> Result<IrType> {
    match ty {
        JvmType::Int => Ok(IrType::Int),
        JvmType::Boolean => Ok(IrType::Boolean),
        JvmType::Char => Ok(IrType::Char),
        JvmType::Void => Ok(IrType::Void),
        JvmType::String => Ok(IrType::Object("java/lang/String".to_string())),
        JvmType::Object(class) => Ok(IrType::Object(class.clone())),
        JvmType::Array(descriptor) => Ok(IrType::Array(Box::new(ir_type_for_descriptor(
            array_component_descriptor(descriptor)?,
        )?))),
        JvmType::Unsupported => bail!(
            "fvm-aot lowerer unsupported {role} primitive type in {method_label}; required feature: primitive bytecode; planned milestone: primitive-completeness"
        ),
    }
}

/// Resolve a field descriptor to its IR type. Reference and int-like fields are
/// supported; wide primitives (long/float/double) are rejected with a milestone
/// diagnostic until P2.7.
pub(super) fn field_ir_type(descriptor: &str, method_label: &str) -> Result<IrType> {
    let ty = ir_type_for_descriptor(descriptor)?;
    if matches!(ty, IrType::Unsupported(_)) {
        bail!(
            "fvm-aot lowerer unsupported field type `{descriptor}` in {method_label}; required feature: wide primitive fields; planned milestone: primitive-completeness"
        );
    }
    Ok(ty)
}

fn ir_type_for_descriptor(descriptor: &str) -> Result<IrType> {
    match descriptor {
        "B" | "S" | "I" => Ok(IrType::Int),
        "Z" => Ok(IrType::Boolean),
        "C" => Ok(IrType::Char),
        "Ljava/lang/String;" => Ok(IrType::Object("java/lang/String".to_string())),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => {
            Ok(IrType::Object(descriptor_to_class(descriptor)?.to_string()))
        }
        other => Ok(IrType::Unsupported(other.to_string())),
    }
}
