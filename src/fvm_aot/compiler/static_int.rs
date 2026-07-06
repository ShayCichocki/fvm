use super::{CompilerPipeline, method_label};
use crate::fvm_aot::codegen::cranelift::{emit_objects, exported_symbol};
use crate::fvm_aot::ir::{FunctionIr, IrInstr, IrType, MethodRef};
use crate::fvm_aot::link::{LinkSpec, LinkedExecutable, link_cranelift_object_with_runtime_stub};
use crate::fvm_aot::lower::lower_method_to_ir;
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
    pub(in crate::fvm_aot) fn compile_static_int_method(
        &self,
        spec: &StaticIntMethodSpec<'_>,
    ) -> Result<NativeStaticIntMethod> {
        let entry = MethodKey::new(&spec.class.replace('.', "/"), spec.name, spec.descriptor);
        let lowered = self.lower_static_int_closure(entry)?;
        let functions = lowered.iter().collect::<Vec<_>>();
        let object = emit_objects(&functions)?;
        let entry_name = format!("{}.{}", spec.class.replace('/', "."), spec.name);
        let entry_symbol = exported_symbol(&entry_name);
        let linked = link_cranelift_object_with_runtime_stub(&LinkSpec {
            cc: spec.cc,
            object_bytes: &object,
            entry_symbol: &entry_symbol,
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
            require_static_int_method(&ir)?;
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

fn require_static_int_method(function: &FunctionIr) -> Result<()> {
    match (&function.return_type, function.blocks.as_slice()) {
        (IrType::Int, [_]) => Ok(()),
        (IrType::Void, _)
        | (IrType::Boolean, _)
        | (IrType::Char, _)
        | (IrType::Object(_), _)
        | (IrType::Array(_), _)
        | (IrType::Unsupported(_), _)
        | (IrType::Int, [])
        | (IrType::Int, [_, _, ..]) => bail!(
            "phase=compiler method={} message=T23 supports single-block static int methods only",
            function.name
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
