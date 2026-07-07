#![allow(dead_code)]

use std::error::Error;
use std::fmt;

pub(super) const STDOUT_WRITE_SYMBOL: &str = "fvm_rt_stdout_write";
pub(super) const PRINT_INT_SYMBOL: &str = "fvm_rt_print_int";
pub(super) const PRINT_INT_RAW_SYMBOL: &str = "fvm_rt_print_int_raw";
pub(super) const PRINTLN_STRING_SYMBOL: &str = "fvm_rt_println_string";
pub(super) const PRINT_STRING_SYMBOL: &str = "fvm_rt_print_string";
pub(super) const PRINTLN_EMPTY_SYMBOL: &str = "fvm_rt_println_empty";
pub(super) const SB_NEW_SYMBOL: &str = "fvm_rt_sb_new";
pub(super) const SB_APPEND_INT_SYMBOL: &str = "fvm_rt_sb_append_int";
pub(super) const SB_APPEND_STRING_SYMBOL: &str = "fvm_rt_sb_append_string";
pub(super) const SB_FINISH_SYMBOL: &str = "fvm_rt_sb_finish";
pub(super) const PROCESS_EXIT_SYMBOL: &str = "fvm_rt_process_exit";
pub(super) const TRAP_UNSUPPORTED_SYMBOL: &str = "fvm_rt_trap_unsupported";
pub(super) const TRAP_DIVIDE_BY_ZERO_SYMBOL: &str = "fvm_rt_trap_divide_by_zero";
pub(super) const TRAP_NULL_SYMBOL: &str = "fvm_rt_trap_null";
pub(super) const TRAP_BOUNDS_SYMBOL: &str = "fvm_rt_trap_bounds";
pub(super) const TRAP_NEGATIVE_ARRAY_SIZE_SYMBOL: &str = "fvm_rt_trap_negative_array_size";
pub(super) const ALLOC_SYMBOL: &str = "fvm_rt_alloc";
pub(super) const ALLOC_OBJECT_SYMBOL: &str = "fvm_rt_alloc_object";

