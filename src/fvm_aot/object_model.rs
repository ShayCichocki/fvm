// allow: dead_code — the object model is only reached through the compiler
// pipeline, which is not yet wired into production `main` (P4.4); the bin target
// therefore can't see these reads.
#![allow(dead_code)]

//! Closed-world object layout: per-class field offsets, instance sizes, and
//! numeric class ids. Superclass fields come first (JVMS layout order); the
//! object header holds the class id.
//!
//! Early profile (Phase 2 start): an 8-byte header (`class_id: u32` + reserved),
//! int-like fields at 4 bytes and references at 8, and single-level inheritance
//! from `java/lang/Object` only. Deeper hierarchies, statics-in-layout, and the
//! long/float/double field widths arrive with later Phase 2 items.

use super::classfile::ClassFile;
use super::ir::IrType;
use anyhow::{Result, bail};
use std::collections::{BTreeMap, HashMap};

/// Object header size in bytes: `class_id: u32` at offset 0, plus reserved space
/// (identity hash / flags / GC bits land here in later phases).
pub(in crate::fvm_aot) const OBJECT_HEADER_BYTES: u32 = 8;
/// Byte offset of the class id within the header.
pub(in crate::fvm_aot) const CLASS_ID_OFFSET: u32 = 0;
/// A reference field/slot is a raw pointer.
pub(in crate::fvm_aot) const REFERENCE_BYTES: u32 = 8;
const INT_BYTES: u32 = 4;

/// Array layout: the shared object header, then the `int` length, then the
/// element storage (8-aligned so reference elements are naturally aligned).
pub(in crate::fvm_aot) const ARRAY_LENGTH_OFFSET: u32 = OBJECT_HEADER_BYTES;
pub(in crate::fvm_aot) const ARRAY_ELEMENTS_OFFSET: u32 = 16;
const ACC_STATIC: u16 = 0x0008;

pub(in crate::fvm_aot) struct FieldSlot {
    pub(in crate::fvm_aot) offset: u32,
    pub(in crate::fvm_aot) ty: IrType,
}

pub(in crate::fvm_aot) struct ClassLayout {
    pub(in crate::fvm_aot) class_id: u32,
    pub(in crate::fvm_aot) instance_size: u32,
    /// Total bytes of per-class static storage (0 when the class declares no
    /// supported static fields). The storage block is a zero-initialized data
    /// object addressed by `static_field` offsets.
    pub(in crate::fvm_aot) static_size: u32,
    /// The superclass's internal name (`None` only for `java/lang/Object`, which
    /// is not itself a modeled class). Drives `instanceof`/`checkcast` subtype
    /// resolution.
    super_name: Option<String>,
    is_interface: bool,
    fields: BTreeMap<String, FieldSlot>,
    static_fields: BTreeMap<String, FieldSlot>,
}

impl ClassLayout {
    pub(in crate::fvm_aot) fn field(&self, name: &str) -> Option<&FieldSlot> {
        self.fields.get(name)
    }

    /// Offset + type of a static field within this class's static storage block.
    pub(in crate::fvm_aot) fn static_field(&self, name: &str) -> Option<&FieldSlot> {
        self.static_fields.get(name)
    }
}

pub(in crate::fvm_aot) struct ObjectModel {
    classes: BTreeMap<String, ClassLayout>,
}

impl ObjectModel {
    /// Compute layouts for every class in the closed world. Class ids are
    /// assigned in sorted-name order starting at 1 (0 is reserved for "no
    /// class"), so the numbering is deterministic across builds.
    pub(in crate::fvm_aot) fn from_classes(classes: &HashMap<String, ClassFile>) -> Result<Self> {
        let mut names: Vec<&String> = classes.keys().collect();
        names.sort();

        let mut layouts = BTreeMap::new();
        for (index, name) in names.into_iter().enumerate() {
            let class_id = u32::try_from(index + 1)
                .map_err(|_| anyhow::anyhow!("fvm-aot object model exceeded u32 class ids"))?;
            let class_file = &classes[name];
            layouts.insert(name.clone(), layout_class(class_file, class_id)?);
        }
        Ok(Self { classes: layouts })
    }

    /// A model with no classes — for codegen paths that never touch objects.
    pub(in crate::fvm_aot) fn empty() -> Self {
        Self {
            classes: BTreeMap::new(),
        }
    }

    pub(in crate::fvm_aot) fn class(&self, name: &str) -> Option<&ClassLayout> {
        self.classes.get(name)
    }

    /// Every class layout, in deterministic (sorted-name) order. Codegen walks
    /// this to allocate static storage for classes that declare static fields.
    pub(in crate::fvm_aot) fn classes(&self) -> impl Iterator<Item = (&String, &ClassLayout)> {
        self.classes.iter()
    }

