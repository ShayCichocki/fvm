use super::{CompilerPipeline, method_label};
use crate::fvm_aot::codegen::cranelift::{emit_objects, exported_symbol};
use crate::fvm_aot::ir::{FunctionIr, IrInstr, IrType, MethodRef};
use crate::fvm_aot::link::{
    EntryReturn, LinkSpec, LinkedExecutable, link_cranelift_object_with_runtime_stub,
};
use crate::fvm_aot::lower::lower_method_to_ir;
use crate::fvm_aot::object_model::ObjectModel;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

const CLINIT_NAME: &str = "<clinit>";
const CLINIT_DESCRIPTOR: &str = "()V";

/// The result of walking the compile closure: every function to emit, plus the
/// class-initializer symbols (`<clinit>`) in the order they must run at startup.
struct LoweredClosure {
    functions: Vec<FunctionIr>,
    clinit_symbols: Vec<String>,
}

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

        let functions = lowered.functions.iter().collect::<Vec<_>>();
        let model = ObjectModel::from_classes(&self.world.classes)?;
        let object = emit_objects(&functions, &model)?;
        let entry_name = format!("{}.main", self.main_class.replace('/', "."));
        let entry_symbol = exported_symbol(&entry_name, "([Ljava/lang/String;)V");
        link_cranelift_object_with_runtime_stub(&LinkSpec {
            cc,
            object_bytes: &object,
            entry_symbol: &entry_symbol,
            entry_return: EntryReturn::Void,
            clinit_symbols: &lowered.clinit_symbols,
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
        let functions = lowered.functions.iter().collect::<Vec<_>>();
        let model = ObjectModel::from_classes(&self.world.classes)?;
        let object = emit_objects(&functions, &model)?;
        let entry_name = format!("{}.{}", spec.class.replace('/', "."), spec.name);
        let entry_symbol = exported_symbol(&entry_name, spec.descriptor);
        let linked = link_cranelift_object_with_runtime_stub(&LinkSpec {
            cc: spec.cc,
            object_bytes: &object,
            entry_symbol: &entry_symbol,
            entry_return: entry_return_for_descriptor(spec.descriptor)?,
            clinit_symbols: &lowered.clinit_symbols,
            output_path: spec.output_path,
        })?;

        Ok(NativeStaticIntMethod::from_linked(linked, entry_symbol))
    }

    fn lower_static_int_closure(&self, entry: MethodKey) -> Result<LoweredClosure> {
        let mut queue = VecDeque::from([entry.clone()]);
        let mut seen = BTreeSet::new();
        let mut lowered = Vec::new();
        // Classes whose `<clinit>` must run at startup: JVMS triggers class
        // initialization on first active use (a static call, `new`, or static
        // field access). We over-approximate to "any actively-used class with a
        // `<clinit>`, batched at process start" per P2.4's simplest-correct v1.
        let mut clinit_classes = BTreeSet::new();
        // Invoking the entry (a static method) is itself an active use of its
        // declaring class.
        self.consider_class_init(&entry.class, &mut clinit_classes, &mut queue);

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
            // The compiled entry is delivered via the entry ABI: an int result
            // is printed, a void method just runs (output comes from println
            // calls). Reference/other returns have no ABI. Helpers reached
            // transitively are unconstrained.
            if lowered.is_empty() {
                require_entry_return_method(&ir)?;
            }
            // Any actively-used closed-world class needs its `<clinit>` scheduled
            // (and compiled). `<clinit>` bodies are walked the same way, so their
            // own dependencies are pulled in transitively.
            for class in referenced_app_classes(&ir) {
                self.consider_class_init(&class, &mut clinit_classes, &mut queue);
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

        let clinit_symbols = clinit_run_order(&clinit_classes, &lowered);
        Ok(LoweredClosure {
            functions: lowered,
            clinit_symbols,
        })
    }

    /// Schedule `class`'s `<clinit>` for compilation and startup execution if the
    /// class is in the closed world, declares a `<clinit>()V`, and we have not
    /// already scheduled it.
    fn consider_class_init(
        &self,
        class: &str,
        clinit_classes: &mut BTreeSet<String>,
        queue: &mut VecDeque<MethodKey>,
    ) {
        if !self.class_has_clinit(class) || !clinit_classes.insert(class.to_string()) {
            return;
        }
        queue.push_back(MethodKey::new(class, CLINIT_NAME, CLINIT_DESCRIPTOR));
    }

    fn class_has_clinit(&self, class: &str) -> bool {
        self.world.classes.get(class).is_some_and(|class_file| {
            class_file
                .methods
                .iter()
                .any(|method| method.name == CLINIT_NAME && method.descriptor == CLINIT_DESCRIPTOR)
        })
    }
}

/// Order the needed class initializers so that a class runs after every other
/// initialized class it actively uses (dependencies first) — the closest a
/// batch-at-startup scheme gets to JVMS's lazy interleaving. Ties and cycles are
/// broken by sorted class name for determinism. Returns each class's `<clinit>`
/// linkage symbol in run order.
fn clinit_run_order(clinit_classes: &BTreeSet<String>, lowered: &[FunctionIr]) -> Vec<String> {
    let clinit_by_class: BTreeMap<&str, &FunctionIr> = lowered
        .iter()
        .filter_map(|function| {
            let class = function.name.strip_suffix(&format!(".{CLINIT_NAME}"))?;
            (function.descriptor == CLINIT_DESCRIPTOR).then_some((class, function))
        })
        .collect();

    let mut order = Vec::new();
    let mut visited = BTreeSet::new();
    for class in clinit_classes {
        let dotted = class.replace('/', ".");
        visit_clinit(
            &dotted,
            clinit_classes,
            &clinit_by_class,
            &mut visited,
            &mut order,
        );
    }
    order
        .into_iter()
        .map(|dotted| exported_symbol(&format!("{dotted}.{CLINIT_NAME}"), CLINIT_DESCRIPTOR))
        .collect()
}

/// Depth-first post-order over the class-init dependency graph. `class` is the
/// dotted class name; `order` accumulates dotted names dependencies-first. The
/// `visited` guard makes cycles terminate (an arbitrary-but-stable break).
fn visit_clinit(
    class: &str,
    clinit_classes: &BTreeSet<String>,
    clinit_by_class: &BTreeMap<&str, &FunctionIr>,
    visited: &mut BTreeSet<String>,
    order: &mut Vec<String>,
) {
    if !visited.insert(class.to_string()) {
        return;
    }
    if let Some(function) = clinit_by_class.get(class) {
        // Dependencies of this initializer: other initialized classes it uses.
        let mut dependencies: Vec<String> = referenced_app_classes(function)
            .into_iter()
            .map(|referenced| referenced.replace('/', "."))
            .filter(|dotted| dotted != class && clinit_classes.contains(&dotted.replace('.', "/")))
            .collect();
        dependencies.sort();
        dependencies.dedup();
        for dependency in dependencies {
            visit_clinit(&dependency, clinit_classes, clinit_by_class, visited, order);
        }
    }
    order.push(class.to_string());
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

/// The compiled entry must return `int` (printed) or `void` (runs for its
/// println side effects). Reference and other returns have no delivery ABI yet,
/// so they are rejected loudly rather than reaching codegen.
fn require_entry_return_method(function: &FunctionIr) -> Result<()> {
    match &function.return_type {
        IrType::Int | IrType::Void if !function.blocks.is_empty() => Ok(()),
        IrType::Int
        | IrType::Void
        | IrType::Boolean
        | IrType::Byte
        | IrType::Short
        | IrType::Char
        | IrType::Object(_)
        | IrType::Array(_)
        | IrType::Unsupported(_) => bail!(
            "phase=compiler method={} message=compiler entry must return int or void",
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

/// Classes actively used by a function's body: static-call targets, `new`
/// targets, and static-field owners (`getstatic`/`putstatic`, i.e. field access
/// with no receiver). These are exactly the uses that trigger class
/// initialization under JVMS. Names are in JVM internal (slash) form.
fn referenced_app_classes(function: &FunctionIr) -> Vec<String> {
    let mut classes = Vec::new();
    for block in &function.blocks {
        for instr in &block.instrs {
            match instr {
                IrInstr::Call(_, method, _) => classes.push(method.class.clone()),
                IrInstr::NewObject(_, class) => classes.push(class.clone()),
                IrInstr::FieldGet(_, field, None) | IrInstr::FieldPut(field, None, _) => {
                    classes.push(field.class.clone());
                }
                _ => {}
            }
        }
    }
    classes
}