const M1_HELPERS: &[RuntimeAbiHelper] = &[
    RuntimeAbiHelper::TrapUnsupported,
    RuntimeAbiHelper::TrapDivideByZero,
    RuntimeAbiHelper::TrapNull,
    RuntimeAbiHelper::TrapBounds,
    RuntimeAbiHelper::TrapNegativeArraySize,
    RuntimeAbiHelper::StdoutWrite,
    RuntimeAbiHelper::PrintInt,
    RuntimeAbiHelper::PrintIntRaw,
    RuntimeAbiHelper::PrintlnString,
    RuntimeAbiHelper::PrintString,
    RuntimeAbiHelper::PrintlnEmpty,
    RuntimeAbiHelper::StringBuilderNew,
    RuntimeAbiHelper::StringBuilderAppendInt,
    RuntimeAbiHelper::StringBuilderAppendString,
    RuntimeAbiHelper::StringBuilderFinish,
    RuntimeAbiHelper::ProcessExit,
    RuntimeAbiHelper::Alloc,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeAbiHelper {
    StdoutWrite,
    PrintInt,
    PrintIntRaw,
    PrintlnString,
    PrintString,
    PrintlnEmpty,
    StringBuilderNew,
    StringBuilderAppendInt,
    StringBuilderAppendString,
    StringBuilderFinish,
    ProcessExit,
    TrapUnsupported,
    TrapDivideByZero,
    TrapNull,
    TrapBounds,
    TrapNegativeArraySize,
    Alloc,
    AllocObject,
}

impl RuntimeAbiHelper {
    pub(super) const fn symbol(self) -> &'static str {
        match self {
            Self::StdoutWrite => STDOUT_WRITE_SYMBOL,
            Self::PrintInt => PRINT_INT_SYMBOL,
            Self::PrintIntRaw => PRINT_INT_RAW_SYMBOL,
            Self::PrintlnString => PRINTLN_STRING_SYMBOL,
            Self::PrintString => PRINT_STRING_SYMBOL,
            Self::PrintlnEmpty => PRINTLN_EMPTY_SYMBOL,
            Self::StringBuilderNew => SB_NEW_SYMBOL,
            Self::StringBuilderAppendInt => SB_APPEND_INT_SYMBOL,
            Self::StringBuilderAppendString => SB_APPEND_STRING_SYMBOL,
            Self::StringBuilderFinish => SB_FINISH_SYMBOL,
            Self::ProcessExit => PROCESS_EXIT_SYMBOL,
            Self::TrapUnsupported => TRAP_UNSUPPORTED_SYMBOL,
            Self::TrapDivideByZero => TRAP_DIVIDE_BY_ZERO_SYMBOL,
            Self::TrapNull => TRAP_NULL_SYMBOL,
            Self::TrapBounds => TRAP_BOUNDS_SYMBOL,
            Self::TrapNegativeArraySize => TRAP_NEGATIVE_ARRAY_SIZE_SYMBOL,
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
            Self::PrintIntRaw => {
                RuntimeHelperDeclaration::trusted(PRINT_INT_RAW_SYMBOL, "void", &["int32_t value"])
            }
            Self::PrintlnString => RuntimeHelperDeclaration::trusted(
                PRINTLN_STRING_SYMBOL,
                "void",
                &["const void *str"],
            ),
            Self::PrintString => {
                RuntimeHelperDeclaration::trusted(PRINT_STRING_SYMBOL, "void", &["const void *str"])
            }
            Self::PrintlnEmpty => {
                RuntimeHelperDeclaration::trusted(PRINTLN_EMPTY_SYMBOL, "void", &[])
            }
            Self::StringBuilderNew => {
                RuntimeHelperDeclaration::trusted(SB_NEW_SYMBOL, "void *", &[])
            }
            Self::StringBuilderAppendInt => RuntimeHelperDeclaration::trusted(
                SB_APPEND_INT_SYMBOL,
                "void",
                &["void *builder", "int32_t value"],
            ),
            Self::StringBuilderAppendString => RuntimeHelperDeclaration::trusted(
                SB_APPEND_STRING_SYMBOL,
                "void",
                &["void *builder", "const void *str"],
            ),
            Self::StringBuilderFinish => {
                RuntimeHelperDeclaration::trusted(SB_FINISH_SYMBOL, "void *", &["void *builder"])
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
            Self::TrapNull => RuntimeHelperDeclaration::trusted(TRAP_NULL_SYMBOL, "void", &[]),
            Self::TrapBounds => RuntimeHelperDeclaration::trusted(
                TRAP_BOUNDS_SYMBOL,
                "void",
                &["int32_t index", "int32_t length"],
            ),
            Self::TrapNegativeArraySize => RuntimeHelperDeclaration::trusted(
                TRAP_NEGATIVE_ARRAY_SIZE_SYMBOL,
                "void",
                &["int32_t size"],
            ),
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
        "#include <stddef.h>\n#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\n",
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
    source.push_str(&trap_null_definition());
    source.push('\n');
    source.push_str(&trap_bounds_definition());
    source.push('\n');
    source.push_str(&trap_negative_array_size_definition());
    source.push('\n');
    source.push_str(&stdout_write_definition());
    source.push('\n');
    source.push_str(&print_int_definition());
    source.push('\n');
    source.push_str(&print_int_raw_definition());
    source.push('\n');
    source.push_str(&string_print_definitions());
    source.push('\n');
    source.push_str(&alloc_definition());
    source.push('\n');
    source.push_str(&string_builder_definitions());
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

/// `System.out.print(int)` — the value with no trailing newline.
fn print_int_raw_definition() -> String {
    format!("void {PRINT_INT_RAW_SYMBOL}(int32_t value) {{\n  printf(\"%d\", (int)value);\n}}\n")
}

/// String printing. A compiled string literal is a length-prefixed UTF-8 blob:
/// an `int32` byte length followed by the bytes (the minimal shape the real
/// Phase 3 `String` object will grow from). `memcpy` reads the length without
/// assuming pointer alignment.
fn string_print_definitions() -> String {
    format!(
        "static void fvm_rt_write_string(const void *str) {{\n\
         \x20 int32_t len;\n\
         \x20 memcpy(&len, str, sizeof(int32_t));\n\
         \x20 const unsigned char *bytes = (const unsigned char *)str + sizeof(int32_t);\n\
         \x20 fwrite(bytes, 1, (size_t)len, stdout);\n\
         }}\n\
         void {PRINT_STRING_SYMBOL}(const void *str) {{\n\
         \x20 fvm_rt_write_string(str);\n\
         }}\n\
         void {PRINTLN_STRING_SYMBOL}(const void *str) {{\n\
         \x20 fvm_rt_write_string(str);\n\
         \x20 fputc('\\n', stdout);\n\
         }}\n\
         void {PRINTLN_EMPTY_SYMBOL}(void) {{\n\
         \x20 fputc('\\n', stdout);\n\
         }}\n"
    )
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

/// Uncaught `NullPointerException` from a null field/array access. The
/// helpful-NPE detail message and stack trace arrive with Phase 5.
fn trap_null_definition() -> String {
    format!(
        "void {TRAP_NULL_SYMBOL}(void) {{\n  fputs(\"Exception in thread \\\"main\\\" java.lang.NullPointerException\\n\", stderr);\n  exit(1);\n}}\n"
    )
}

/// Uncaught `ArrayIndexOutOfBoundsException`, matching HotSpot's message shape
/// (`Index N out of bounds for length M`).
fn trap_bounds_definition() -> String {
    format!(
        "void {TRAP_BOUNDS_SYMBOL}(int32_t index, int32_t length) {{\n  fprintf(stderr, \"Exception in thread \\\"main\\\" java.lang.ArrayIndexOutOfBoundsException: Index %d out of bounds for length %d\\n\", (int)index, (int)length);\n  exit(1);\n}}\n"
    )
}

/// Uncaught `NegativeArraySizeException`, matching HotSpot's message shape (the
/// requested size).
fn trap_negative_array_size_definition() -> String {
    format!(
        "void {TRAP_NEGATIVE_ARRAY_SIZE_SYMBOL}(int32_t size) {{\n  fprintf(stderr, \"Exception in thread \\\"main\\\" java.lang.NegativeArraySizeException: %d\\n\", (int)size);\n  exit(1);\n}}\n"
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

/// A transient string builder backing `StringConcatFactory` string concatenation
/// (`"a" + b`). It grows on the C heap (malloc/realloc) while appending; `finish`
/// copies the result into a managed, length-prefixed `String` blob and frees the
/// scratch buffer. The full `StringBuilder` API is P3.4.
fn string_builder_definitions() -> String {
    format!(
        "typedef struct {{ unsigned char *data; size_t len; size_t cap; }} fvm_rt_sb;\n\
         static void fvm_rt_sb_reserve(fvm_rt_sb *sb, size_t extra) {{\n\
         \x20 if (sb->len + extra <= sb->cap) return;\n\
         \x20 size_t cap = sb->cap ? sb->cap : 16;\n\
         \x20 while (sb->len + extra > cap) cap *= 2;\n\
         \x20 sb->data = (unsigned char *)realloc(sb->data, cap);\n\
         \x20 if (sb->data == NULL) {{ {TRAP_UNSUPPORTED_SYMBOL}(\"fvm-aot runtime: string builder out of memory\"); }}\n\
         \x20 sb->cap = cap;\n\
         }}\n\
         void *{SB_NEW_SYMBOL}(void) {{\n\
         \x20 fvm_rt_sb *sb = (fvm_rt_sb *)malloc(sizeof(fvm_rt_sb));\n\
         \x20 if (sb == NULL) {{ {TRAP_UNSUPPORTED_SYMBOL}(\"fvm-aot runtime: string builder out of memory\"); }}\n\
         \x20 sb->data = NULL; sb->len = 0; sb->cap = 0;\n\
         \x20 return sb;\n\
         }}\n\
         void {SB_APPEND_INT_SYMBOL}(void *builder, int32_t value) {{\n\
         \x20 fvm_rt_sb *sb = (fvm_rt_sb *)builder;\n\
         \x20 char buffer[16];\n\
         \x20 int written = snprintf(buffer, sizeof(buffer), \"%d\", (int)value);\n\
         \x20 fvm_rt_sb_reserve(sb, (size_t)written);\n\
         \x20 memcpy(sb->data + sb->len, buffer, (size_t)written);\n\
         \x20 sb->len += (size_t)written;\n\
         }}\n\
         void {SB_APPEND_STRING_SYMBOL}(void *builder, const void *str) {{\n\
         \x20 fvm_rt_sb *sb = (fvm_rt_sb *)builder;\n\
         \x20 int32_t len; memcpy(&len, str, sizeof(int32_t));\n\
         \x20 const unsigned char *bytes = (const unsigned char *)str + sizeof(int32_t);\n\
         \x20 fvm_rt_sb_reserve(sb, (size_t)len);\n\
         \x20 memcpy(sb->data + sb->len, bytes, (size_t)len);\n\
         \x20 sb->len += (size_t)len;\n\
         }}\n\
         void *{SB_FINISH_SYMBOL}(void *builder) {{\n\
         \x20 fvm_rt_sb *sb = (fvm_rt_sb *)builder;\n\
         \x20 void *result = {ALLOC_SYMBOL}((int64_t)(sizeof(int32_t) + sb->len));\n\
         \x20 int32_t len = (int32_t)sb->len;\n\
         \x20 memcpy(result, &len, sizeof(int32_t));\n\
         \x20 memcpy((unsigned char *)result + sizeof(int32_t), sb->data, sb->len);\n\
         \x20 free(sb->data); free(sb);\n\
         \x20 return result;\n\
         }}\n"
    )
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
