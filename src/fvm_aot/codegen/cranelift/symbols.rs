use super::super::super::ir::{FunctionIr, MethodRef};

pub(in crate::fvm_aot) fn function_label(function: &FunctionIr) -> String {
    format!("{}{}", function.name, function.descriptor)
}

pub(in crate::fvm_aot) fn method_label(method: &MethodRef) -> String {
    format!(
        "{}.{}{}",
        method.class.replace('/', "."),
        method.name,
        method.descriptor
    )
}

/// Mangle a method's name **and descriptor** into a C-safe linkage symbol.
///
/// The descriptor is included so overloads — same class and name, different
/// parameter types — get distinct symbols instead of colliding on one. The name
/// stays readable (alphanumerics pass through, everything else becomes `_`);
/// the descriptor is hex-escaped (`(` → `_28`) so two distinct descriptors can
/// never map to the same suffix. The result is always a valid C identifier.
pub(in crate::fvm_aot) fn exported_symbol(function_name: &str, descriptor: &str) -> String {
    let mut symbol = String::from("fvm_aot_");
    for character in function_name.chars() {
        match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' => symbol.push(character),
            _ => symbol.push('_'),
        }
    }
    for byte in descriptor.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => symbol.push(char::from(byte)),
            _ => {
                symbol.push('_');
                symbol.push(hex_digit(byte >> 4));
                symbol.push(hex_digit(byte & 0x0f));
            }
        }
    }
    symbol
}

fn hex_digit(nibble: u8) -> char {
    char::from(match nibble {
        0..=9 => b'0' + nibble,
        _ => b'a' + (nibble - 10),
    })
}
