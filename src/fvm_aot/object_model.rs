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
    fields: BTreeMap<String, FieldSlot>,
}

impl ClassLayout {
    pub(in crate::fvm_aot) fn field(&self, name: &str) -> Option<&FieldSlot> {
        self.fields.get(name)
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
    for field in &class_file.fields {
        if field.access_flags & ACC_STATIC != 0 {
            continue; // static fields live in per-class storage, not the instance
        }
        let (ty, size) = field_type(&field.descriptor, &class_file.this_name)?;
        offset = align_up(offset, size);
        fields.insert(field.name.clone(), FieldSlot { offset, ty });
        offset += size;
    }

    Ok(ClassLayout {
        class_id,
        instance_size: align_up(offset, REFERENCE_BYTES),
        fields,
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
