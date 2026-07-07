use super::{CompilerPipeline, method_label};
use crate::fvm_aot::codegen::cranelift::{emit_objects, exported_symbol};
use crate::fvm_aot::ir::{FunctionIr, IrInstr, IrType, MethodRef};
use crate::fvm_aot::link::{
    EntryReturn, LinkSpec, LinkedExecutable, link_cranelift_object_with_runtime_stub,
};
use crate::fvm_aot::lower::lower_method_to_ir;
use crate::fvm_aot::object_model::ObjectModel;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

pub(in crate::fvm_aot) struct StaticIntMethodSpec<'a> {
    pub(in crate::fvm_aot) class: &'a str,
    pub(in crate::fvm_aot) name: &'a str,
    pub(in crate::fvm_aot) descriptor: &'a str,
    pub(in crate::fvm_aot) cc: &'a str,
    pub(in crate::fvm_aot) output_path: &'a Path,
}

#[derive(Debug)]
pub(in crate::fvm_aot) struct NativeStaticIntMethod {
    path: PathBuf,
    entry_symbol: String,
}

impl NativeStaticIntMethod {
    fn from_linked(linked: LinkedExecutable, entry_symbol: String) -> Self {
        Self {
            path: linked.path().to_path_buf(),
            entry_symbol,
        }
    }

    pub(in crate::fvm_aot) fn path(&self) -> &Path {
        &self.path
    }

    pub(in crate::fvm_aot) fn entry_symbol(&self) -> &str {
        &self.entry_symbol
    }
}

impl CompilerPipeline {
    /// Compile the program's `main` entry through the IR → Cranelift path only.
    ///
    /// This is the "compiler-required" seam: there is no build-time evaluator
    /// fallback here. Any construct the compiler cannot yet express surfaces as
    /// a loud, milestone-tagged lowering diagnostic rather than being silently
    /// constant-folded. Phase 1+ grows what this path accepts.
    pub(in crate::fvm_aot) fn compile_entry(
        &self,
        cc: &str,
        output_path: &Path,
        dry_run: bool,
    ) -> Result<()> {
        let entry = MethodKey::new(&self.main_class, "main", "([Ljava/lang/String;)V");
        let lowered = self.lower_static_int_closure(entry)?;

        if dry_run {
            std::fs::write(output_path, "dry-run fvm-aot compiler-path placeholder\n")
                .with_context(|| {
                    format!(
                        "failed to write compiler-path dry-run placeholder {}",
                        output_path.display()
                    )
                })?;
            return Ok(());
        }

        let functions = lowered.iter().collect::<Vec<_>>();
        let model = ObjectModel::from_classes(&self.world.classes)?;
        let object = emit_objects(&functions, &model)?;
        let entry_name = format!("{}.main", self.main_class.replace('/', "."));
        let entry_symbol = exported_symbol(&entry_name, "([Ljava/lang/String;)V");
        link_cranelift_object_with_runtime_stub(&LinkSpec {
            cc,
            object_bytes: &object,
            entry_symbol: &entry_symbol,
            entry_return: EntryReturn::Void,
            output_path,
        })?;
        Ok(())
    }

    pub(in crate::fvm_aot) fn compile_static_int_method(
        &self,
        spec: &StaticIntMethodSpec<'_>,
    ) -> Result<NativeStaticIntMethod> {
        let entry = MethodKey::new(&spec.class.replace('.', "/"), spec.name, spec.descriptor);
        let lowered = self.lower_static_int_closure(entry)?;
        let functions = lowered.iter().collect::<Vec<_>>();
        let model = ObjectModel::from_classes(&self.world.classes)?;
        let object = emit_objects(&functions, &model)?;
        let entry_name = format!("{}.{}", spec.class.replace('/', "."), spec.name);
        let entry_symbol = exported_symbol(&entry_name, spec.descriptor);
        let linked = link_cranelift_object_with_runtime_stub(&LinkSpec {
            cc: spec.cc,
            object_bytes: &object,
            entry_symbol: &entry_symbol,
            entry_return: entry_return_for_descriptor(spec.descriptor)?,
            output_path: spec.output_path,
        })?;

        Ok(NativeStaticIntMethod::from_linked(linked, entry_symbol))
    }

