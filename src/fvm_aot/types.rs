use anyhow::{Context, Result, bail};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum JvmType {
    Int,
    Boolean,
    Char,
    String,
    Object(String),
    Array(String),
    Void,
    Unsupported,
}

pub(super) fn parse_method_descriptor(descriptor: &str) -> Result<(Vec<JvmType>, JvmType)> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        bail!("invalid method descriptor `{descriptor}`");
    }
    let mut index = 1_usize;
    let mut params = Vec::new();
    while index < bytes.len() && bytes[index] != b')' {
        params.push(parse_type(descriptor, &mut index)?);
    }
    if index >= bytes.len() || bytes[index] != b')' {
        bail!("invalid method descriptor `{descriptor}`");
    }
    index += 1;
    let return_type = parse_type(descriptor, &mut index)?;
    if index != bytes.len() {
        bail!("invalid trailing method descriptor data `{descriptor}`");
    }
    Ok((params, return_type))
}

fn parse_type(descriptor: &str, index: &mut usize) -> Result<JvmType> {
    let bytes = descriptor.as_bytes();
    if *index >= bytes.len() {
        bail!("truncated method descriptor `{descriptor}`");
    }
    let ty = match bytes[*index] {
        b'B' | b'S' | b'I' => {
            *index += 1;
            JvmType::Int
        }
        b'C' => {
            *index += 1;
            JvmType::Char
        }
        b'Z' => {
            *index += 1;
            JvmType::Boolean
        }
        b'V' => {
            *index += 1;
            JvmType::Void
        }
        b'L' => {
            let start = *index + 1;
            let Some(end) = descriptor[start..].find(';').map(|offset| start + offset) else {
                bail!("unterminated object type in descriptor `{descriptor}`");
            };
            let class = &descriptor[start..end];
            *index = end + 1;
            if class == "java/lang/String" {
                JvmType::String
            } else {
                JvmType::Object(class.to_string())
            }
        }
        b'[' => {
            let start = *index;
            while *index < bytes.len() && bytes[*index] == b'[' {
                *index += 1;
            }
            let _ = parse_type(descriptor, index)?;
            JvmType::Array(descriptor[start..*index].to_string())
        }
        _ => {
            *index += 1;
            JvmType::Unsupported
        }
    };
    Ok(ty)
}

pub(super) fn supported_field_descriptor(descriptor: &str) -> Result<()> {
    match descriptor {
        "B" | "S" | "I" | "Z" | "C" | "Ljava/lang/String;" => Ok(()),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => Ok(()),
        descriptor if descriptor.starts_with('[') => {
            let component = array_component_descriptor(descriptor)?;
            supported_array_component(component)
        }
        other => bail!("fvm-aot unsupported field descriptor {other}"),
    }
}

fn supported_array_component(descriptor: &str) -> Result<()> {
    match descriptor {
        "B" | "S" | "I" | "Z" | "C" | "Ljava/lang/String;" => Ok(()),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => Ok(()),
        other => bail!("fvm-aot unsupported array component descriptor {other}"),
    }
}

pub(super) fn class_descriptor(class: &str) -> String {
    format!("L{class};")
}

pub(super) fn descriptor_to_class(descriptor: &str) -> Result<&str> {
    descriptor
        .strip_prefix('L')
        .and_then(|value| value.strip_suffix(';'))
        .with_context(|| format!("invalid object descriptor {descriptor}"))
}

pub(super) fn array_component_descriptor(descriptor: &str) -> Result<&str> {
    let component = descriptor
        .strip_prefix('[')
        .with_context(|| format!("invalid array descriptor {descriptor}"))?;
    if component.starts_with('[') {
        bail!("fvm-aot only supports one-dimensional arrays for now");
    }
    Ok(component)
}

pub(super) fn newarray_component_descriptor(atype: u8) -> Result<&'static str> {
    match atype {
        4 => Ok("Z"),
        5 => Ok("C"),
        8 => Ok("B"),
        9 => Ok("S"),
        10 => Ok("I"),
        other => bail!("fvm-aot unsupported newarray atype {other}"),
    }
}

pub(super) fn primitive_array_opcode_matches(expected: &str, actual: &str) -> bool {
    match expected {
        "B/Z" => actual == "B" || actual == "Z",
        descriptor => actual == descriptor,
    }
}
