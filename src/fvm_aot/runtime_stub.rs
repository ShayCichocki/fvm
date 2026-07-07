#![allow(dead_code)]

use std::error::Error;
use std::fmt;

pub(super) const STDOUT_WRITE_SYMBOL: &str = "fvm_rt_stdout_write";
pub(super) const PRINT_INT_SYMBOL: &str = "fvm_rt_print_int";
pub(super) const PROCESS_EXIT_SYMBOL: &str = "fvm_rt_process_exit";
pub(super) const TRAP_UNSUPPORTED_SYMBOL: &str = "fvm_rt_trap_unsupported";
pub(super) const TRAP_DIVIDE_BY_ZERO_SYMBOL: &str = "fvm_rt_trap_divide_by_zero";
pub(super) const ALLOC_SYMBOL: &str = "fvm_rt_alloc";
pub(super) const ALLOC_OBJECT_SYMBOL: &str = "fvm_rt_alloc_object";

const M1_HELPERS: &[RuntimeAbiHelper] = &[
    RuntimeAbiHelper::TrapUnsupported,
    RuntimeAbiHelper::TrapDivideByZero,
    RuntimeAbiHelper::StdoutWrite,
    RuntimeAbiHelper::PrintInt,
    RuntimeAbiHelper::ProcessExit,
    RuntimeAbiHelper::Alloc,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeAbiHelper {
    StdoutWrite,
    PrintInt,
    ProcessExit,
    TrapUnsupported,
    TrapDivideByZero,
    Alloc,
    AllocObject,
}

impl RuntimeAbiHelper {
    pub(super) const fn symbol(self) -> &'static str {
        match self {
            Self::StdoutWrite => STDOUT_WRITE_SYMBOL,
            Self::PrintInt => PRINT_INT_SYMBOL,
            Self::ProcessExit => PROCESS_EXIT_SYMBOL,
            Self::TrapUnsupported => TRAP_UNSUPPORTED_SYMBOL,
            Self::TrapDivideByZero => TRAP_DIVIDE_BY_ZERO_SYMBOL,
            Self::Alloc => ALLOC_SYMBOL,
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
            Self::PrintInt => {
                RuntimeHelperDeclaration::trusted(PRINT_INT_SYMBOL, "void", &["int32_t value"])
            }
            Self::ProcessExit => {
                RuntimeHelperDeclaration::trusted(PROCESS_EXIT_SYMBOL, "void", &["int32_t code"])
            }
            Self::TrapUnsupported => RuntimeHelperDeclaration::trusted(
                TRAP_UNSUPPORTED_SYMBOL,
                "void",
                &["const char *message"],
            ),
            Self::TrapDivideByZero => {
                RuntimeHelperDeclaration::trusted(TRAP_DIVIDE_BY_ZERO_SYMBOL, "void", &[])
            }
            Self::Alloc => {
                RuntimeHelperDeclaration::trusted(ALLOC_SYMBOL, "void *", &["int64_t size"])
            }
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
        let params = if self.params.is_empty() {
            "void".to_string()
        } else {
            self.params.join(", ")
        };
        format!("{} {}({})", self.return_type, self.symbol, params)
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
    source.push_str(&trap_divide_by_zero_definition());
    source.push('\n');
    source.push_str(&stdout_write_definition());
    source.push('\n');
    source.push_str(&print_int_definition());
    source.push('\n');
    source.push_str(&alloc_definition());
    source.push('\n');
    source.push_str(&process_exit_definition());
    source
}

/// A bump allocator over a fixed BSS heap: zero-initialized (so freshly returned
/// objects have zeroed fields/headers), 8-byte aligned, with a deterministic
/// abort on exhaustion. GC arrives in Phase 5; until then a long-running
/// allocator will eventually OOM — documented and honest.
fn alloc_definition() -> String {
    format!(
        "#define FVM_RT_HEAP_BYTES (64u * 1024u * 1024u)\n\
         static unsigned char fvm_rt_heap[FVM_RT_HEAP_BYTES];\n\
         static size_t fvm_rt_heap_used = 0;\n\
         void *{ALLOC_SYMBOL}(int64_t size) {{\n\
         \x20 if (size < 0) {{\n\
         \x20   {TRAP_UNSUPPORTED_SYMBOL}(\"fvm-aot runtime: negative allocation size\");\n\
         \x20 }}\n\
         \x20 size_t bytes = (size_t)size;\n\
         \x20 size_t aligned = (bytes + 7u) & ~(size_t)7u;\n\
         \x20 if (aligned > FVM_RT_HEAP_BYTES - fvm_rt_heap_used) {{\n\
         \x20   fputs(\"fvm-aot runtime: heap exhausted\\n\", stderr);\n\
         \x20   exit(137);\n\
         \x20 }}\n\
         \x20 void *object = &fvm_rt_heap[fvm_rt_heap_used];\n\
         \x20 fvm_rt_heap_used += aligned;\n\
         \x20 return object;\n\
         }}\n"
    )
}

/// Delivers an int the way `System.out.println(int)` does: decimal digits then a
/// newline on stdout. This replaces the old exit-code result channel, which
/// truncated results to 8 bits (`return 342` exited 86) and poisoned
/// differential comparisons against HotSpot.
fn print_int_definition() -> String {
    format!("void {PRINT_INT_SYMBOL}(int32_t value) {{\n  printf(\"%d\\n\", (int)value);\n}}\n")
}

fn trap_unsupported_definition() -> String {
    format!(
        "void {TRAP_UNSUPPORTED_SYMBOL}(const char *message) {{\n  if (message != NULL) {{\n    fputs(message, stderr);\n    fputc('\\n', stderr);\n  }}\n  abort();\n}}\n"
    )
}

/// Java raises `ArithmeticException: / by zero` for integer `/0` and `%0`.
/// Until Phase 5 turns traps into catchable exception objects, an uncaught
/// division trap prints the exception's first line to stderr and exits 1 —
/// deterministic, and the exit code an uncaught exception yields on HotSpot.
fn trap_divide_by_zero_definition() -> String {
    format!(
        "void {TRAP_DIVIDE_BY_ZERO_SYMBOL}(void) {{\n  fputs(\"Exception in thread \\\"main\\\" java.lang.ArithmeticException: / by zero\\n\", stderr);\n  exit(1);\n}}\n"
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

pub(super) fn is_c_identifier(value: &str) -> bool {
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