    /// Resolve an `instanceof`/`checkcast` target class to how the runtime type
    /// check should be performed against the closed-world hierarchy.
    pub(in crate::fvm_aot) fn subtype_check(&self, target: &str) -> SubtypeCheck {
        // Every non-null reference is an `Object`; no class-id comparison needed.
        if target == "java/lang/Object" {
            return SubtypeCheck::AnyReference;
        }
        match self.classes.get(target) {
            None => SubtypeCheck::Unsupported(format!(
                "instanceof/checkcast target {target} is not a closed-world class (JDK classes, arrays, and strings are not modeled yet)"
            )),
            Some(layout) if layout.is_interface => SubtypeCheck::Unsupported(format!(
                "instanceof/checkcast target {target} is an interface; interface type checks need itable metadata"
            )),
            Some(_) => {
                // A class D matches `target` iff `target` is D itself or an
                // ancestor on D's superclass chain. With only single inheritance
                // modeled, the chain is short; this generalizes automatically as
                // deeper hierarchies become supported.
                let mut ids: Vec<u32> = self
                    .classes
                    .iter()
                    .filter(|(_, layout)| !layout.is_interface)
                    .filter(|(name, _)| self.is_subclass_of(name, target))
                    .map(|(_, layout)| layout.class_id)
                    .collect();
                ids.sort_unstable();
                SubtypeCheck::ClassIds(ids)
            }
        }
    }

    /// Whether `class` is `target` or transitively extends it, walking the
    /// modeled superclass chain (terminating at `java/lang/Object`, which is not
    /// itself a modeled class).
    fn is_subclass_of(&self, class: &str, target: &str) -> bool {
        let mut current = class;
        loop {
            if current == target {
                return true;
            }
            match self.classes.get(current).and_then(|l| l.super_name.as_deref()) {
                Some(parent) => current = parent,
                None => return false,
            }
        }
    }
}

/// How a runtime `instanceof`/`checkcast` against a target type is performed.
pub(in crate::fvm_aot) enum SubtypeCheck {
    /// The target is `java/lang/Object`: any non-null reference matches.
    AnyReference,
    /// The object matches iff its header class id is one of these (the target
    /// class and its modeled subclasses).
    ClassIds(Vec<u32>),
    /// The target type is not modeled yet; reject loudly with this reason.
    Unsupported(String),
}

fn layout_class(class_file: &ClassFile, class_id: u32) -> Result<ClassLayout> {
    match class_file.super_name.as_deref() {
        None | Some("java/lang/Object") => {}
        Some(other) => bail!(
            "fvm-aot object model cannot lay out {}: it extends {other}, and only direct subclasses of java/lang/Object are supported; required feature: inheritance field layout; planned milestone: object-model-inheritance",
            class_file.this_name
        ),
    }

    let mut offset = OBJECT_HEADER_BYTES;
    let mut fields = BTreeMap::new();
    let mut static_offset = 0;
    let mut static_fields = BTreeMap::new();
    for field in &class_file.fields {
        if field.access_flags & ACC_STATIC != 0 {
            // Static fields live in a per-class storage block, not the instance.
            // Unsupported widths (long/float/double, P2.7) are omitted rather
            // than failing the whole layout: a class with an unused wide static
            // must still compile. Accessing an omitted field fails loudly in
            // codegen (`static_field` returns `None`).
            if let Ok((ty, size)) = field_type(&field.descriptor, &class_file.this_name) {
                static_offset = align_up(static_offset, size);
                static_fields.insert(
                    field.name.clone(),
                    FieldSlot {
                        offset: static_offset,
                        ty,
                    },
                );
                static_offset += size;
            }
            continue;
        }
        let (ty, size) = field_type(&field.descriptor, &class_file.this_name)?;
        offset = align_up(offset, size);
        fields.insert(field.name.clone(), FieldSlot { offset, ty });
        offset += size;
    }

    Ok(ClassLayout {
        class_id,
        instance_size: align_up(offset, REFERENCE_BYTES),
        static_size: align_up(static_offset, REFERENCE_BYTES),
        super_name: class_file.super_name.clone(),
        is_interface: class_file.is_interface(),
        fields,
        static_fields,
    })
}

/// Field static type and storage width. Only int-like and reference fields are
/// supported today; long/float/double widths arrive with P2.7.
fn field_type(descriptor: &str, class_name: &str) -> Result<(IrType, u32)> {
    match descriptor {
        "I" | "B" | "S" => Ok((IrType::Int, INT_BYTES)),
        "Z" => Ok((IrType::Boolean, INT_BYTES)),
        "C" => Ok((IrType::Char, INT_BYTES)),
        reference if reference.starts_with('L') && reference.ends_with(';') => Ok((
            IrType::Object(reference[1..reference.len() - 1].to_string()),
            REFERENCE_BYTES,
        )),
        array if array.starts_with('[') => {
            let (component, _) = field_type(&array[1..], class_name)?;
            Ok((IrType::Array(Box::new(component)), REFERENCE_BYTES))
        }
        other => bail!(
            "fvm-aot object model cannot lay out field of type `{other}` in {class_name}; required feature: wide primitive fields; planned milestone: primitive-completeness"
        ),
    }
}

fn align_up(offset: u32, alignment: u32) -> u32 {
    offset.div_ceil(alignment) * alignment
}
