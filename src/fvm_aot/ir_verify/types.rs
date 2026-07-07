use super::{IrConst, IrType, RuntimeHelper, TrapReason};
use anyhow::{Result, bail};

pub(super) fn constant_type(constant: &IrConst) -> IrType {
    match constant {
        IrConst::Int(_) => IrType::Int,
        IrConst::Boolean(_) => IrType::Boolean,
        IrConst::Char(_) => IrType::Char,
        IrConst::Null => IrType::Object("null".to_string()),
        IrConst::String(_) => IrType::Object("java/lang/String".to_string()),
    }
}

pub(super) fn runtime_return_type(helper: &RuntimeHelper) -> Option<IrType> {
    match helper {
        RuntimeHelper::Println
        | RuntimeHelper::PrintlnInt
        | RuntimeHelper::PrintlnString
        | RuntimeHelper::PrintlnEmpty
        | RuntimeHelper::PrintInt
        | RuntimeHelper::PrintString
        | RuntimeHelper::StringBuilderAppendInt
        | RuntimeHelper::StringBuilderAppendString
        | RuntimeHelper::HttpRespond => None,
        RuntimeHelper::StringBuilderNew => {
            Some(IrType::Object("fvm/runtime/StringBuilder".to_string()))
        }
        RuntimeHelper::StringConcat | RuntimeHelper::StringBuilderFinish => {
            Some(IrType::Object("java/lang/String".to_string()))
        }
        RuntimeHelper::ArrayClone => Some(IrType::Object("java/lang/Object".to_string())),
        RuntimeHelper::ObjectHashCode => Some(IrType::Int),
    }
}

pub(super) fn descriptor_return_type(descriptor: &str) -> Result<IrType> {
    let Some((_params, return_descriptor)) = descriptor.rsplit_once(')') else {
        bail!("IR call descriptor `{descriptor}` is missing return type")
    };
    descriptor_type(return_descriptor)
}

pub(super) fn return_compatible(expected: &IrType, actual: &IrType) -> bool {
    if expected == actual {
        return true;
    }
    match (expected, actual) {
        (IrType::Boolean | IrType::Char, IrType::Int) => true,
        (IrType::Object(_) | IrType::Array(_), IrType::Object(class)) => class == "null",
        (IrType::Void, _)
        | (IrType::Int, _)
        | (IrType::Boolean, _)
        | (IrType::Char, _)
        | (IrType::Object(_), _)
        | (IrType::Array(_), _)
        | (IrType::Unsupported(_), _) => false,
    }
}

pub(super) fn verify_descriptor_model_return(
    label: &str,
    descriptor: &str,
    modeled: &IrType,
) -> Result<()> {
    let descriptor_return = descriptor_return_type(descriptor)?;
    verify_supported_type(label, "descriptor return type", &descriptor_return)?;
    if return_compatible(&descriptor_return, modeled) {
        return Ok(());
    }
    bail!(
        "IR function `{label}` descriptor return type mismatch: descriptor {descriptor_return}, modeled {modeled}"
    )
}

pub(super) fn verify_supported_type(label: &str, role: &str, ty: &IrType) -> Result<()> {
    match ty {
        IrType::Void | IrType::Int | IrType::Boolean | IrType::Char | IrType::Object(_) => Ok(()),
        IrType::Array(element) => verify_supported_type(label, role, element),
        IrType::Unsupported(_) => bail!("IR function `{label}` has unsupported {role}: {ty}"),
    }
}

pub(super) fn verify_supported_trap(label: &str, reason: &TrapReason) -> Result<()> {
    match reason {
        TrapReason::NullReference | TrapReason::Bounds | TrapReason::DivideByZero => Ok(()),
        TrapReason::Unsupported(reason) => {
            bail!("IR function `{label}` has unsupported trap reason: {reason}")
        }
    }
}

fn descriptor_type(descriptor: &str) -> Result<IrType> {
    match descriptor {
        "V" => Ok(IrType::Void),
        "B" | "S" | "I" => Ok(IrType::Int),
        "Z" => Ok(IrType::Boolean),
        "C" => Ok(IrType::Char),
        "Ljava/lang/String;" => Ok(IrType::Object("java/lang/String".to_string())),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => Ok(
            IrType::Object(descriptor[1..descriptor.len() - 1].to_string()),
        ),
        descriptor if descriptor.starts_with('[') => {
            Ok(IrType::Array(Box::new(descriptor_type(&descriptor[1..])?)))
        }
        other => Ok(IrType::Unsupported(other.to_string())),
    }
}
