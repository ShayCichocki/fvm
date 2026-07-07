use super::super::classfile::{ClassFile, Code};
use super::super::ir::{FieldRef, IrConst, IrType, MethodRef, RuntimeHelper};
use super::super::types::{JvmType, parse_method_descriptor};
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

pub(super) fn push_ldc_constant(input: &mut CallLowering<'_, '_>, index: u16) -> Result<()> {
    if let Ok(value) = input.class_file.int_constant(index) {
        let _ = input.state.push_constant(IrConst::Int(value));
        return Ok(());
    }
    if let Ok(text) = input.class_file.string_constant(index) {
        let _ = input
            .state
            .push_constant(IrConst::String(text.into_bytes()));
        return Ok(());
    }
    bail!(
        "fvm-aot lowerer only supports int and String ldc constants (index {index}); required feature: float/class constants; planned milestone: primitive-completeness"
    )
}

/// `getstatic` pushes `System.out` as a sentinel PrintStream, or reads an
/// application class's static field from its per-class static storage. Static
/// fields of classes outside the closed world (other JDK statics) surface as a
/// loud codegen diagnostic — no static storage exists for them.
pub(super) fn lower_getstatic(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let member = input
        .class_file
        .field_ref(read_u16(&input.code.bytes, input.pc)?)?;
    if member.class == "java/lang/System"
        && member.name == "out"
        && member.descriptor == "Ljava/io/PrintStream;"
    {
        input.state.push_stdout();
        return Ok(());
    }
    let ty = field_ir_type(&member.descriptor, input.method_label)?;
    input.state.push_static_get(FieldRef {
        class: member.class,
        name: member.name,
        ty,
    });
    Ok(())
}

/// `putstatic` writes an application class's static field into its per-class
/// static storage. As with `getstatic`, fields outside the closed world are
/// rejected loudly in codegen.
pub(super) fn lower_putstatic(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let member = input
        .class_file
        .field_ref(read_u16(&input.code.bytes, input.pc)?)?;
    let ty = field_ir_type(&member.descriptor, input.method_label)?;
    input.state.store_static_put(FieldRef {
        class: member.class,
        name: member.name,
        ty,
    })
}

/// `invokevirtual` is intrinsified only for `System.out` `print`/`println`.
/// General virtual dispatch awaits P3.3.
pub(super) fn lower_invokevirtual(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let method_ref = input
        .class_file
        .method_ref(read_u16(&input.code.bytes, input.pc)?)?;
    if method_ref.class == "java/io/PrintStream" {
        return lower_print_stream_call(input, &method_ref);
    }
    bail!(
        "fvm-aot lowerer only supports System.out.print/println virtual calls today, not {}.{}{}; required feature: virtual dispatch; planned milestone: dispatch-and-strings",
        method_ref.class,
        method_ref.name,
        method_ref.descriptor
    )
}

