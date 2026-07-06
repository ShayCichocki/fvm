#![allow(dead_code)]

use super::classfile::{ClassFile, Method};
use super::lower::lower_method_to_ir;
use super::reachability::analyze_main;
use super::{ClassWorld, read_class_world};
use anyhow::Result;
use std::fmt::Write;
use std::path::Path;

pub(super) struct CompilerPipeline {
    world: ClassWorld,
    main_class: String,
}

impl CompilerPipeline {
    pub(super) fn from_jar(jar_path: &Path, main_class: &str) -> Result<Self> {
        Ok(Self::from_world(read_class_world(jar_path)?, main_class))
    }

    pub(super) fn from_world(world: ClassWorld, main_class: &str) -> Self {
        Self {
            world,
            main_class: main_class.replace('.', "/"),
        }
    }

    pub(super) fn run(&self) -> Result<CompilerReport> {
        let reachable = analyze_main(&self.world, &self.main_class)?;
        let mut lowered = Vec::new();
        let mut diagnostics = Vec::new();

        for (class, name, descriptor) in reachable.methods() {
            let Some((class_file, method)) = self.find_method(class, name, descriptor) else {
                diagnostics.push(PipelineDiagnostic::missing_method(class, name, descriptor));
                break;
            };

            match lower_method_to_ir(class_file, method).and_then(|ir| {
                ir.verify()?;
                Ok(ir)
            }) {
                Ok(ir) => lowered.push(LoweredMethodReport {
                    method: method_label(class, name, descriptor),
                    blocks: ir.blocks.len(),
                }),
                Err(err) => {
                    diagnostics.push(PipelineDiagnostic::lowering(
                        class,
                        name,
                        descriptor,
                        format!("{err:#}"),
                    ));
                    break;
                }
            }
        }

        Ok(CompilerReport {
            reachable_text: reachable.render_text(),
            lowered,
            diagnostics,
        })
    }

    fn find_method<'a>(
        &'a self,
        class: &str,
        name: &str,
        descriptor: &str,
    ) -> Option<(&'a ClassFile, &'a Method)> {
        let class_file = self.world.classes.get(class)?;
        let method = class_file
            .methods
            .iter()
            .find(|method| method.name == name && method.descriptor == descriptor)?;
        Some((class_file, method))
    }
}

#[derive(Debug)]
pub(super) struct CompilerReport {
    reachable_text: String,
    lowered: Vec<LoweredMethodReport>,
    diagnostics: Vec<PipelineDiagnostic>,
}

impl CompilerReport {
    pub(super) fn render_text(&self) -> String {
        let mut text = String::new();
        text.push_str("compiler_pipeline:\nreachable:\n");
        text.push_str(&self.reachable_text);
        text.push_str("lowered:\n");
        if self.lowered.is_empty() {
            text.push_str("  <none>\n");
        } else {
            for method in &self.lowered {
                let _ = writeln!(
                    text,
                    "  {} verified blocks={}",
                    method.method, method.blocks
                );
            }
        }
        text.push_str("diagnostics:\n");
        if self.diagnostics.is_empty() {
            text.push_str("  <none>\n");
        } else {
            for diagnostic in &self.diagnostics {
                let _ = writeln!(text, "  {diagnostic}");
            }
        }
        text
    }
}

#[derive(Debug)]
struct LoweredMethodReport {
    method: String,
    blocks: usize,
}

#[derive(Debug)]
struct PipelineDiagnostic {
    phase: &'static str,
    method: String,
    message: String,
}

impl PipelineDiagnostic {
    fn missing_method(class: &str, name: &str, descriptor: &str) -> Self {
        Self {
            phase: "resolve",
            method: method_label(class, name, descriptor),
            message: "reachable method was not found in class world".to_string(),
        }
    }

    fn lowering(class: &str, name: &str, descriptor: &str, message: String) -> Self {
        Self {
            phase: "lower",
            method: method_label(class, name, descriptor),
            message,
        }
    }
}

impl std::fmt::Display for PipelineDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "phase={} method={} message={}",
            self.phase, self.method, self.message
        )
    }
}

fn method_label(class: &str, name: &str, descriptor: &str) -> String {
    format!("{class}.{name}{descriptor}")
}
