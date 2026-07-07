use anyhow::{Context, Result, bail};

#[derive(Clone, Debug)]
pub(super) struct ClassFile {
    pub(super) this_name: String,
    /// The superclass's internal name. `None` only for `java/lang/Object`, whose
    /// `super_class` constant-pool index is 0. Read by the object-model layout
    /// pass, which the bin's dead-code analysis can't see yet (P4.4).
    #[allow(dead_code)]
    pub(super) super_name: Option<String>,
    #[allow(dead_code)]
    pub(super) access_flags: u16,
    constants: Vec<Option<Constant>>,
    pub(super) fields: Vec<Field>,
    pub(super) methods: Vec<Method>,
    bootstrap_methods: Vec<BootstrapMethod>,
}

#[derive(Clone, Debug)]
pub(super) struct Field {
    pub(super) access_flags: u16,
    pub(super) name: String,
    pub(super) descriptor: String,
    pub(super) constant_value_index: Option<u16>,
}

#[derive(Clone, Debug)]
pub(super) struct Method {
    pub(super) access_flags: u16,
    pub(super) name: String,
    pub(super) descriptor: String,
    pub(super) code: Option<Code>,
}

#[derive(Clone, Debug)]
pub(super) struct Code {
    pub(super) max_locals: u16,
    pub(super) bytes: Vec<u8>,
    /// Whether the method declares any exception handlers. The handler ranges
    /// themselves are not modeled yet; lowering rejects such methods loudly
    /// (Phase 5) rather than silently dropping the handlers as dead code. Only
    /// the compiler path reads this, which is not yet wired into production
    /// `main` (P4.4) — so the bin's dead-code pass can't see the read.
    #[allow(dead_code)]
    pub(super) has_exception_table: bool,
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedMember {
    pub(super) class: String,
    pub(super) name: String,
    pub(super) descriptor: String,
}

#[derive(Clone, Debug)]
pub(super) struct ResolvedInvokeDynamic {
    pub(super) bootstrap_method_attr_index: u16,
    pub(super) name: String,
    pub(super) descriptor: String,
}

#[derive(Clone, Debug)]
pub(super) struct BootstrapMethod {
    pub(super) method_ref: u16,
    pub(super) arguments: Vec<u16>,
}

#[derive(Clone, Debug)]
pub(super) enum Constant {
    Utf8(String),
    Integer(i32),
    Float,
    Long,
    Double,
    Class {
        name_index: u16,
    },
    String {
        string_index: u16,
    },
    Fieldref {
        class_index: u16,
        name_and_type_index: u16,
    },
    Methodref {
        class_index: u16,
        name_and_type_index: u16,
    },
    InterfaceMethodref {
        class_index: u16,
        name_and_type_index: u16,
    },
    NameAndType {
        name_index: u16,
        descriptor_index: u16,
    },
    MethodHandle {
        reference_kind: u8,
        reference_index: u16,
    },
    MethodType,
    Dynamic,
    InvokeDynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    Module,
    Package,
}

impl ClassFile {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let magic = reader.u4()?;
        if magic != 0xcafebabe {
            bail!("invalid Java classfile magic 0x{magic:08x}");
        }
        let _minor = reader.u2()?;
        let major = reader.u2()?;
        if major > 69 {
            bail!("fvm-aot supports classfile versions up to Java 25 for now, got major {major}");
        }

