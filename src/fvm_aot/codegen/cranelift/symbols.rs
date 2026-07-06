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

pub(in crate::fvm_aot) fn exported_symbol(function_name: &str) -> String {
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