fn lower_print_stream_call(
    input: &mut CallLowering<'_, '_>,
    method_ref: &super::super::classfile::ResolvedMember,
) -> Result<()> {
    let (helper, has_argument) = match (method_ref.name.as_str(), method_ref.descriptor.as_str()) {
        ("println", "()V") => (RuntimeHelper::PrintlnEmpty, false),
        ("println", "(I)V") => (RuntimeHelper::PrintlnInt, true),
        ("println", "(Ljava/lang/String;)V") => (RuntimeHelper::PrintlnString, true),
        ("print", "(I)V") => (RuntimeHelper::PrintInt, true),
        ("print", "(Ljava/lang/String;)V") => (RuntimeHelper::PrintString, true),
        (name, descriptor) => bail!(
            "fvm-aot lowerer supports PrintStream.print(int|String)/println(int|String|<empty>) only, not {name}{descriptor}; required feature: print overloads; planned milestone: dispatch-and-strings"
        ),
    };
    let mut args = Vec::new();
    if has_argument {
        args.push(input.state.pop_stack()?);
    }
    let _receiver = input.state.pop_stack()?; // the System.out sentinel
    input.state.emit_runtime_call(helper, args);
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

/// `invokedynamic` is supported only for `StringConcatFactory` string
/// concatenation (`"a" + b`), which javac emits as
/// `makeConcat`/`makeConcatWithConstants`. Everything else (lambdas, etc.) is
/// rejected until later phases.
pub(super) fn lower_invokedynamic(input: &mut CallLowering<'_, '_>) -> Result<()> {
    let index = read_u16(&input.code.bytes, input.pc)?;
    let _ = read_u16(&input.code.bytes, input.pc)?; // two reserved zero bytes

    let indy = input.class_file.invoke_dynamic(index)?;
    let bootstrap = input
        .class_file
        .bootstrap_method(indy.bootstrap_method_attr_index)?
        .clone();
    let handle = input.class_file.method_handle_ref(bootstrap.method_ref)?;

    if handle.class != "java/lang/invoke/StringConcatFactory" {
        bail!(
            "fvm-aot lowerer only supports StringConcatFactory invokedynamic today, not {}.{}; required feature: invokedynamic (lambdas/indy); planned milestone: dispatch-and-strings",
            handle.class,
            handle.name
        );
    }

    let (params, return_type) = parse_method_descriptor(&indy.descriptor)?;
    if !matches!(return_type, JvmType::String | JvmType::Object(_)) {
        bail!(
            "fvm-aot lowerer string concat must return String, got descriptor {}",
            indy.descriptor
        );
    }

    match handle.name.as_str() {
        "makeConcat" => lower_concat(input, &params, None, &[]),
        "makeConcatWithConstants" => {
            let recipe =
                input
                    .class_file
                    .string_constant(*bootstrap.arguments.first().context(
                        "makeConcatWithConstants bootstrap is missing its recipe argument",
                    )?)?;
            let statics = bootstrap.arguments[1..].to_vec();
            lower_concat(input, &params, Some(&recipe), &statics)
        }
        other => bail!("fvm-aot lowerer unsupported StringConcatFactory method {other}"),
    }
}

/// Build the concatenation via the runtime string builder: `sb_new`, an append
/// per recipe segment (literal text, a dynamic argument, or a static constant),
/// then `sb_finish` producing the result String.
fn lower_concat(
    input: &mut CallLowering<'_, '_>,
    params: &[JvmType],
    recipe: Option<&str>,
    statics: &[u16],
) -> Result<()> {
    // Dynamic args were pushed left-to-right, so they pop in reverse.
    let mut dynamics = Vec::with_capacity(params.len());
    for param in params.iter().rev() {
        let ty = ir_type_for_jvm(param, "concat argument", input.method_label)?;
        let (value, _stack_ty) = input.state.pop_value()?;
        dynamics.push((value, ty));
    }
    dynamics.reverse();
    let mut dynamics = dynamics.into_iter();

    let builder_ty = IrType::Object("fvm/runtime/StringBuilder".to_string());
    let sb =
        input
            .state
            .emit_runtime_value(RuntimeHelper::StringBuilderNew, Vec::new(), builder_ty);

    match recipe {
        None => {
            // makeConcat: every parameter is a dynamic placeholder, no literals.
            for (value, ty) in dynamics.by_ref() {
                append_dynamic(input, sb, value, &ty)?;
            }
        }
        Some(recipe) => {
            let mut statics = statics.iter();
            let mut literal = Vec::new();
            for character in recipe.chars() {
                match character {
                    '\u{1}' => {
                        flush_literal(input, sb, &mut literal);
                        let (value, ty) = dynamics.next().context(
                            "string concat recipe references more arguments than supplied",
                        )?;
                        append_dynamic(input, sb, value, &ty)?;
                    }
                    '\u{2}' => {
                        flush_literal(input, sb, &mut literal);
                        let constant = *statics.next().context(
                            "string concat recipe references more constants than supplied",
                        )?;
                        append_static_constant(input, sb, constant)?;
                    }
                    text => {
                        let mut buffer = [0_u8; 4];
                        literal.extend_from_slice(text.encode_utf8(&mut buffer).as_bytes());
                    }
                }
            }
            flush_literal(input, sb, &mut literal);
        }
    }

    let result_ty = IrType::Object("java/lang/String".to_string());
    let result = input.state.emit_runtime_value(
        RuntimeHelper::StringBuilderFinish,
        vec![sb],
        result_ty.clone(),
    );
    input.state.push_value(result, result_ty);
    Ok(())
}

fn flush_literal(
    input: &mut CallLowering<'_, '_>,
    sb: super::super::ir::ValueId,
    literal: &mut Vec<u8>,
) {
    if literal.is_empty() {
        return;
    }
    let value = input.state.emit_string_constant(std::mem::take(literal));
    input
        .state
        .emit_runtime_call(RuntimeHelper::StringBuilderAppendString, vec![sb, value]);
}

fn append_dynamic(
    input: &mut CallLowering<'_, '_>,
    sb: super::super::ir::ValueId,
    value: super::super::ir::ValueId,
    ty: &IrType,
) -> Result<()> {
    match ty {
        IrType::Int => input
            .state
            .emit_runtime_call(RuntimeHelper::StringBuilderAppendInt, vec![sb, value]),
        IrType::Object(_) => input
            .state
            .emit_runtime_call(RuntimeHelper::StringBuilderAppendString, vec![sb, value]),
        other => bail!(
            "fvm-aot lowerer string concat supports int and String arguments only, not {other}; required feature: full concat conversions; planned milestone: dispatch-and-strings"
        ),
    }
    Ok(())
}

fn append_static_constant(
    input: &mut CallLowering<'_, '_>,
    sb: super::super::ir::ValueId,
    constant: u16,
) -> Result<()> {
    if let Ok(text) = input.class_file.string_constant(constant) {
        let value = input.state.emit_string_constant(text.into_bytes());
        input
            .state
            .emit_runtime_call(RuntimeHelper::StringBuilderAppendString, vec![sb, value]);
        return Ok(());
    }
    if let Ok(number) = input.class_file.int_constant(constant) {
        let value = input.state.emit_constant(IrConst::Int(number));
        input
            .state
            .emit_runtime_call(RuntimeHelper::StringBuilderAppendInt, vec![sb, value]);
        return Ok(());
    }
    bail!("fvm-aot lowerer string concat supports int and String constants only (index {constant})")
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