        let constant_pool_count = reader.u2()? as usize;
        let mut constants = vec![None];
        let mut index = 1_usize;
        while index < constant_pool_count {
            let tag = reader.u1()?;
            let constant = match tag {
                1 => {
                    let len = reader.u2()? as usize;
                    let bytes = reader.bytes(len)?;
                    Constant::Utf8(decode_modified_utf8(bytes)?)
                }
                3 => Constant::Integer(reader.u4()? as i32),
                4 => {
                    let _ = reader.u4()?;
                    Constant::Float
                }
                5 => {
                    let _ = reader.u4()?;
                    let _ = reader.u4()?;
                    constants.push(Some(Constant::Long));
                    constants.push(None);
                    index += 2;
                    continue;
                }
                6 => {
                    let _ = reader.u4()?;
                    let _ = reader.u4()?;
                    constants.push(Some(Constant::Double));
                    constants.push(None);
                    index += 2;
                    continue;
                }
                7 => Constant::Class {
                    name_index: reader.u2()?,
                },
                8 => Constant::String {
                    string_index: reader.u2()?,
                },
                9 => Constant::Fieldref {
                    class_index: reader.u2()?,
                    name_and_type_index: reader.u2()?,
                },
                10 => Constant::Methodref {
                    class_index: reader.u2()?,
                    name_and_type_index: reader.u2()?,
                },
                11 => Constant::InterfaceMethodref {
                    class_index: reader.u2()?,
                    name_and_type_index: reader.u2()?,
                },
                12 => Constant::NameAndType {
                    name_index: reader.u2()?,
                    descriptor_index: reader.u2()?,
                },
                15 => Constant::MethodHandle {
                    reference_kind: reader.u1()?,
                    reference_index: reader.u2()?,
                },
                16 => {
                    let _ = reader.u2()?;
                    Constant::MethodType
                }
                17 => {
                    let _ = reader.u2()?;
                    let _ = reader.u2()?;
                    Constant::Dynamic
                }
                18 => Constant::InvokeDynamic {
                    bootstrap_method_attr_index: reader.u2()?,
                    name_and_type_index: reader.u2()?,
                },
                19 => {
                    let _ = reader.u2()?;
                    Constant::Module
                }
                20 => {
                    let _ = reader.u2()?;
                    Constant::Package
                }
                _ => bail!("unsupported constant pool tag {tag}"),
            };
            constants.push(Some(constant));
            index += 1;
        }

        let access_flags = reader.u2()?;
        let this_class = reader.u2()?;
        let super_class = reader.u2()?;

        skip_table(&mut reader, 2)?;
        let fields = parse_fields(&mut reader, &constants)?;
        let methods = parse_methods(&mut reader, &constants)?;
        let bootstrap_methods = parse_class_attributes(&mut reader, &constants)?;
        reader.finish()?;

        let this_name = match constants
            .get(this_class as usize)
            .and_then(|constant| constant.as_ref())
        {
            Some(Constant::Class { name_index }) => utf8(&constants, *name_index)?.to_string(),
            _ => bail!("classfile this_class did not point at a class constant"),
        };

        // `super_class` is 0 only for `java/lang/Object`; every other class names
        // a superclass constant.
        let super_name = if super_class == 0 {
            None
        } else {
            match constants
                .get(super_class as usize)
                .and_then(|constant| constant.as_ref())
            {
                Some(Constant::Class { name_index }) => {
                    Some(utf8(&constants, *name_index)?.to_string())
                }
                _ => bail!("classfile super_class did not point at a class constant"),
            }
        };

        Ok(Self {
            this_name,
            super_name,
            access_flags,
            constants,
            fields,
            methods,
            bootstrap_methods,
        })
    }

    /// Whether this class file is an interface (`ACC_INTERFACE`). Interfaces are
    /// not modeled by the object layout, so `instanceof`/`checkcast` against them
    /// is rejected until interface metadata lands (P3.3).
    pub(super) fn is_interface(&self) -> bool {
        const ACC_INTERFACE: u16 = 0x0200;
        self.access_flags & ACC_INTERFACE != 0
    }

    pub(super) fn constant(&self, index: u16) -> Result<&Constant> {
        self.constants
            .get(index as usize)
            .and_then(|constant| constant.as_ref())
            .with_context(|| format!("invalid constant pool index {index}"))
    }

    fn utf8(&self, index: u16) -> Result<&str> {
        match self.constant(index)? {
            Constant::Utf8(value) => Ok(value),
            other => bail!("constant {index} is not Utf8: {other:?}"),
        }
    }

    pub(super) fn class_name(&self, index: u16) -> Result<String> {
        match self.constant(index)? {
            Constant::Class { name_index } => Ok(self.utf8(*name_index)?.to_string()),
            other => bail!("constant {index} is not Class: {other:?}"),
        }
    }

