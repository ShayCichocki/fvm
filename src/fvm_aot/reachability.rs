#![allow(dead_code)]

use super::ClassWorld;
use super::classfile::{ClassFile, Method, ResolvedMember};
use super::types::{
    JvmType, array_component_descriptor, descriptor_to_class, parse_method_descriptor,
};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeSet, VecDeque};
use std::fmt::Write;

#[derive(Debug, Default)]
pub(super) struct ReachabilityGraph {
    classes: BTreeSet<String>,
    methods: BTreeSet<MethodNode>,
    fields: BTreeSet<FieldNode>,
}

impl ReachabilityGraph {
    pub(super) fn methods(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.methods
            .iter()
            .map(|method| (method.0.as_str(), method.1.as_str(), method.2.as_str()))
    }

    pub(super) fn render_text(&self) -> String {
        let mut text = String::new();
        text.push_str("classes:\n");
        for class in &self.classes {
            let _ = writeln!(text, "  {class}");
        }
        text.push_str("methods:\n");
        for method in &self.methods {
            let _ = writeln!(text, "  {}.{}{}", method.0, method.1, method.2);
        }
        text.push_str("fields:\n");
        for field in &self.fields {
            let _ = writeln!(text, "  {}.{}:{}", field.0, field.1, field.2);
        }
        text
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct MethodNode(String, String, String);

impl MethodNode {
    fn new(class: &str, name: &str, descriptor: &str) -> Self {
        Self(class.to_string(), name.to_string(), descriptor.to_string())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FieldNode(String, String, String);

impl FieldNode {
    fn from_member(field: &ResolvedMember) -> Self {
        Self(
            field.class.clone(),
            field.name.clone(),
            field.descriptor.clone(),
        )
    }
}

pub(super) fn analyze_main(world: &ClassWorld, main_class: &str) -> Result<ReachabilityGraph> {
    let mut analyzer = ReachabilityAnalyzer {
        world,
        graph: ReachabilityGraph::default(),
        queue: VecDeque::new(),
    };
    analyzer.enqueue_method(main_class, "main", "([Ljava/lang/String;)V")?;
    analyzer.run()
}

struct ReachabilityAnalyzer<'a> {
    world: &'a ClassWorld,
    graph: ReachabilityGraph,
    queue: VecDeque<MethodNode>,
}

impl ReachabilityAnalyzer<'_> {
    fn run(mut self) -> Result<ReachabilityGraph> {
        while let Some(node) = self.queue.pop_front() {
            let class_file = self.class(&node.0)?.clone();
            let method = find_method(&class_file, &node.1, &node.2)?.clone();
            self.scan_method(&class_file, &method).with_context(|| {
                format!(
                    "fvm-aot reachability error in {}.{}{}",
                    node.0, node.1, node.2
                )
            })?;
        }
        Ok(self.graph)
    }

    fn enqueue_method(&mut self, class: &str, name: &str, descriptor: &str) -> Result<()> {
        let class_file = self.class(class)?;
        let method = find_method(class_file, name, descriptor)?;
        if method.access_flags & 0x0008 == 0 {
            bail!("fvm-aot reachability method {class}.{name}{descriptor} is not static");
        }
        self.mark_class(class)?;
        self.mark_descriptor(descriptor)?;

        let node = MethodNode::new(class, name, descriptor);
        if self.graph.methods.insert(node.clone()) {
            self.queue.push_back(node);
        }
        Ok(())
    }

    fn mark_class(&mut self, class: &str) -> Result<()> {
        if !self.world.classes.contains_key(class) {
            return Ok(());
        }
        if self.graph.classes.insert(class.to_string()) {
            self.enqueue_clinit(class)?;
            let class_file = self.class(class)?;
            let fields = class_file.fields.clone();
            for field in fields {
                self.mark_descriptor(&field.descriptor)?;
            }
        }
        Ok(())
    }

    fn enqueue_clinit(&mut self, class: &str) -> Result<()> {
        let class_file = self.class(class)?;
        if class_file
            .methods
            .iter()
            .any(|method| method.name == "<clinit>" && method.descriptor == "()V")
        {
            self.enqueue_method(class, "<clinit>", "()V")?;
        }
        Ok(())
    }

    fn scan_method(&mut self, class_file: &ClassFile, method: &Method) -> Result<()> {
        self.mark_descriptor(&method.descriptor)?;
        let Some(code) = method.code.as_ref() else {
            return Ok(());
        };
        let mut pc = 0_usize;
        while pc < code.bytes.len() {
            let opcode = read_u8(&code.bytes, &mut pc)?;
            match opcode {
                0x12 => {
                    let _ = read_u8(&code.bytes, &mut pc)?;
                }
                0x13 | 0x14 | 0x84 | 0x99..=0xa8 | 0xc6 | 0xc7 => {
                    let _ = read_u16(&code.bytes, &mut pc)?;
                }
                0xb2..=0xb5 => {
                    let field = class_file.field_ref(read_u16(&code.bytes, &mut pc)?)?;
                    self.mark_field(&field)?;
                }
                0xb8 => {
                    let method_ref = class_file.method_ref(read_u16(&code.bytes, &mut pc)?)?;
                    if method_ref.class == "java/lang/Class" && method_ref.name == "forName" {
                        bail!(
                            "fvm-aot unsupported feature: dynamic class loading/Class.forName; required feature: closed-world reflection metadata; planned milestone: reflection-and-metadata"
                        );
                    }
                    if self.world.classes.contains_key(&method_ref.class) {
                        self.enqueue_method(
                            &method_ref.class,
                            &method_ref.name,
                            &method_ref.descriptor,
                        )?;
                    }
                }
                0xbb | 0xbd | 0xc0 | 0xc1 => {
                    let class = class_file.class_name(read_u16(&code.bytes, &mut pc)?)?;
                    self.mark_class(&class)?;
                }
                _ => skip_fixed_width(opcode, &code.bytes, &mut pc)?,
            }
        }
        Ok(())
    }

    fn mark_field(&mut self, field: &ResolvedMember) -> Result<()> {
        self.mark_descriptor(&field.descriptor)?;
        self.mark_class(&field.class)?;
        if self.world.classes.contains_key(&field.class) {
            self.graph.fields.insert(FieldNode::from_member(field));
        }
        Ok(())
    }

    fn mark_descriptor(&mut self, descriptor: &str) -> Result<()> {
        if descriptor.starts_with('(') {
            let (params, return_type) = parse_method_descriptor(descriptor)?;
            for ty in params.iter().chain(std::iter::once(&return_type)) {
                self.mark_type(ty)?;
            }
            return Ok(());
        }
        if descriptor.starts_with('[') {
            self.mark_descriptor(array_component_descriptor(descriptor)?)?;
        } else if descriptor.starts_with('L') && descriptor.ends_with(';') {
            self.mark_class(descriptor_to_class(descriptor)?)?;
        }
        Ok(())
    }

    fn mark_type(&mut self, ty: &JvmType) -> Result<()> {
        match ty {
            JvmType::Object(class) => self.mark_class(class),
            JvmType::Array(descriptor) => self.mark_descriptor(descriptor),
            JvmType::Int
            | JvmType::Boolean
            | JvmType::Char
            | JvmType::String
            | JvmType::Void
            | JvmType::Unsupported => Ok(()),
        }
    }

    fn class(&self, name: &str) -> Result<&ClassFile> {
        self.world
            .classes
            .get(name)
            .with_context(|| format!("class `{name}` not found in closed-world JAR"))
    }
}

fn find_method<'a>(class_file: &'a ClassFile, name: &str, descriptor: &str) -> Result<&'a Method> {
    class_file
        .methods
        .iter()
        .find(|method| method.name == name && method.descriptor == descriptor)
        .with_context(|| format!("method {name}{descriptor} not found"))
}

