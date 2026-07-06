#![allow(dead_code)]

use std::error::Error;
use std::fmt;

pub(super) const STDOUT_WRITE_SYMBOL: &str = "fvm_rt_stdout_write";
pub(super) const PROCESS_EXIT_SYMBOL: &str = "fvm_rt_process_exit";
pub(super) const TRAP_UNSUPPORTED_SYMBOL: &str = "fvm_rt_trap_unsupported";
pub(super) const ALLOC_OBJECT_SYMBOL: &str = "fvm_rt_alloc_object";

const M1_HELPERS: &[RuntimeAbiHelper] = &[
    RuntimeAbiHelper::TrapUnsupported,
    RuntimeAbiHelper::StdoutWrite,
    RuntimeAbiHelper::ProcessExit,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeAbiHelper {
    StdoutWrite,
    ProcessExit,
    TrapUnsupported,
    AllocObject,
}

impl RuntimeAbiHelper {
    pub(super) const fn symbol(self) -> &'static str {
        match self {
            Self::StdoutWrite => STDOUT_WRITE_SYMBOL,
            Self::ProcessExit => PROCESS_EXIT_SYMBOL,
            Self::TrapUnsupported => TRAP_UNSUPPORTED_SYMBOL,
            Self::AllocObject => ALLOC_OBJECT_SYMBOL,
        }
    }

    const fn declaration(self) -> RuntimeHelperDeclaration {
        match self {
            Self::StdoutWrite => RuntimeHelperDeclaration::trusted(
                STDOUT_WRITE_SYMBOL,
                "void",
                &["const unsigned char *bytes", "size_t len"],
            ),
            Self::ProcessExit => {
                RuntimeHelperDeclaration::trusted(PROCESS_EXIT_SYMBOL, "void", &["int32_t code"])
            }
            Self::TrapUnsupported => RuntimeHelperDeclaration::trusted(
                TRAP_UNSUPPORTED_SYMBOL,
                "void",
                &["const char *message"],
            ),
            Self::AllocObject => RuntimeHelperDeclaration::trusted(
                ALLOC_OBJECT_SYMBOL,
                "void *",
                &["uint32_t class_id"],
            ),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct RuntimeHelperDeclaration {
    symbol: &'static str,
    return_type: &'static str,
    params: &'static [&'static str],
}

impl RuntimeHelperDeclaration {
    pub(super) fn new(
        symbol: &'static str,
        return_type: &'static str,
        params: &'static [&'static str],
    ) -> Result<Self, RuntimeStubError> {
        if is_c_identifier(symbol) {
            return Ok(Self {
                symbol,
                return_type,
                params,
            });
        }

        Err(RuntimeStubError::MalformedDeclaration { symbol })
    }

    const fn trusted(
        symbol: &'static str,
        return_type: &'static str,
        params: &'static [&'static str],
    ) -> Self {
        Self {
            symbol,
            return_type,
            params,
        }
    }

    fn prototype(&self) -> String {
        format!(
            "{} {}({})",
            self.return_type,
            self.symbol,
            self.params.join(", ")
        )
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum RuntimeStubError {
    MalformedDeclaration { symbol: &'static str },
}

impl fmt::Display for RuntimeStubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedDeclaration { symbol } => write!(
                formatter,
                "runtime helper declaration `{symbol}` is not a valid C identifier"
            ),
        }
    }
}

impl Error for RuntimeStubError {}

pub(super) fn emit_runtime_stub_c() -> String {
    let mut source = String::from(
        "#include <stddef.h>\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n\n",
    );
    for helper in M1_HELPERS {
        source.push_str(&helper.declaration().prototype());
        source.push_str(";\n");
    }
    source.push('\n');
    source.push_str(&trap_unsupported_definition());
    source.push('\n');
    source.push_str(&stdout_write_definition());
    source.push('\n');
    source.push_str(&process_exit_definition());
    source
}

fn trap_unsupported_definition() -> String {
    format!(
        "void {TRAP_UNSUPPORTED_SYMBOL}(const char *message) {{\n  if (message != NULL) {{\n    fputs(message, stderr);\n    fputc('\\n', stderr);\n  }}\n  abort();\n}}\n"
    )
}

fn stdout_write_definition() -> String {
    format!(
        "void {STDOUT_WRITE_SYMBOL}(const unsigned char *bytes, size_t len) {{\n  if (bytes == NULL && len != 0U) {{\n    {TRAP_UNSUPPORTED_SYMBOL}(\"stdout write received a null buffer\");\n  }}\n  fwrite(bytes, 1U, len, stdout);\n}}\n"
    )
}

fn process_exit_definition() -> String {
    format!("void {PROCESS_EXIT_SYMBOL}(int32_t code) {{\n  exit((int)code);\n}}\n")
}

fn is_c_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, 'A'..='Z' | 'a'..='z' | '_')
        && chars.all(|character| matches!(character, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_future_allocation_symbol_without_emitting_it_for_m1() {
        let source = emit_runtime_stub_c();

        assert_eq!(RuntimeAbiHelper::AllocObject.symbol(), ALLOC_OBJECT_SYMBOL);
        assert!(!source.contains("fvm_rt_alloc_object("));
    }
}