    fn name_and_type(&self, index: u16) -> Result<(String, String)> {
        match self.constant(index)? {
            Constant::NameAndType {
                name_index,
                descriptor_index,
            } => Ok((
                self.utf8(*name_index)?.to_string(),
                self.utf8(*descriptor_index)?.to_string(),
            )),
            other => bail!("constant {index} is not NameAndType: {other:?}"),
        }
    }

    pub(super) fn field_ref(&self, index: u16) -> Result<ResolvedMember> {
        match self.constant(index)? {
            Constant::Fieldref {
                class_index,
                name_and_type_index,
            } => {
                let (name, descriptor) = self.name_and_type(*name_and_type_index)?;
                Ok(ResolvedMember {
                    class: self.class_name(*class_index)?,
                    name,
                    descriptor,
                })
            }
            other => bail!("constant {index} is not Fieldref: {other:?}"),
        }
    }

    pub(super) fn method_ref(&self, index: u16) -> Result<ResolvedMember> {
        match self.constant(index)? {
            Constant::Methodref {
                class_index,
                name_and_type_index,
            }
            | Constant::InterfaceMethodref {
                class_index,
                name_and_type_index,
            } => {
                let (name, descriptor) = self.name_and_type(*name_and_type_index)?;
                Ok(ResolvedMember {
                    class: self.class_name(*class_index)?,
                    name,
                    descriptor,
                })
            }
            other => bail!("constant {index} is not Methodref: {other:?}"),
        }
    }

    pub(super) fn invoke_dynamic(&self, index: u16) -> Result<ResolvedInvokeDynamic> {
        match self.constant(index)? {
            Constant::InvokeDynamic {
                bootstrap_method_attr_index,
                name_and_type_index,
            } => {
                let (name, descriptor) = self.name_and_type(*name_and_type_index)?;
                Ok(ResolvedInvokeDynamic {
                    bootstrap_method_attr_index: *bootstrap_method_attr_index,
                    name,
                    descriptor,
                })
            }
            other => bail!("constant {index} is not InvokeDynamic: {other:?}"),
        }
    }

    pub(super) fn bootstrap_method(&self, index: u16) -> Result<&BootstrapMethod> {
        self.bootstrap_methods
            .get(index as usize)
            .with_context(|| format!("invalid bootstrap method index {index}"))
    }

    pub(super) fn method_handle_ref(&self, index: u16) -> Result<ResolvedMember> {
        match self.constant(index)? {
            Constant::MethodHandle {
                reference_kind,
                reference_index,
            } => {
                if *reference_kind != 6 {
                    bail!(
                        "fvm-aot only supports REF_invokeStatic method handles, got kind {reference_kind}"
                    );
                }
                self.method_ref(*reference_index)
            }
            other => bail!("constant {index} is not MethodHandle: {other:?}"),
        }
    }

    pub(super) fn string_constant(&self, index: u16) -> Result<String> {
        match self.constant(index)? {
            Constant::String { string_index } => Ok(self.utf8(*string_index)?.to_string()),
            other => bail!("constant {index} is not String: {other:?}"),
        }
    }

    pub(super) fn int_constant(&self, index: u16) -> Result<i32> {
        match self.constant(index)? {
            Constant::Integer(value) => Ok(*value),
            other => bail!("constant {index} is not Integer: {other:?}"),
        }
    }
}

fn parse_methods(reader: &mut Reader<'_>, constants: &[Option<Constant>]) -> Result<Vec<Method>> {
    let count = reader.u2()?;
    let mut methods = Vec::new();
    for _ in 0..count {
        let access_flags = reader.u2()?;
        let name_index = reader.u2()?;
        let descriptor_index = reader.u2()?;
        let name = utf8(constants, name_index)?.to_string();
        let descriptor = utf8(constants, descriptor_index)?.to_string();
        let attribute_count = reader.u2()?;
        let mut code = None;
        for _ in 0..attribute_count {
            let attribute_name_index = reader.u2()?;
            let attribute_name = utf8(constants, attribute_name_index)?;
            let attribute_length = reader.u4()? as usize;
            if attribute_name == "Code" {
                let mut code_reader = Reader::new(reader.bytes(attribute_length)?);
                let _max_stack = code_reader.u2()?;
                let max_locals = code_reader.u2()?;
                let code_length = code_reader.u4()? as usize;
                let bytes = code_reader.bytes(code_length)?.to_vec();
                let exception_table_length = code_reader.u2()? as usize;
                code_reader.skip(exception_table_length * 8)?;
                code = Some(Code {
                    max_locals,
                    bytes,
                    has_exception_table: exception_table_length > 0,
                });
                skip_attributes(&mut code_reader)?;
                code_reader.finish()?;
            } else {
                reader.skip(attribute_length)?;
            }
        }
        methods.push(Method {
            access_flags,
            name,
            descriptor,
            code,
        });
    }
    Ok(methods)
}