fn skip_fixed_width(opcode: u8, code: &[u8], pc: &mut usize) -> Result<()> {
    let width = match opcode {
        0x10 | 0x15..=0x19 | 0x36..=0x3a | 0xa9 | 0xbc => 1,
        0x11 => 2,
        0x00..=0x0f
        | 0x1a..=0x35
        | 0x3b..=0x83
        | 0x85..=0x98
        | 0xac..=0xb1
        | 0xbe..=0xc3
        | 0xca => 0,
        0xc4 => bail!("fvm-aot reachability does not support wide bytecode yet"),
        _ => bail!("fvm-aot reachability cannot decode opcode 0x{opcode:02x}"),
    };
    skip_bytes(code, pc, width)
}

fn skip_bytes(code: &[u8], pc: &mut usize, width: usize) -> Result<()> {
    for _ in 0..width {
        let _ = read_u8(code, pc)?;
    }
    Ok(())
}

fn read_u8(code: &[u8], pc: &mut usize) -> Result<u8> {
    if *pc >= code.len() {
        bail!("truncated bytecode at pc {pc}");
    }
    let value = code[*pc];
    *pc += 1;
    Ok(value)
}

fn read_u16(code: &[u8], pc: &mut usize) -> Result<u16> {
    let high = read_u8(code, pc)?;
    let low = read_u8(code, pc)?;
    Ok(u16::from_be_bytes([high, low]))
}