    fn lower_static_int_closure(&self, entry: MethodKey) -> Result<Vec<FunctionIr>> {
        let mut queue = VecDeque::from([entry]);
        let mut seen = BTreeSet::new();
        let mut lowered = Vec::new();

        while let Some(method_key) = queue.pop_front() {
            if !seen.insert(method_key.clone()) {
                continue;
            }
            let (class_file, method) = self
                .find_method(&method_key.class, &method_key.name, &method_key.descriptor)
                .with_context(|| {
                    format!("fvm-aot compiler could not resolve {}", method_key.label())
                })?;
            let ir = lower_method_to_ir(class_file, method)
                .with_context(|| format!("phase=lower method={}", method_key.label()))?;
            // The compiled entry's result is delivered via the int print ABI, so
            // it must return int; helper methods reached transitively (void
            // constructors, reference-returning helpers) are unconstrained.
            if lowered.is_empty() {
                require_int_return_method(&ir)?;
            }
            for call in direct_static_calls(&ir) {
                if self.world.classes.contains_key(&call.class) {
                    queue.push_back(MethodKey::new(&call.class, &call.name, &call.descriptor));
                } else {
                    bail!(
                        "phase=resolve method={} message=T23 supports direct app-owned static calls only; external call {}.{}{}",
                        method_key.label(),
                        call.class,
                        call.name,
                        call.descriptor
                    );
                }
            }
            lowered.push(ir);
        }

        Ok(lowered)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MethodKey {
    class: String,
    name: String,
    descriptor: String,
}

impl MethodKey {
    fn new(class: &str, name: &str, descriptor: &str) -> Self {
        Self {
            class: class.to_string(),
            name: name.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    fn label(&self) -> String {
        method_label(&self.class, &self.name, &self.descriptor)
    }
}

/// The Cranelift path lowers int-returning static methods; control flow
/// (multiple blocks, loops, branches) is supported as of P1.3. Non-int return
/// types still await later phases (object/void/long ABIs), so they are rejected
/// loudly here rather than reaching codegen.
fn require_int_return_method(function: &FunctionIr) -> Result<()> {
    match &function.return_type {
        IrType::Int if !function.blocks.is_empty() => Ok(()),
        IrType::Int
        | IrType::Void
        | IrType::Boolean
        | IrType::Char
        | IrType::Object(_)
        | IrType::Array(_)
        | IrType::Unsupported(_) => bail!(
            "phase=compiler method={} message=compiler path supports int-returning static methods only",
            function.name
        ),
    }
}

/// Map a compiled entry method's descriptor return type to how its result is
/// delivered. The closure only admits int-returning methods (plus the void
/// `main`), so those are the two cases; anything else is a bug upstream.
fn entry_return_for_descriptor(descriptor: &str) -> Result<EntryReturn> {
    let return_type = descriptor
        .rsplit_once(')')
        .map(|(_params, ret)| ret)
        .with_context(|| format!("entry descriptor `{descriptor}` is missing a return type"))?;
    match return_type {
        "I" => Ok(EntryReturn::Int),
        "V" => Ok(EntryReturn::Void),
        other => bail!(
            "phase=link message=entry return type `{other}` has no delivery ABI yet (descriptor {descriptor})"
        ),
    }
}

fn direct_static_calls(function: &FunctionIr) -> Vec<MethodRef> {
    let mut calls = Vec::new();
    for block in &function.blocks {
        for instr in &block.instrs {
            if let IrInstr::Call(_value, method, _args) = instr {
                calls.push(method.clone());
            }
        }
    }
    calls
}