fn parse_fields(reader: &mut Reader<'_>, constants: &[Option<Constant>]) -> Result<Vec<Field>> {
    let count = reader.u2()?;
    let mut fields = Vec::new();
    for _ in 0..count {
        let access_flags = reader.u2()?;
        let name_index = reader.u2()?;
        let descriptor_index = reader.u2()?;
        let name = utf8(constants, name_index)?.to_string();
        let descriptor = utf8(constants, descriptor_index)?.to_string();
        let attribute_count = reader.u2()?;
        let mut constant_value_index = None;
        for _ in 0..attribute_count {
            let attribute_name_index = reader.u2()?;
            let attribute_name = utf8(constants, attribute_name_index)?;
            let attribute_length = reader.u4()? as usize;
            if attribute_name == "ConstantValue" {
                if attribute_length != 2 {
                    bail!("invalid ConstantValue attribute length {attribute_length}");
                }
                constant_value_index = Some(reader.u2()?);
            } else {
                reader.skip(attribute_length)?;
            }
        }
        fields.push(Field {
            access_flags,
            name,
            descriptor,
            constant_value_index,
        });
    }
    Ok(fields)
}

fn parse_class_attributes(
    reader: &mut Reader<'_>,
    constants: &[Option<Constant>],
) -> Result<Vec<BootstrapMethod>> {
    let count = reader.u2()?;
    let mut bootstrap_methods = Vec::new();
    for _ in 0..count {
        let attribute_name_index = reader.u2()?;
        let attribute_name = utf8(constants, attribute_name_index)?;
        let attribute_length = reader.u4()? as usize;
        if attribute_name == "BootstrapMethods" {
            let mut attribute_reader = Reader::new(reader.bytes(attribute_length)?);
            let count = attribute_reader.u2()?;
            for _ in 0..count {
                let method_ref = attribute_reader.u2()?;
                let argument_count = attribute_reader.u2()?;
                let mut arguments = Vec::with_capacity(argument_count as usize);
                for _ in 0..argument_count {
                    arguments.push(attribute_reader.u2()?);
                }
                bootstrap_methods.push(BootstrapMethod {
                    method_ref,
                    arguments,
                });
            }
            attribute_reader.finish()?;
        } else {
            reader.skip(attribute_length)?;
        }
    }
    Ok(bootstrap_methods)
}

fn skip_attributes(reader: &mut Reader<'_>) -> Result<()> {
    let count = reader.u2()?;
    for _ in 0..count {
        let _name_index = reader.u2()?;
        let length = reader.u4()? as usize;
        reader.skip(length)?;
    }
    Ok(())
}

fn skip_table(reader: &mut Reader<'_>, entry_size: usize) -> Result<()> {
    let count = reader.u2()? as usize;
    reader.skip(count * entry_size)
}

fn utf8(constants: &[Option<Constant>], index: u16) -> Result<&str> {
    match constants
        .get(index as usize)
        .and_then(|constant| constant.as_ref())
    {
        Some(Constant::Utf8(value)) => Ok(value),
        Some(other) => bail!("constant {index} is not Utf8: {other:?}"),
        None => bail!("invalid constant pool index {index}"),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u1(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u2(&mut self) -> Result<u16> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn u4(&mut self) -> Result<u32> {
        let bytes = self.bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("classfile offset overflow")?;
        if end > self.bytes.len() {
            bail!("truncated classfile at offset {}", self.offset);
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn skip(&mut self, len: usize) -> Result<()> {
        let _ = self.bytes(len)?;
        Ok(())
    }

    fn finish(&self) -> Result<()> {
        if self.offset != self.bytes.len() {
            bail!(
                "classfile parser left {} trailing bytes",
                self.bytes.len() - self.offset
            );
        }
        Ok(())
    }
}

/// Decode a JVM `CONSTANT_Utf8` byte string, which is encoded in *Modified
/// UTF-8* (JVMS §4.4.7), a.k.a. CESU-8.
///
/// Modified UTF-8 differs from standard UTF-8 in two ways that
/// `String::from_utf8_lossy` silently corrupts:
///   - the NUL code point (U+0000) is encoded as the two bytes `0xC0 0x80`;
///   - supplementary code points (U+10000..) are encoded as a UTF-16 surrogate
///     pair, each surrogate emitted as a 3-byte sequence — there is no 4-byte
///     form.
///
/// We decode the raw bytes into UTF-16 code units, then materialize a Rust
/// `String`. Every valid code point — NUL, BMP, and paired-surrogate
/// supplementary characters — round-trips faithfully, which is exactly the
/// UTF-16-faithful model the runtime string work (Phase 3) builds on. Unpaired
/// surrogates (representable only via `\uD800`-style source escapes and absent
/// from any javac-emitted name/descriptor) become U+FFFD, the one residual
/// lossy case; Phase 3's dedicated `char[]` model will preserve them.
fn decode_modified_utf8(bytes: &[u8]) -> Result<String> {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        if b0 & 0x80 == 0 {
            units.push(u16::from(b0));
            i += 1;
        } else if b0 & 0xE0 == 0xC0 {
            let b1 = continuation_byte(bytes, i + 1)?;
            units.push((u16::from(b0 & 0x1F) << 6) | u16::from(b1 & 0x3F));
            i += 2;
        } else if b0 & 0xF0 == 0xE0 {
            let b1 = continuation_byte(bytes, i + 1)?;
            let b2 = continuation_byte(bytes, i + 2)?;
            units.push(
                (u16::from(b0 & 0x0F) << 12) | (u16::from(b1 & 0x3F) << 6) | u16::from(b2 & 0x3F),
            );
            i += 3;
        } else {
            bail!("invalid modified-UTF-8 leading byte 0x{b0:02x} at offset {i}");
        }
    }
    Ok(String::from_utf16_lossy(&units))
}

fn continuation_byte(bytes: &[u8], index: usize) -> Result<u8> {
    let byte = *bytes
        .get(index)
        .context("truncated modified-UTF-8 multibyte sequence")?;
    if byte & 0xC0 != 0x80 {
        bail!("invalid modified-UTF-8 continuation byte 0x{byte:02x} at offset {index}");
    }
    Ok(byte)
}

#[cfg(test)]
mod modified_utf8_tests {
    use super::decode_modified_utf8;

    #[test]
    fn decodes_ascii_identically() {
        assert_eq!(
            decode_modified_utf8(b"java/lang/String").unwrap(),
            "java/lang/String"
        );
    }

    #[test]
    fn decodes_nul_from_two_byte_form() {
        // Modified UTF-8 encodes U+0000 as 0xC0 0x80, not a bare 0x00.
        assert_eq!(decode_modified_utf8(&[0xC0, 0x80]).unwrap(), "\u{0000}");
    }

    #[test]
    fn decodes_supplementary_char_from_surrogate_pair() {
        // U+1F600 😀 as a CESU-8 surrogate pair: D83D DE00, each a 3-byte form.
        let bytes = [0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80];
        let decoded = decode_modified_utf8(&bytes).unwrap();
        assert_eq!(decoded, "\u{1F600}");
        // Faithful round-trip: one scalar, two UTF-16 units (Java length()).
        assert_eq!(decoded.chars().count(), 1);
        assert_eq!(decoded.encode_utf16().count(), 2);
    }

    #[test]
    fn decodes_two_byte_bmp_char() {
        // U+00E9 é encoded as 0xC3 0xA9.
        assert_eq!(decode_modified_utf8(&[0xC3, 0xA9]).unwrap(), "é");
    }

    #[test]
    fn rejects_truncated_sequence() {
        assert!(decode_modified_utf8(&[0xC0]).is_err());
    }
}
