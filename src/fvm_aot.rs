use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

pub struct CompileSpec {
    pub jar_path: PathBuf,
    pub main_class: Option<String>,
    pub output_path: PathBuf,
    pub cc: String,
    pub dry_run: bool,
}

pub fn compile_jar(spec: &CompileSpec) -> Result<()> {
    let main_class = spec
        .main_class
        .as_deref()
        .context("fvm-aot requires a Main-Class manifest entry or --main-class")?;
    let world = read_class_world(&spec.jar_path)?;
    let program = compile_main(&world, &main_class.replace('.', "/"))?;

    if spec.dry_run {
        std::fs::write(
            &spec.output_path,
            format!(
                "dry-run fvm-aot native binary placeholder\nmain_class={}\nprintln_count={}\nhttp_server={}\n",
                main_class,
                program.printlns.len(),
                program.http_server.is_some()
            ),
        )?;
        make_executable(&spec.output_path)?;
        return Ok(());
    }

    let temp = tempfile::tempdir().context("failed to create fvm-aot build directory")?;
    let c_path = temp.path().join("app.c");
    std::fs::write(&c_path, emit_c(&program))
        .with_context(|| format!("failed to write generated C source {}", c_path.display()))?;

    let status = Command::new(&spec.cc)
        .arg("-Os")
        .arg(&c_path)
        .arg("-o")
        .arg(&spec.output_path)
        .status()
        .with_context(|| format!("failed to execute fvm-aot C compiler `{}`", spec.cc))?;
    if !status.success() {
        bail!(
            "fvm-aot C compiler `{}` exited with status {status}",
            spec.cc
        );
    }
    make_executable(&spec.output_path)?;
    Ok(())
}

fn read_class_world(jar_path: &Path) -> Result<ClassWorld> {
    let file = std::fs::File::open(jar_path)
        .with_context(|| format!("failed to open JAR {}", jar_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to read JAR/ZIP archive {}", jar_path.display()))?;

    let mut classes = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_string();
        if !name.ends_with(".class") || name.ends_with("module-info.class") {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let class_file = ClassFile::parse(&bytes)
            .with_context(|| format!("failed to parse classfile entry {name}"))?;
        classes.insert(class_file.this_name.clone(), class_file);
    }
    if classes.is_empty() {
        bail!("fvm-aot found no class files in JAR {}", jar_path.display());
    }
    Ok(ClassWorld { classes })
}

#[derive(Debug)]
struct AotProgram {
    printlns: Vec<Vec<u8>>,
    http_server: Option<HttpServer>,
}

#[derive(Debug)]
struct HttpServer {
    port: u16,
    body: Vec<u8>,
}

#[derive(Debug)]
struct ClassWorld {
    classes: HashMap<String, ClassFile>,
}

impl ClassWorld {
    fn class(&self, name: &str) -> Result<&ClassFile> {
        self.classes
            .get(name)
            .with_context(|| format!("class `{name}` not found in closed-world JAR"))
    }

    fn class_opt(&self, name: &str) -> Option<&ClassFile> {
        self.classes.get(name)
    }

    fn initial_static_values(&self) -> Result<HashMap<StaticFieldKey, Value>> {
        let mut statics = HashMap::new();
        for class_file in self.classes.values() {
            for (key, value) in class_file.initial_static_values()? {
                statics.insert(key, value);
            }
        }
        Ok(statics)
    }
}

fn compile_main(world: &ClassWorld, main_class: &str) -> Result<AotProgram> {
    let class_file = world.class(main_class)?;
    let method = class_file
        .methods
        .iter()
        .find(|method| method.name == "main" && method.descriptor == "([Ljava/lang/String;)V")
        .context("fvm-aot requires public static void main(String[] args)")?;
    if method.access_flags & 0x0008 == 0 {
        bail!("fvm-aot main method must be static");
    }
    let mut evaluator = Evaluator {
        world,
        program: AotProgram {
            printlns: Vec::new(),
            http_server: None,
        },
        statics: world.initial_static_values()?,
        objects: HashMap::new(),
        arrays: HashMap::new(),
        next_ref: 1,
        initialized: HashSet::new(),
        initializing: HashSet::new(),
        depth: 0,
    };
    evaluator.ensure_class_initialized(&class_file.this_name)?;
    let result = evaluator.eval_method(class_file, method, vec![Value::Null])?;
    if result.is_some() {
        bail!("fvm-aot main method must return void");
    }
    Ok(evaluator.program)
}

#[derive(Clone, Debug)]
enum Value {
    Int(i32),
    Bool(bool),
    Char(char),
    String(Vec<u8>),
    Object(usize),
    Array(usize),
    SystemOut,
    Null,
}

type StaticFieldKey = (String, String, String);
type FieldKey = (String, String);

#[derive(Debug)]
struct ObjectValue {
    class: String,
    fields: HashMap<FieldKey, Value>,
}

#[derive(Debug)]
struct ArrayValue {
    component_descriptor: String,
    values: Vec<Value>,
}

struct Evaluator<'a> {
    world: &'a ClassWorld,
    program: AotProgram,
    statics: HashMap<StaticFieldKey, Value>,
    objects: HashMap<usize, ObjectValue>,
    arrays: HashMap<usize, ArrayValue>,
    next_ref: usize,
    initialized: HashSet<String>,
    initializing: HashSet<String>,
    depth: u8,
}

impl Evaluator<'_> {
    fn eval_method(
        &mut self,
        class_file: &ClassFile,
        method: &Method,
        args: Vec<Value>,
    ) -> Result<Option<Value>> {
        if self.depth >= 32 {
            bail!("fvm-aot static method recursion limit exceeded");
        }
        let code = method.code.as_ref().with_context(|| {
            format!(
                "fvm-aot method {}{} has no Code attribute",
                method.name, method.descriptor
            )
        })?;
        let mut locals = vec![None; (code.max_locals as usize).max(args.len())];
        for (index, arg) in args.into_iter().enumerate() {
            locals[index] = Some(arg);
        }
        let mut stack = Vec::new();
        let mut pc = 0_usize;
        let mut steps = 0_usize;
        self.depth += 1;

        while pc < code.bytes.len() {
            steps += 1;
            if steps > 20_000 {
                self.depth -= 1;
                bail!(
                    "fvm-aot exceeded bytecode step limit in {}{}",
                    method.name,
                    method.descriptor
                );
            }
            let opcode_pc = pc;
            let opcode = read_opcode(&code.bytes, &mut pc)?;
            match opcode {
                0x00 => {}
                0x01 => stack.push(Value::Null),
                0x02 => stack.push(Value::Int(-1)),
                0x03..=0x08 => stack.push(Value::Int((opcode - 0x03) as i32)),
                0x10 => stack.push(Value::Int(read_code_u8(&code.bytes, &mut pc)? as i8 as i32)),
                0x11 => stack.push(Value::Int(read_code_i16(&code.bytes, &mut pc)? as i32)),
                0x12 => {
                    let index = read_code_u8(&code.bytes, &mut pc)? as u16;
                    stack.push(self.constant_value(class_file, index)?);
                }
                0x13 => {
                    let index = read_code_u16(&code.bytes, &mut pc)?;
                    stack.push(self.constant_value(class_file, index)?);
                }
                0x15 | 0x19 => {
                    let index = read_code_u8(&code.bytes, &mut pc)? as usize;
                    stack.push(load_local(&locals, index)?);
                }
                0x1a..=0x1d => stack.push(load_local(&locals, (opcode - 0x1a) as usize)?),
                0x2e => self.eval_primitive_array_load(&mut stack, "I")?,
                0x2a..=0x2d => stack.push(load_local(&locals, (opcode - 0x2a) as usize)?),
                0x32 => self.eval_aaload(&mut stack)?,
                0x33 => self.eval_primitive_array_load(&mut stack, "B/Z")?,
                0x34 => self.eval_primitive_array_load(&mut stack, "C")?,
                0x35 => self.eval_primitive_array_load(&mut stack, "S")?,
                0x36 | 0x3a => {
                    let index = read_code_u8(&code.bytes, &mut pc)? as usize;
                    store_local(&mut locals, index, pop_value(&mut stack)?)?;
                }
                0x3b..=0x3e => {
                    let value = pop_value(&mut stack)?;
                    ensure_int(&value)?;
                    store_local(&mut locals, (opcode - 0x3b) as usize, value)?;
                }
                0x4b..=0x4e => {
                    let value = pop_value(&mut stack)?;
                    store_local(&mut locals, (opcode - 0x4b) as usize, value)?;
                }
                0x4f => self.eval_primitive_array_store(&mut stack, "I")?,
                0x53 => self.eval_aastore(&mut stack)?,
                0x54 => self.eval_primitive_array_store(&mut stack, "B/Z")?,
                0x55 => self.eval_primitive_array_store(&mut stack, "C")?,
                0x56 => self.eval_primitive_array_store(&mut stack, "S")?,
                0x57 => {
                    let _ = pop_value(&mut stack)?;
                }
                0x59 => {
                    let value = stack
                        .last()
                        .context("fvm-aot stack underflow on dup")?
                        .clone();
                    stack.push(value);
                }
                0x60 => push_binary_int(&mut stack, i32::wrapping_add)?,
                0x64 => push_binary_int(&mut stack, i32::wrapping_sub)?,
                0x68 => push_binary_int(&mut stack, i32::wrapping_mul)?,
                0x6c => push_div_int(&mut stack)?,
                0x70 => push_rem_int(&mut stack)?,
                0x74 => {
                    let value = pop_int(&mut stack)?;
                    stack.push(Value::Int(value.wrapping_neg()));
                }
                0x84 => {
                    let index = read_code_u8(&code.bytes, &mut pc)? as usize;
                    let delta = read_code_u8(&code.bytes, &mut pc)? as i8 as i32;
                    let value = local_int(&locals, index)?.wrapping_add(delta);
                    store_local(&mut locals, index, Value::Int(value))?;
                }
                0x91 => {
                    let value = pop_int(&mut stack)? as i8 as i32;
                    stack.push(Value::Int(value));
                }
                0x92 => {
                    let value = pop_int(&mut stack)? as u16 as i32;
                    stack.push(Value::Int(value));
                }
                0x93 => {
                    let value = pop_int(&mut stack)? as i16 as i32;
                    stack.push(Value::Int(value));
                }
                0x99..=0x9e => {
                    let offset = read_code_i16(&code.bytes, &mut pc)?;
                    let value = pop_int(&mut stack)?;
                    let take = match opcode {
                        0x99 => value == 0,
                        0x9a => value != 0,
                        0x9b => value < 0,
                        0x9c => value >= 0,
                        0x9d => value > 0,
                        0x9e => value <= 0,
                        _ => unreachable!(),
                    };
                    if take {
                        pc = branch_target(opcode_pc, offset, code.bytes.len())?;
                    }
                }
                0x9f..=0xa4 => {
                    let offset = read_code_i16(&code.bytes, &mut pc)?;
                    let rhs = pop_int(&mut stack)?;
                    let lhs = pop_int(&mut stack)?;
                    let take = match opcode {
                        0x9f => lhs == rhs,
                        0xa0 => lhs != rhs,
                        0xa1 => lhs < rhs,
                        0xa2 => lhs >= rhs,
                        0xa3 => lhs > rhs,
                        0xa4 => lhs <= rhs,
                        _ => unreachable!(),
                    };
                    if take {
                        pc = branch_target(opcode_pc, offset, code.bytes.len())?;
                    }
                }
                0xa5 | 0xa6 => {
                    let offset = read_code_i16(&code.bytes, &mut pc)?;
                    let rhs = pop_value(&mut stack)?;
                    let lhs = pop_value(&mut stack)?;
                    let equals = ref_equals(&lhs, &rhs);
                    let take = if opcode == 0xa5 { equals } else { !equals };
                    if take {
                        pc = branch_target(opcode_pc, offset, code.bytes.len())?;
                    }
                }
                0xa7 => {
                    let offset = read_code_i16(&code.bytes, &mut pc)?;
                    pc = branch_target(opcode_pc, offset, code.bytes.len())?;
                }
                0xac => {
                    let result = Value::Int(pop_int(&mut stack)?);
                    self.depth -= 1;
                    return Ok(Some(result));
                }
                0xb0 => {
                    let result = pop_value(&mut stack)?;
                    match result {
                        Value::String(_) | Value::Object(_) | Value::Array(_) | Value::Null => {
                            self.depth -= 1;
                            return Ok(Some(result));
                        }
                        other => bail!("fvm-aot areturn expected reference value, got {other:?}"),
                    }
                }
                0xb1 => {
                    self.depth -= 1;
                    return Ok(None);
                }
                0xbe => self.eval_arraylength(&mut stack)?,
                0xb2 => self.eval_getstatic(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb3 => self.eval_putstatic(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb4 => self.eval_getfield(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb5 => self.eval_putfield(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb6 => self.eval_invokevirtual(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb7 => self.eval_invokespecial(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb8 => self.eval_invokestatic(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xb9 => self.eval_invokeinterface(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xba => self.eval_invokedynamic(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xbb => self.eval_new(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xbc => self.eval_newarray(&code.bytes, &mut pc, &mut stack)?,
                0xbd => self.eval_anewarray(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xbf => bail!("fvm-aot exceptions/athrow are not supported yet"),
                0xc0 => self.eval_checkcast(class_file, &code.bytes, &mut pc, &mut stack)?,
                0xc6 | 0xc7 => {
                    let offset = read_code_i16(&code.bytes, &mut pc)?;
                    let value = pop_value(&mut stack)?;
                    let is_null = matches!(value, Value::Null);
                    let take = if opcode == 0xc6 { is_null } else { !is_null };
                    if take {
                        pc = branch_target(opcode_pc, offset, code.bytes.len())?;
                    }
                }
                other => bail!(
                    "fvm-aot unsupported opcode 0x{other:02x}; supported subset includes int-compatible locals/arithmetic/branches, same-class objects/static helpers/fields, arrays, core String/Object intrinsics, println, and Http.respond"
                ),
            }
        }

        self.depth -= 1;
        bail!(
            "fvm-aot method {}{} ended without return",
            method.name,
            method.descriptor
        )
    }

    fn constant_value(&self, class_file: &ClassFile, index: u16) -> Result<Value> {
        match class_file.constant(index)? {
            Constant::Integer(_) => Ok(Value::Int(class_file.int_constant(index)?)),
            Constant::String { .. } => Ok(Value::String(
                class_file.string_constant(index)?.into_bytes(),
            )),
            other => bail!("fvm-aot cannot load constant {index}: {other:?}"),
        }
    }

    fn eval_getstatic(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let field_index = read_code_u16(code, pc)?;
        let field = class_file.field_ref(field_index)?;
        if field.class == "java/lang/System"
            && field.name == "out"
            && field.descriptor == "Ljava/io/PrintStream;"
        {
            stack.push(Value::SystemOut);
            return Ok(());
        }
        if let Some(target_class) = self.world.class_opt(&field.class).cloned() {
            self.ensure_class_initialized(&field.class)?;
            target_class.static_field(&field.name, &field.descriptor)?;
            let key = static_field_key(&field.class, &field.name, &field.descriptor);
            let value = match self.statics.get(&key) {
                Some(value) => value.clone(),
                None => default_field_value(&field.descriptor)?,
            };
            stack.push(value);
            return Ok(());
        }
        bail!(
            "fvm-aot only supports getstatic java.lang.System.out or closed-world static fields, got {}.{}:{}",
            field.class,
            field.name,
            field.descriptor
        )
    }

    fn eval_putstatic(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let field_index = read_code_u16(code, pc)?;
        let field = class_file.field_ref(field_index)?;
        let target_class = self.world.class(&field.class)?.clone();
        target_class.static_field(&field.name, &field.descriptor)?;
        let value = pop_field_value(stack, &field.descriptor)?;
        self.ensure_reference_value_for_descriptor(&value, &field.descriptor)?;
        self.statics.insert(
            static_field_key(&field.class, &field.name, &field.descriptor),
            value,
        );
        Ok(())
    }

    fn eval_new(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let class_index = read_code_u16(code, pc)?;
        let class = class_file.class_name(class_index)?;
        self.world.class(&class)?;
        self.ensure_class_initialized(&class)?;
        let id = self.allocate_ref();
        self.objects.insert(
            id,
            ObjectValue {
                class,
                fields: HashMap::new(),
            },
        );
        stack.push(Value::Object(id));
        Ok(())
    }

    fn eval_getfield(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let field_index = read_code_u16(code, pc)?;
        let field = class_file.field_ref(field_index)?;
        let object_id = pop_object(stack)?;
        let field = self.resolve_instance_field(&field)?;
        let object = self.object(object_id)?;
        if object.class != field.class {
            bail!(
                "fvm-aot object class mismatch: field {}.{}:{} read from {}",
                field.class,
                field.name,
                field.descriptor,
                object.class
            );
        }
        let key = field_key(&field.name, &field.descriptor);
        let value = match object.fields.get(&key) {
            Some(value) => value.clone(),
            None => default_field_value(&field.descriptor)?,
        };
        stack.push(value);
        Ok(())
    }

    fn eval_putfield(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let field_index = read_code_u16(code, pc)?;
        let field = class_file.field_ref(field_index)?;
        let field = self.resolve_instance_field(&field)?;
        let value = pop_field_value(stack, &field.descriptor)?;
        self.ensure_reference_value_for_descriptor(&value, &field.descriptor)?;
        let object_id = pop_object(stack)?;
        let object = self.object_mut(object_id)?;
        if object.class != field.class {
            bail!(
                "fvm-aot object class mismatch: field {}.{}:{} written to {}",
                field.class,
                field.name,
                field.descriptor,
                object.class
            );
        }
        object
            .fields
            .insert(field_key(&field.name, &field.descriptor), value);
        Ok(())
    }

    fn eval_newarray(&mut self, code: &[u8], pc: &mut usize, stack: &mut Vec<Value>) -> Result<()> {
        let atype = read_code_u8(code, pc)?;
        let component_descriptor = newarray_component_descriptor(atype)?;
        let len = pop_array_len(stack)?;
        let id = self.allocate_ref();
        self.arrays.insert(
            id,
            ArrayValue {
                component_descriptor: component_descriptor.to_string(),
                values: vec![default_field_value(component_descriptor)?; len],
            },
        );
        stack.push(Value::Array(id));
        Ok(())
    }

    fn eval_anewarray(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let class_index = read_code_u16(code, pc)?;
        let class = class_file.class_name(class_index)?;
        if class != "java/lang/String" {
            self.world.class(&class)?;
        }
        let component_descriptor = class_descriptor(&class);
        let len = pop_array_len(stack)?;
        let id = self.allocate_ref();
        self.arrays.insert(
            id,
            ArrayValue {
                component_descriptor,
                values: vec![Value::Null; len],
            },
        );
        stack.push(Value::Array(id));
        Ok(())
    }

    fn eval_primitive_array_load(&self, stack: &mut Vec<Value>, expected: &str) -> Result<()> {
        let index = pop_array_index(stack)?;
        let array_id = pop_array(stack)?;
        let array = self.array(array_id)?;
        if !primitive_array_opcode_matches(expected, &array.component_descriptor) {
            bail!(
                "fvm-aot primitive array load expected {expected}, got [{}",
                array.component_descriptor
            );
        }
        let value = array
            .values
            .get(index)
            .with_context(|| format!("fvm-aot array index {index} out of bounds"))?;
        stack.push(array_value_for_stack(
            value.clone(),
            &array.component_descriptor,
        )?);
        Ok(())
    }

    fn eval_aaload(&self, stack: &mut Vec<Value>) -> Result<()> {
        let index = pop_array_index(stack)?;
        let array_id = pop_array(stack)?;
        let array = self.array(array_id)?;
        if array.component_descriptor == "I" {
            bail!("fvm-aot aaload expected reference array, got int array");
        }
        let value = array
            .values
            .get(index)
            .with_context(|| format!("fvm-aot array index {index} out of bounds"))?;
        stack.push(value.clone());
        Ok(())
    }

    fn eval_primitive_array_store(&mut self, stack: &mut Vec<Value>, expected: &str) -> Result<()> {
        let value = pop_int(stack)?;
        let index = pop_array_index(stack)?;
        let array_id = pop_array(stack)?;
        let array = self.array_mut(array_id)?;
        if !primitive_array_opcode_matches(expected, &array.component_descriptor) {
            bail!(
                "fvm-aot primitive array store expected {expected}, got [{}",
                array.component_descriptor
            );
        }
        let value = array_value_from_int(value, &array.component_descriptor)?;
        let slot = array
            .values
            .get_mut(index)
            .with_context(|| format!("fvm-aot array index {index} out of bounds"))?;
        *slot = value;
        Ok(())
    }

    fn eval_aastore(&mut self, stack: &mut Vec<Value>) -> Result<()> {
        let value = pop_value(stack)?;
        let index = pop_array_index(stack)?;
        let array_id = pop_array(stack)?;
        let component_descriptor = self.array(array_id)?.component_descriptor.clone();
        if component_descriptor == "I" {
            bail!("fvm-aot aastore expected reference array, got int array");
        }
        self.ensure_reference_value_for_descriptor(&value, &component_descriptor)?;
        let array = self.array_mut(array_id)?;
        let slot = array
            .values
            .get_mut(index)
            .with_context(|| format!("fvm-aot array index {index} out of bounds"))?;
        *slot = value;
        Ok(())
    }

    fn eval_arraylength(&self, stack: &mut Vec<Value>) -> Result<()> {
        let array_id = pop_array(stack)?;
        let len = self.array(array_id)?.values.len();
        let len = i32::try_from(len).context("fvm-aot array length exceeded i32")?;
        stack.push(Value::Int(len));
        Ok(())
    }

    fn eval_checkcast(
        &self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut [Value],
    ) -> Result<()> {
        let class_index = read_code_u16(code, pc)?;
        let class = class_file.class_name(class_index)?;
        let value = stack
            .last()
            .context("fvm-aot stack underflow on checkcast")?;
        if matches!(value, Value::Null) || self.reference_matches_class(value, &class)? {
            return Ok(());
        }
        bail!("fvm-aot checkcast to {class} failed for {value:?}")
    }

    fn eval_invokevirtual(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let method_index = read_code_u16(code, pc)?;
        let method = class_file.method_ref(method_index)?;
        if method.class == "java/io/PrintStream" && method.name == "println" {
            let value = match method.descriptor.as_str() {
                "(Ljava/lang/String;)V" => pop_string(stack)?,
                "(I)V" => pop_int(stack)?.to_string().into_bytes(),
                "(Z)V" => value_to_string_bytes(&Value::Bool(bool_value(pop_value(stack)?)?))?,
                "(C)V" => value_to_string_bytes(&Value::Char(char_value(pop_value(stack)?)?))?,
                other => bail!("fvm-aot unsupported println descriptor {other}"),
            };
            match pop_value(stack)? {
                Value::SystemOut => {
                    self.program.printlns.push(value);
                    return Ok(());
                }
                other => bail!("fvm-aot println receiver was not System.out: {other:?}"),
            }
        }

        let (params, return_type) = parse_method_descriptor(&method.descriptor)?;
        let mut args = Vec::with_capacity(params.len() + 1);
        let mut params_values = Vec::with_capacity(params.len());
        for param in params.iter().rev() {
            params_values.push(pop_typed(stack, param)?);
        }
        params_values.reverse();
        let receiver_value = pop_value(stack)?;
        if self.eval_core_virtual(
            &method,
            receiver_value.clone(),
            &params_values,
            return_type.clone(),
            stack,
        )? {
            return Ok(());
        }
        let receiver = match receiver_value {
            Value::Object(id) => id,
            Value::Null => bail!("fvm-aot null object reference during invokevirtual"),
            other => bail!(
                "fvm-aot unsupported invokevirtual receiver for {}.{}{}: {other:?}",
                method.class,
                method.name,
                method.descriptor
            ),
        };
        let actual_class_name = self.object(receiver)?.class.clone();
        let target_class = self.world.class(&actual_class_name)?.clone();
        let target = target_class
            .method(&method.name, &method.descriptor)?
            .clone();
        if target.access_flags & 0x0008 != 0 {
            bail!(
                "fvm-aot invokevirtual target {}{} is static",
                method.name,
                method.descriptor
            );
        }
        args.push(Value::Object(receiver));
        args.extend(params_values);
        let result = self.eval_method(&target_class, &target, args)?;
        self.push_method_result(&method.name, &method.descriptor, return_type, result, stack)
    }

    fn eval_core_virtual(
        &mut self,
        method: &ResolvedMember,
        receiver: Value,
        args: &[Value],
        return_type: JvmType,
        stack: &mut Vec<Value>,
    ) -> Result<bool> {
        let result = if matches!(receiver, Value::String(_))
            && (method.class == "java/lang/String" || method.class == "java/lang/Object")
        {
            Some(self.eval_string_virtual(method, receiver, args)?)
        } else if matches!(receiver, Value::Array(_))
            && (method.class == "java/lang/Object" || method.class.starts_with('['))
        {
            Some(self.eval_array_virtual(method, receiver, args)?)
        } else if method.class == "java/lang/Object" {
            Some(self.eval_object_virtual(method, receiver, args)?)
        } else {
            None
        };

        let Some(result) = result else {
            return Ok(false);
        };
        self.push_method_result(&method.name, &method.descriptor, return_type, result, stack)?;
        Ok(true)
    }

    fn eval_string_virtual(
        &self,
        method: &ResolvedMember,
        receiver: Value,
        args: &[Value],
    ) -> Result<Option<Value>> {
        let Value::String(bytes) = receiver else {
            bail!("fvm-aot null String receiver during compile-time evaluation");
        };
        match (method.name.as_str(), method.descriptor.as_str()) {
            ("length", "()I") => Ok(Some(Value::Int(java_string_len(&bytes)?))),
            ("isEmpty", "()Z") => Ok(Some(Value::Bool(bytes.is_empty()))),
            ("charAt", "(I)C") => {
                let index = arg_int(args, 0)?;
                Ok(Some(Value::Char(java_string_char_at(&bytes, index)?)))
            }
            ("equals", "(Ljava/lang/Object;)Z") => {
                let other = arg_value(args, 0)?;
                Ok(Some(Value::Bool(
                    matches!(other, Value::String(other) if *other == bytes),
                )))
            }
            ("hashCode", "()I") => Ok(Some(Value::Int(java_string_hash_code(&bytes)?))),
            ("toString", "()Ljava/lang/String;") => Ok(Some(Value::String(bytes))),
            ("startsWith", "(Ljava/lang/String;)Z") => {
                let prefix = arg_string(args, 0)?;
                Ok(Some(Value::Bool(bytes.starts_with(prefix))))
            }
            ("endsWith", "(Ljava/lang/String;)Z") => {
                let suffix = arg_string(args, 0)?;
                Ok(Some(Value::Bool(bytes.ends_with(suffix))))
            }
            ("contains", "(Ljava/lang/CharSequence;)Z") => {
                let needle = arg_string(args, 0)?;
                Ok(Some(Value::Bool(
                    bytes.windows(needle.len()).any(|window| window == needle),
                )))
            }
            ("substring", "(I)Ljava/lang/String;") => {
                let start = arg_int(args, 0)?;
                Ok(Some(Value::String(java_string_substring(
                    &bytes,
                    start,
                    java_string_len(&bytes)?,
                )?)))
            }
            ("substring", "(II)Ljava/lang/String;") => {
                let start = arg_int(args, 0)?;
                let end = arg_int(args, 1)?;
                Ok(Some(Value::String(java_string_substring(
                    &bytes, start, end,
                )?)))
            }
            _ => bail!(
                "fvm-aot unsupported String intrinsic {}{}",
                method.name,
                method.descriptor
            ),
        }
    }

    fn eval_object_virtual(
        &self,
        method: &ResolvedMember,
        receiver: Value,
        args: &[Value],
    ) -> Result<Option<Value>> {
        if matches!(receiver, Value::Null) {
            bail!("fvm-aot null Object receiver during compile-time evaluation");
        }
        match (method.name.as_str(), method.descriptor.as_str()) {
            ("equals", "(Ljava/lang/Object;)Z") => {
                let other = arg_value(args, 0)?;
                Ok(Some(Value::Bool(ref_equals(&receiver, other))))
            }
            ("hashCode", "()I") => Ok(Some(Value::Int(self.reference_hash_code(&receiver)?))),
            ("toString", "()Ljava/lang/String;") => {
                Ok(Some(Value::String(self.reference_to_string(&receiver)?)))
            }
            _ => bail!(
                "fvm-aot unsupported Object intrinsic {}{}",
                method.name,
                method.descriptor
            ),
        }
    }

    fn eval_array_virtual(
        &mut self,
        method: &ResolvedMember,
        receiver: Value,
        args: &[Value],
    ) -> Result<Option<Value>> {
        match (method.name.as_str(), method.descriptor.as_str()) {
            ("clone", _) if args.is_empty() => {
                let Value::Array(id) = receiver else {
                    bail!("fvm-aot array clone receiver was not an array");
                };
                let source = self.array(id)?;
                let clone = ArrayValue {
                    component_descriptor: source.component_descriptor.clone(),
                    values: source.values.clone(),
                };
                let clone_id = self.allocate_ref();
                self.arrays.insert(clone_id, clone);
                Ok(Some(Value::Array(clone_id)))
            }
            ("equals", "(Ljava/lang/Object;)Z") => {
                let other = arg_value(args, 0)?;
                Ok(Some(Value::Bool(ref_equals(&receiver, other))))
            }
            ("hashCode", "()I") => Ok(Some(Value::Int(self.reference_hash_code(&receiver)?))),
            ("toString", "()Ljava/lang/String;") => {
                Ok(Some(Value::String(self.reference_to_string(&receiver)?)))
            }
            _ => bail!(
                "fvm-aot unsupported array intrinsic {}{}",
                method.name,
                method.descriptor
            ),
        }
    }

    fn eval_invokeinterface(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let method_index = read_code_u16(code, pc)?;
        let method = class_file.method_ref(method_index)?;
        let _count = read_code_u8(code, pc)?;
        let zero = read_code_u8(code, pc)?;
        if zero != 0 {
            bail!("fvm-aot invokeinterface reserved byte must be 0, got {zero}");
        }

        let (params, return_type) = parse_method_descriptor(&method.descriptor)?;
        let mut args = Vec::with_capacity(params.len() + 1);
        let mut params_values = Vec::with_capacity(params.len());
        for param in params.iter().rev() {
            params_values.push(pop_typed(stack, param)?);
        }
        params_values.reverse();
        let receiver = pop_object(stack)?;
        let actual_class_name = self.object(receiver)?.class.clone();
        let target_class = self.world.class(&actual_class_name)?.clone();
        let target = target_class
            .method(&method.name, &method.descriptor)?
            .clone();
        if target.access_flags & 0x0008 != 0 {
            bail!(
                "fvm-aot invokeinterface target {}{} is static",
                method.name,
                method.descriptor
            );
        }
        args.push(Value::Object(receiver));
        args.extend(params_values);
        let result = self.eval_method(&target_class, &target, args)?;
        self.push_method_result(&method.name, &method.descriptor, return_type, result, stack)
    }

    fn eval_invokespecial(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let method_index = read_code_u16(code, pc)?;
        let method = class_file.method_ref(method_index)?;
        let (params, return_type) = parse_method_descriptor(&method.descriptor)?;
        let mut args = Vec::with_capacity(params.len() + 1);
        let mut params_values = Vec::with_capacity(params.len());
        for param in params.iter().rev() {
            params_values.push(pop_typed(stack, param)?);
        }
        params_values.reverse();
        let receiver = pop_object(stack)?;

        if method.class == "java/lang/Object"
            && method.name == "<init>"
            && method.descriptor == "()V"
        {
            return Ok(());
        }

        let target_class = self.world.class(&method.class)?.clone();
        self.ensure_object_class(receiver, &method.class)?;
        args.push(Value::Object(receiver));
        args.extend(params_values);
        let target = target_class
            .method(&method.name, &method.descriptor)?
            .clone();
        if target.access_flags & 0x0008 != 0 {
            bail!(
                "fvm-aot invokespecial target {}{} is static",
                method.name,
                method.descriptor
            );
        }
        let result = self.eval_method(&target_class, &target, args)?;
        self.push_method_result(&method.name, &method.descriptor, return_type, result, stack)
    }

    fn eval_invokestatic(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let method_index = read_code_u16(code, pc)?;
        let method_ref = class_file.method_ref(method_index)?;
        if method_ref.class == "fvm/runtime/Http"
            && method_ref.name == "respond"
            && method_ref.descriptor == "(ILjava/lang/String;)V"
        {
            let body = pop_string(stack)?;
            let port = pop_int(stack)?;
            if !(1..=65535).contains(&port) {
                bail!("fvm-aot HTTP intrinsic port must be 1..=65535, got {port}");
            }
            if self.program.http_server.is_some() {
                bail!("fvm-aot supports only one Http.respond server per app for now");
            }
            self.program.http_server = Some(HttpServer {
                port: port as u16,
                body,
            });
            return Ok(());
        }

        let target_class = self.world.class(&method_ref.class)?.clone();
        self.ensure_class_initialized(&method_ref.class)?;
        let (params, return_type) = parse_method_descriptor(&method_ref.descriptor)?;
        let mut args = Vec::with_capacity(params.len());
        for param in params.iter().rev() {
            args.push(pop_typed(stack, param)?);
        }
        args.reverse();
        let helper = target_class.static_method(&method_ref.name, &method_ref.descriptor)?;
        let helper = helper.clone();
        let result = self.eval_method(&target_class, &helper, args)?;
        self.push_method_result(
            &method_ref.name,
            &method_ref.descriptor,
            return_type,
            result,
            stack,
        )
    }

    fn eval_invokedynamic(
        &mut self,
        class_file: &ClassFile,
        code: &[u8],
        pc: &mut usize,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        let dynamic_index = read_code_u16(code, pc)?;
        let zero1 = read_code_u8(code, pc)?;
        let zero2 = read_code_u8(code, pc)?;
        if zero1 != 0 || zero2 != 0 {
            bail!("fvm-aot invokedynamic reserved bytes must be 0, got {zero1}/{zero2}");
        }

        let dynamic = class_file.invoke_dynamic(dynamic_index)?;
        let bootstrap = class_file.bootstrap_method(dynamic.bootstrap_method_attr_index)?;
        let bootstrap_method = class_file.method_handle_ref(bootstrap.method_ref)?;
        if bootstrap_method.class != "java/lang/invoke/StringConcatFactory" {
            bail!(
                "fvm-aot only supports StringConcatFactory invokedynamic for now, got {}.{}{}",
                bootstrap_method.class,
                bootstrap_method.name,
                bootstrap_method.descriptor
            );
        }
        if dynamic.name != "makeConcat" && dynamic.name != "makeConcatWithConstants" {
            bail!("fvm-aot unsupported invokedynamic name `{}`", dynamic.name);
        }

        let (params, return_type) = parse_method_descriptor(&dynamic.descriptor)?;
        if return_type != JvmType::String {
            bail!(
                "fvm-aot StringConcatFactory invokedynamic must return String, got {}",
                dynamic.descriptor
            );
        }
        let mut args = Vec::with_capacity(params.len());
        for param in params.iter().rev() {
            args.push(pop_typed(stack, param)?);
        }
        args.reverse();

        let result = if bootstrap_method.name == "makeConcatWithConstants" {
            let recipe_index = *bootstrap
                .arguments
                .first()
                .context("fvm-aot concat bootstrap missing recipe argument")?;
            let recipe = class_file.string_constant(recipe_index)?;
            let constants = bootstrap
                .arguments
                .iter()
                .skip(1)
                .map(|index| self.constant_value(class_file, *index))
                .collect::<Result<Vec<_>>>()?;
            eval_concat_recipe(&recipe, &args, &constants)?
        } else if bootstrap_method.name == "makeConcat" {
            concat_values(&args)?
        } else {
            bail!(
                "fvm-aot unsupported StringConcatFactory method {}{}",
                bootstrap_method.name,
                bootstrap_method.descriptor
            );
        };
        stack.push(Value::String(result));
        Ok(())
    }

    fn push_method_result(
        &mut self,
        name: &str,
        descriptor: &str,
        return_type: JvmType,
        result: Option<Value>,
        stack: &mut Vec<Value>,
    ) -> Result<()> {
        match return_type {
            JvmType::Void => {
                if result.is_some() {
                    bail!(
                        "fvm-aot helper {}{} returned a value unexpectedly",
                        name,
                        descriptor
                    );
                }
            }
            JvmType::Int
            | JvmType::Boolean
            | JvmType::Char
            | JvmType::String
            | JvmType::Object(_)
            | JvmType::Array(_) => {
                let Some(result) = result else {
                    bail!(
                        "fvm-aot helper {}{} did not return a value",
                        name,
                        descriptor
                    );
                };
                stack.push(pop_typed_value(result, &return_type)?);
            }
            JvmType::Unsupported => {
                bail!(
                    "fvm-aot unsupported helper return type in {}{}",
                    name,
                    descriptor
                )
            }
        }
        Ok(())
    }

    fn allocate_ref(&mut self) -> usize {
        let id = self.next_ref;
        self.next_ref += 1;
        id
    }

    fn ensure_class_initialized(&mut self, class: &str) -> Result<()> {
        if self.initialized.contains(class) || self.world.class_opt(class).is_none() {
            return Ok(());
        }
        if self.initializing.contains(class) {
            return Ok(());
        }

        self.initializing.insert(class.to_string());
        let class_file = self.world.class(class)?.clone();
        if let Some(clinit) = class_file.static_method_opt("<clinit>", "()V").cloned() {
            let result = self.eval_method(&class_file, &clinit, Vec::new())?;
            if result.is_some() {
                bail!("fvm-aot <clinit> for {class} must return void");
            }
        }
        self.initializing.remove(class);
        self.initialized.insert(class.to_string());
        Ok(())
    }

    fn resolve_instance_field(&self, field: &ResolvedMember) -> Result<ResolvedMember> {
        let class_file = self.world.class(&field.class)?;
        class_file.instance_field(&field.name, &field.descriptor)?;
        Ok(field.clone())
    }

    fn object(&self, id: usize) -> Result<&ObjectValue> {
        self.objects
            .get(&id)
            .with_context(|| format!("fvm-aot invalid object reference {id}"))
    }

    fn object_mut(&mut self, id: usize) -> Result<&mut ObjectValue> {
        self.objects
            .get_mut(&id)
            .with_context(|| format!("fvm-aot invalid object reference {id}"))
    }

    fn array(&self, id: usize) -> Result<&ArrayValue> {
        self.arrays
            .get(&id)
            .with_context(|| format!("fvm-aot invalid array reference {id}"))
    }

    fn array_mut(&mut self, id: usize) -> Result<&mut ArrayValue> {
        self.arrays
            .get_mut(&id)
            .with_context(|| format!("fvm-aot invalid array reference {id}"))
    }

    fn ensure_object_class(&self, id: usize, class: &str) -> Result<()> {
        let object = self.object(id)?;
        if object.class != class {
            bail!(
                "fvm-aot expected object of class {class}, got {}",
                object.class
            );
        }
        Ok(())
    }

    fn reference_hash_code(&self, value: &Value) -> Result<i32> {
        match value {
            Value::String(bytes) => java_string_hash_code(bytes),
            Value::Object(id) | Value::Array(id) => {
                i32::try_from(*id).context("fvm-aot reference id exceeded i32")
            }
            Value::Null => bail!("fvm-aot null receiver during hashCode"),
            other => bail!("fvm-aot value {other:?} is not a reference"),
        }
    }

    fn reference_to_string(&self, value: &Value) -> Result<Vec<u8>> {
        match value {
            Value::String(bytes) => Ok(bytes.clone()),
            Value::Object(id) => {
                let object = self.object(*id)?;
                Ok(format!(
                    "{}@{:x}",
                    object.class.replace('/', "."),
                    self.reference_hash_code(value)?
                )
                .into_bytes())
            }
            Value::Array(id) => {
                let array = self.array(*id)?;
                Ok(format!(
                    "{}@{:x}",
                    array_class_name(&array.component_descriptor),
                    self.reference_hash_code(value)?
                )
                .into_bytes())
            }
            Value::Null => bail!("fvm-aot null receiver during toString"),
            other => bail!("fvm-aot value {other:?} is not a reference"),
        }
    }

    fn reference_matches_class(&self, value: &Value, class: &str) -> Result<bool> {
        match value {
            Value::String(_) => Ok(class == "java/lang/String" || class == "java/lang/Object"),
            Value::Object(id) => {
                let object = self.object(*id)?;
                Ok(class == "java/lang/Object" || object.class == class)
            }
            Value::Array(id) => {
                let array_descriptor = format!("[{}", self.array(*id)?.component_descriptor);
                Ok(class == "java/lang/Object" || class == array_descriptor)
            }
            Value::Null => Ok(true),
            _ => Ok(false),
        }
    }

    fn ensure_reference_value_for_descriptor(&self, value: &Value, descriptor: &str) -> Result<()> {
        match value {
            Value::Int(_) if descriptor == "I" => Ok(()),
            Value::Int(_) if matches!(descriptor, "B" | "S") => Ok(()),
            Value::Bool(_) if descriptor == "Z" => Ok(()),
            Value::Int(value) if descriptor == "Z" && (*value == 0 || *value == 1) => Ok(()),
            Value::Char(_) if descriptor == "C" => Ok(()),
            Value::Int(value) if descriptor == "C" && char::from_u32(*value as u32).is_some() => {
                Ok(())
            }
            Value::String(_) if descriptor == "Ljava/lang/String;" => Ok(()),
            Value::String(_)
                if descriptor == "Ljava/lang/Object;"
                    || descriptor == "Ljava/lang/CharSequence;" =>
            {
                Ok(())
            }
            Value::Array(_) if descriptor == "Ljava/lang/Object;" => Ok(()),
            Value::Object(id) if descriptor.starts_with('L') && descriptor.ends_with(';') => {
                let class = descriptor_to_class(descriptor)?;
                if class == "java/lang/Object" {
                    Ok(())
                } else {
                    self.ensure_object_class(*id, class)
                }
            }
            Value::Array(id) if descriptor.starts_with('[') => {
                let expected = array_component_descriptor(descriptor)?;
                let actual = &self.array(*id)?.component_descriptor;
                if actual != expected {
                    bail!("fvm-aot expected array component {expected}, got {actual}");
                }
                Ok(())
            }
            Value::Null if descriptor.starts_with('L') || descriptor.starts_with('[') => Ok(()),
            other => bail!("fvm-aot value {other:?} does not match descriptor {descriptor}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum JvmType {
    Int,
    Boolean,
    Char,
    String,
    Object(String),
    Array(String),
    Void,
    Unsupported,
}

fn parse_method_descriptor(descriptor: &str) -> Result<(Vec<JvmType>, JvmType)> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        bail!("invalid method descriptor `{descriptor}`");
    }
    let mut index = 1_usize;
    let mut params = Vec::new();
    while index < bytes.len() && bytes[index] != b')' {
        params.push(parse_type(descriptor, &mut index)?);
    }
    if index >= bytes.len() || bytes[index] != b')' {
        bail!("invalid method descriptor `{descriptor}`");
    }
    index += 1;
    let return_type = parse_type(descriptor, &mut index)?;
    if index != bytes.len() {
        bail!("invalid trailing method descriptor data `{descriptor}`");
    }
    Ok((params, return_type))
}

fn parse_type(descriptor: &str, index: &mut usize) -> Result<JvmType> {
    let bytes = descriptor.as_bytes();
    if *index >= bytes.len() {
        bail!("truncated method descriptor `{descriptor}`");
    }
    let ty = match bytes[*index] {
        b'B' | b'S' | b'I' => {
            *index += 1;
            JvmType::Int
        }
        b'C' => {
            *index += 1;
            JvmType::Char
        }
        b'Z' => {
            *index += 1;
            JvmType::Boolean
        }
        b'V' => {
            *index += 1;
            JvmType::Void
        }
        b'L' => {
            let start = *index + 1;
            let Some(end) = descriptor[start..].find(';').map(|offset| start + offset) else {
                bail!("unterminated object type in descriptor `{descriptor}`");
            };
            let class = &descriptor[start..end];
            *index = end + 1;
            if class == "java/lang/String" {
                JvmType::String
            } else {
                JvmType::Object(class.to_string())
            }
        }
        b'[' => {
            let start = *index;
            while *index < bytes.len() && bytes[*index] == b'[' {
                *index += 1;
            }
            let _ = parse_type(descriptor, index)?;
            JvmType::Array(descriptor[start..*index].to_string())
        }
        _ => {
            *index += 1;
            JvmType::Unsupported
        }
    };
    Ok(ty)
}

fn load_local(locals: &[Option<Value>], index: usize) -> Result<Value> {
    locals
        .get(index)
        .and_then(|value| value.clone())
        .with_context(|| format!("fvm-aot read uninitialized local {index}"))
}

fn store_local(locals: &mut Vec<Option<Value>>, index: usize, value: Value) -> Result<()> {
    if index >= locals.len() {
        locals.resize(index + 1, None);
    }
    locals[index] = Some(value);
    Ok(())
}

fn int_value(value: Value) -> Result<i32> {
    match value {
        Value::Int(value) => Ok(value),
        Value::Bool(value) => Ok(if value { 1 } else { 0 }),
        Value::Char(value) => Ok(value as i32),
        other => bail!("fvm-aot expected int-compatible value, got {other:?}"),
    }
}

fn bool_value(value: Value) -> Result<bool> {
    match value {
        Value::Bool(value) => Ok(value),
        Value::Int(value) => Ok(value != 0),
        other => bail!("fvm-aot expected boolean-compatible value, got {other:?}"),
    }
}

fn char_value(value: Value) -> Result<char> {
    match value {
        Value::Char(value) => Ok(value),
        Value::Int(value) => char_from_i32(value),
        other => bail!("fvm-aot expected char-compatible value, got {other:?}"),
    }
}

fn char_from_i32(value: i32) -> Result<char> {
    if !(0..=0xffff).contains(&value) {
        bail!("fvm-aot char value {value} is outside Java char range");
    }
    char_from_u16(value as u16)
}

fn char_from_u16(value: u16) -> Result<char> {
    char::from_u32(value as u32)
        .with_context(|| format!("fvm-aot Java surrogate char 0x{value:04x} is unsupported"))
}

fn local_int(locals: &[Option<Value>], index: usize) -> Result<i32> {
    int_value(load_local(locals, index)?)
        .with_context(|| format!("fvm-aot local {index} is not int-compatible"))
}

fn pop_value(stack: &mut Vec<Value>) -> Result<Value> {
    stack.pop().context("fvm-aot stack underflow")
}

fn pop_int(stack: &mut Vec<Value>) -> Result<i32> {
    int_value(pop_value(stack)?)
}

fn pop_string(stack: &mut Vec<Value>) -> Result<Vec<u8>> {
    match pop_value(stack)? {
        Value::String(value) => Ok(value),
        other => bail!("fvm-aot expected String on stack, got {other:?}"),
    }
}

fn ensure_int(value: &Value) -> Result<()> {
    match value {
        Value::Int(_) | Value::Bool(_) | Value::Char(_) => Ok(()),
        other => bail!("fvm-aot expected int-compatible value, got {other:?}"),
    }
}

fn pop_typed(stack: &mut Vec<Value>, ty: &JvmType) -> Result<Value> {
    let value = pop_value(stack)?;
    pop_typed_value(value, ty)
}

fn pop_typed_value(value: Value, ty: &JvmType) -> Result<Value> {
    match (ty, value) {
        (JvmType::Int, value) => Ok(Value::Int(int_value(value)?)),
        (JvmType::Boolean, value) => Ok(Value::Bool(bool_value(value)?)),
        (JvmType::Char, value) => Ok(Value::Char(char_value(value)?)),
        (JvmType::String, Value::String(value)) => Ok(Value::String(value)),
        (JvmType::String, Value::Null) => Ok(Value::Null),
        (JvmType::Object(_), Value::Object(id)) => Ok(Value::Object(id)),
        (JvmType::Object(_), Value::String(value)) => Ok(Value::String(value)),
        (JvmType::Object(_), Value::Array(id)) => Ok(Value::Array(id)),
        (JvmType::Object(_), Value::Null) => Ok(Value::Null),
        (JvmType::Array(_), Value::Array(id)) => Ok(Value::Array(id)),
        (JvmType::Array(_), Value::Null) => Ok(Value::Null),
        (JvmType::Unsupported, _) => bail!("fvm-aot unsupported parameter or return type"),
        (expected, actual) => bail!("fvm-aot expected {expected:?}, got {actual:?}"),
    }
}

fn static_field_key(class: &str, name: &str, descriptor: &str) -> StaticFieldKey {
    (class.to_string(), name.to_string(), descriptor.to_string())
}

fn field_key(name: &str, descriptor: &str) -> FieldKey {
    (name.to_string(), descriptor.to_string())
}

fn pop_field_value(stack: &mut Vec<Value>, descriptor: &str) -> Result<Value> {
    let value = pop_value(stack)?;
    match (descriptor, value) {
        ("I", value) => Ok(Value::Int(int_value(value)?)),
        ("B", value) => Ok(Value::Int(int_value(value)? as i8 as i32)),
        ("S", value) => Ok(Value::Int(int_value(value)? as i16 as i32)),
        ("Z", value) => Ok(Value::Bool(bool_value(value)?)),
        ("C", value) => Ok(Value::Char(char_value(value)?)),
        ("Ljava/lang/String;", Value::String(value)) => Ok(Value::String(value)),
        ("Ljava/lang/String;", Value::Null) => Ok(Value::Null),
        (descriptor, Value::Array(id)) if descriptor.starts_with('[') => Ok(Value::Array(id)),
        ("Ljava/lang/Object;", Value::Array(id)) => Ok(Value::Array(id)),
        ("Ljava/lang/Object;" | "Ljava/lang/CharSequence;", Value::String(value)) => {
            Ok(Value::String(value))
        }
        (descriptor, Value::Null) if descriptor.starts_with('[') => Ok(Value::Null),
        (descriptor, Value::Object(id)) if descriptor.starts_with('L') => Ok(Value::Object(id)),
        (descriptor, Value::Null) if descriptor.starts_with('L') => Ok(Value::Null),
        (other, actual) => {
            bail!("fvm-aot value {actual:?} does not match field descriptor {other}")
        }
    }
}

fn default_field_value(descriptor: &str) -> Result<Value> {
    match descriptor {
        "B" | "S" | "I" => Ok(Value::Int(0)),
        "Z" => Ok(Value::Bool(false)),
        "C" => Ok(Value::Char('\0')),
        descriptor if descriptor.starts_with('L') || descriptor.starts_with('[') => Ok(Value::Null),
        other => bail!("fvm-aot unsupported field descriptor {other}"),
    }
}

fn supported_field_descriptor(descriptor: &str) -> Result<()> {
    match descriptor {
        "B" | "S" | "I" | "Z" | "C" | "Ljava/lang/String;" => Ok(()),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => Ok(()),
        descriptor if descriptor.starts_with('[') => {
            let component = array_component_descriptor(descriptor)?;
            supported_array_component(component)
        }
        other => bail!("fvm-aot unsupported field descriptor {other}"),
    }
}

fn supported_array_component(descriptor: &str) -> Result<()> {
    match descriptor {
        "B" | "S" | "I" | "Z" | "C" | "Ljava/lang/String;" => Ok(()),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => Ok(()),
        other => bail!("fvm-aot unsupported array component descriptor {other}"),
    }
}

fn class_descriptor(class: &str) -> String {
    format!("L{class};")
}

fn descriptor_to_class(descriptor: &str) -> Result<&str> {
    descriptor
        .strip_prefix('L')
        .and_then(|value| value.strip_suffix(';'))
        .with_context(|| format!("invalid object descriptor {descriptor}"))
}

fn array_component_descriptor(descriptor: &str) -> Result<&str> {
    let component = descriptor
        .strip_prefix('[')
        .with_context(|| format!("invalid array descriptor {descriptor}"))?;
    if component.starts_with('[') {
        bail!("fvm-aot only supports one-dimensional arrays for now");
    }
    Ok(component)
}

fn newarray_component_descriptor(atype: u8) -> Result<&'static str> {
    match atype {
        4 => Ok("Z"),
        5 => Ok("C"),
        8 => Ok("B"),
        9 => Ok("S"),
        10 => Ok("I"),
        other => bail!("fvm-aot unsupported newarray atype {other}"),
    }
}

fn primitive_array_opcode_matches(expected: &str, actual: &str) -> bool {
    match expected {
        "B/Z" => actual == "B" || actual == "Z",
        descriptor => actual == descriptor,
    }
}

fn array_value_for_stack(value: Value, component_descriptor: &str) -> Result<Value> {
    match component_descriptor {
        "B" | "S" | "I" => Ok(Value::Int(int_value(value)?)),
        "Z" => Ok(Value::Bool(bool_value(value)?)),
        "C" => Ok(Value::Char(char_value(value)?)),
        other => bail!("fvm-aot primitive array load does not support component {other}"),
    }
}

fn array_value_from_int(value: i32, component_descriptor: &str) -> Result<Value> {
    match component_descriptor {
        "B" => Ok(Value::Int(value as i8 as i32)),
        "S" => Ok(Value::Int(value as i16 as i32)),
        "I" => Ok(Value::Int(value)),
        "Z" => Ok(Value::Bool(value != 0)),
        "C" => Ok(Value::Char(char_from_u16(value as u16)?)),
        other => bail!("fvm-aot primitive array store does not support component {other}"),
    }
}

fn array_class_name(component_descriptor: &str) -> String {
    match component_descriptor {
        "B" | "S" | "I" | "Z" | "C" => format!("[{component_descriptor}"),
        descriptor if descriptor.starts_with('L') && descriptor.ends_with(';') => {
            format!(
                "[L{};",
                descriptor_to_class(descriptor)
                    .unwrap_or(descriptor)
                    .replace('/', ".")
            )
        }
        descriptor => format!("[{descriptor}"),
    }
}

fn arg_value(args: &[Value], index: usize) -> Result<&Value> {
    args.get(index)
        .with_context(|| format!("fvm-aot intrinsic missing argument {index}"))
}

fn arg_int(args: &[Value], index: usize) -> Result<i32> {
    int_value(arg_value(args, index)?.clone())
}

fn arg_string(args: &[Value], index: usize) -> Result<&[u8]> {
    match arg_value(args, index)? {
        Value::String(bytes) => Ok(bytes),
        Value::Null => bail!("fvm-aot null String argument during compile-time evaluation"),
        other => bail!("fvm-aot expected String argument, got {other:?}"),
    }
}

fn ref_equals(lhs: &Value, rhs: &Value) -> bool {
    match (lhs, rhs) {
        (Value::Null, Value::Null) => true,
        (Value::Object(lhs), Value::Object(rhs)) => lhs == rhs,
        (Value::Array(lhs), Value::Array(rhs)) => lhs == rhs,
        (Value::String(lhs), Value::String(rhs)) => lhs == rhs,
        _ => false,
    }
}

fn pop_object(stack: &mut Vec<Value>) -> Result<usize> {
    match pop_value(stack)? {
        Value::Object(id) => Ok(id),
        Value::Null => bail!("fvm-aot null object reference during compile-time evaluation"),
        other => bail!("fvm-aot expected object reference, got {other:?}"),
    }
}

fn pop_array(stack: &mut Vec<Value>) -> Result<usize> {
    match pop_value(stack)? {
        Value::Array(id) => Ok(id),
        Value::Null => bail!("fvm-aot null array reference during compile-time evaluation"),
        other => bail!("fvm-aot expected array reference, got {other:?}"),
    }
}

fn pop_array_len(stack: &mut Vec<Value>) -> Result<usize> {
    let len = pop_int(stack)?;
    if len < 0 {
        bail!("fvm-aot negative array length {len}");
    }
    Ok(len as usize)
}

fn pop_array_index(stack: &mut Vec<Value>) -> Result<usize> {
    let index = pop_int(stack)?;
    if index < 0 {
        bail!("fvm-aot negative array index {index}");
    }
    Ok(index as usize)
}

fn push_binary_int(stack: &mut Vec<Value>, op: fn(i32, i32) -> i32) -> Result<()> {
    let rhs = pop_int(stack)?;
    let lhs = pop_int(stack)?;
    stack.push(Value::Int(op(lhs, rhs)));
    Ok(())
}

fn push_div_int(stack: &mut Vec<Value>) -> Result<()> {
    let rhs = pop_int(stack)?;
    let lhs = pop_int(stack)?;
    if rhs == 0 {
        bail!("fvm-aot division by zero during compile-time evaluation");
    }
    let result = if lhs == i32::MIN && rhs == -1 {
        i32::MIN
    } else {
        lhs / rhs
    };
    stack.push(Value::Int(result));
    Ok(())
}

fn push_rem_int(stack: &mut Vec<Value>) -> Result<()> {
    let rhs = pop_int(stack)?;
    let lhs = pop_int(stack)?;
    if rhs == 0 {
        bail!("fvm-aot remainder by zero during compile-time evaluation");
    }
    let result = if lhs == i32::MIN && rhs == -1 {
        0
    } else {
        lhs % rhs
    };
    stack.push(Value::Int(result));
    Ok(())
}

fn concat_values(values: &[Value]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    for value in values {
        result.extend(value_to_string_bytes(value)?);
    }
    Ok(result)
}

fn eval_concat_recipe(recipe: &str, args: &[Value], constants: &[Value]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut arg_index = 0_usize;
    let mut constant_index = 0_usize;
    for ch in recipe.chars() {
        match ch {
            '\u{0001}' => {
                let value = args.get(arg_index).with_context(|| {
                    format!("fvm-aot concat recipe referenced missing argument {arg_index}")
                })?;
                result.extend(value_to_string_bytes(value)?);
                arg_index += 1;
            }
            '\u{0002}' => {
                let value = constants.get(constant_index).with_context(|| {
                    format!("fvm-aot concat recipe referenced missing constant {constant_index}")
                })?;
                result.extend(value_to_string_bytes(value)?);
                constant_index += 1;
            }
            _ => {
                let mut buf = [0_u8; 4];
                result.extend(ch.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    if arg_index != args.len() {
        bail!(
            "fvm-aot concat recipe consumed {arg_index} args but descriptor supplied {}",
            args.len()
        );
    }
    if constant_index != constants.len() {
        bail!(
            "fvm-aot concat recipe consumed {constant_index} constants but bootstrap supplied {}",
            constants.len()
        );
    }
    Ok(result)
}

fn value_to_string_bytes(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::Int(value) => Ok(value.to_string().into_bytes()),
        Value::Bool(value) => Ok(if *value {
            b"true".to_vec()
        } else {
            b"false".to_vec()
        }),
        Value::Char(value) => {
            let mut buf = [0_u8; 4];
            Ok(value.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        Value::String(value) => Ok(value.clone()),
        Value::Null => Ok(b"null".to_vec()),
        other => bail!("fvm-aot cannot stringify value {other:?} for invokedynamic concat"),
    }
}

fn java_string(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).context("fvm-aot String value was not valid UTF-8")
}

fn java_string_len(bytes: &[u8]) -> Result<i32> {
    let len = java_string(bytes)?
        .chars()
        .map(char::len_utf16)
        .sum::<usize>();
    i32::try_from(len).context("fvm-aot String length exceeded i32")
}

fn java_string_char_at(bytes: &[u8], index: i32) -> Result<char> {
    if index < 0 {
        bail!("fvm-aot String.charAt negative index {index}");
    }
    let target = index as usize;
    let mut utf16_index = 0_usize;
    for ch in java_string(bytes)?.chars() {
        let width = ch.len_utf16();
        if utf16_index == target {
            if width == 1 {
                return Ok(ch);
            }
            bail!("fvm-aot String.charAt on surrogate pair is unsupported");
        }
        if width == 2 && utf16_index + 1 == target {
            bail!("fvm-aot String.charAt on surrogate pair is unsupported");
        }
        utf16_index += width;
    }
    bail!("fvm-aot String.charAt index {index} out of bounds")
}

fn java_string_substring(bytes: &[u8], start: i32, end: i32) -> Result<Vec<u8>> {
    if start < 0 || end < start {
        bail!("fvm-aot invalid String.substring range {start}..{end}");
    }
    let start = start as usize;
    let end = end as usize;
    let source = java_string(bytes)?;
    let start_byte = java_utf16_boundary_to_byte_index(source, start)?;
    let end_byte = java_utf16_boundary_to_byte_index(source, end)?;
    Ok(source.as_bytes()[start_byte..end_byte].to_vec())
}

fn java_utf16_boundary_to_byte_index(value: &str, target: usize) -> Result<usize> {
    let mut utf16_index = 0_usize;
    for (byte_index, ch) in value.char_indices() {
        if utf16_index == target {
            return Ok(byte_index);
        }
        utf16_index += ch.len_utf16();
        if utf16_index > target {
            bail!("fvm-aot String substring boundary splits a surrogate pair");
        }
    }
    if utf16_index == target {
        return Ok(value.len());
    }
    bail!("fvm-aot String substring boundary {target} out of bounds")
}

fn java_string_hash_code(bytes: &[u8]) -> Result<i32> {
    let mut hash = 0_i32;
    for ch in java_string(bytes)?.chars() {
        let mut units = [0_u16; 2];
        for unit in ch.encode_utf16(&mut units) {
            hash = hash.wrapping_mul(31).wrapping_add(i32::from(*unit));
        }
    }
    Ok(hash)
}

fn branch_target(opcode_pc: usize, offset: i16, code_len: usize) -> Result<usize> {
    let target = opcode_pc as isize + offset as isize;
    if target < 0 || target as usize > code_len {
        bail!("fvm-aot branch target {target} out of range");
    }
    Ok(target as usize)
}

fn emit_c(program: &AotProgram) -> String {
    if let Some(server) = &program.http_server {
        return emit_http_server_c(server);
    }

    let mut c = String::from("#include <stdio.h>\n#include <stddef.h>\n\nint main(void) {\n");
    for (index, bytes) in program.printlns.iter().enumerate() {
        c.push_str("  static const unsigned char msg");
        c.push_str(&index.to_string());
        c.push_str("[] = {");
        for byte in bytes.iter().copied().chain([b'\n']) {
            c.push_str(&format!("0x{byte:02x},"));
        }
        c.push_str("};\n  fwrite(msg");
        c.push_str(&index.to_string());
        c.push_str(", 1, sizeof(msg");
        c.push_str(&index.to_string());
        c.push_str("), stdout);\n");
    }
    c.push_str("  return 0;\n}\n");
    c
}

fn emit_http_server_c(server: &HttpServer) -> String {
    let mut c = String::from(
        "#include <arpa/inet.h>\n#include <netinet/in.h>\n#include <stdio.h>\n#include <string.h>\n#include <sys/socket.h>\n#include <unistd.h>\n\nint main(void) {\n",
    );
    c.push_str("  static const unsigned char body[] = {");
    for byte in &server.body {
        c.push_str(&format!("0x{byte:02x},"));
    }
    c.push_str("};\n");
    c.push_str(&format!(
        "  static const char header[] = \"HTTP/1.1 200 OK\\r\\nContent-Length: {}\\r\\nConnection: close\\r\\n\\r\\n\";\n",
        server.body.len()
    ));
    c.push_str(
        "  int server_fd = socket(AF_INET, SOCK_STREAM, 0);\n  if (server_fd < 0) return 1;\n  int one = 1;\n  setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));\n  struct sockaddr_in addr;\n  memset(&addr, 0, sizeof(addr));\n  addr.sin_family = AF_INET;\n  addr.sin_addr.s_addr = htonl(INADDR_ANY);\n",
    );
    c.push_str(&format!("  addr.sin_port = htons({});\n", server.port));
    c.push_str(
        "  if (bind(server_fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) return 2;\n  if (listen(server_fd, 128) != 0) return 3;\n  for (;;) {\n    int client = accept(server_fd, NULL, NULL);\n    if (client < 0) continue;\n    char request[1024];\n    ssize_t read_result = read(client, request, sizeof(request));\n    if (read_result < 0) { close(client); continue; }\n    ssize_t header_result = write(client, header, sizeof(header) - 1);\n    if (header_result < 0) { close(client); continue; }\n    ssize_t body_result = write(client, body, sizeof(body));\n    if (body_result < 0) { close(client); continue; }\n    close(client);\n  }\n}\n",
    );
    c
}

#[derive(Clone, Debug)]
struct ClassFile {
    this_name: String,
    constants: Vec<Option<Constant>>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    bootstrap_methods: Vec<BootstrapMethod>,
}

#[derive(Clone, Debug)]
struct Field {
    access_flags: u16,
    name: String,
    descriptor: String,
    constant_value_index: Option<u16>,
}

#[derive(Clone, Debug)]
struct Method {
    access_flags: u16,
    name: String,
    descriptor: String,
    code: Option<Code>,
}

#[derive(Clone, Debug)]
struct Code {
    max_locals: u16,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct ResolvedMember {
    class: String,
    name: String,
    descriptor: String,
}

#[derive(Clone, Debug)]
struct ResolvedInvokeDynamic {
    bootstrap_method_attr_index: u16,
    name: String,
    descriptor: String,
}

#[derive(Clone, Debug)]
struct BootstrapMethod {
    method_ref: u16,
    arguments: Vec<u16>,
}

#[derive(Clone, Debug)]
enum Constant {
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
    fn parse(bytes: &[u8]) -> Result<Self> {
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
                    Constant::Utf8(String::from_utf8_lossy(bytes).to_string())
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

        let _access_flags = reader.u2()?;
        let this_class = reader.u2()?;
        let _super_class = reader.u2()?;

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

        Ok(Self {
            this_name,
            constants,
            fields,
            methods,
            bootstrap_methods,
        })
    }

    fn constant(&self, index: u16) -> Result<&Constant> {
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

    fn class_name(&self, index: u16) -> Result<String> {
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

    fn field_ref(&self, index: u16) -> Result<ResolvedMember> {
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

    fn method_ref(&self, index: u16) -> Result<ResolvedMember> {
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

    fn invoke_dynamic(&self, index: u16) -> Result<ResolvedInvokeDynamic> {
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

    fn bootstrap_method(&self, index: u16) -> Result<&BootstrapMethod> {
        self.bootstrap_methods
            .get(index as usize)
            .with_context(|| format!("invalid bootstrap method index {index}"))
    }

    fn method_handle_ref(&self, index: u16) -> Result<ResolvedMember> {
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

    fn string_constant(&self, index: u16) -> Result<String> {
        match self.constant(index)? {
            Constant::String { string_index } => Ok(self.utf8(*string_index)?.to_string()),
            other => bail!("constant {index} is not String: {other:?}"),
        }
    }

    fn int_constant(&self, index: u16) -> Result<i32> {
        match self.constant(index)? {
            Constant::Integer(value) => Ok(*value),
            other => bail!("constant {index} is not Integer: {other:?}"),
        }
    }

    fn initial_static_values(&self) -> Result<HashMap<StaticFieldKey, Value>> {
        let mut statics = HashMap::new();
        for field in &self.fields {
            let Some(index) = field.constant_value_index else {
                continue;
            };
            if field.access_flags & 0x0008 == 0 {
                continue;
            }
            statics.insert(
                static_field_key(&self.this_name, &field.name, &field.descriptor),
                self.field_constant_value(field, index)?,
            );
        }
        Ok(statics)
    }

    fn field_constant_value(&self, field: &Field, index: u16) -> Result<Value> {
        match field.descriptor.as_str() {
            "B" => Ok(Value::Int(self.int_constant(index)? as i8 as i32)),
            "S" => Ok(Value::Int(self.int_constant(index)? as i16 as i32)),
            "I" => Ok(Value::Int(self.int_constant(index)?)),
            "Z" => Ok(Value::Bool(self.int_constant(index)? != 0)),
            "C" => Ok(Value::Char(char_from_i32(self.int_constant(index)?)?)),
            "Ljava/lang/String;" => Ok(Value::String(self.string_constant(index)?.into_bytes())),
            other => bail!(
                "fvm-aot unsupported ConstantValue static field {}:{}",
                field.name,
                other
            ),
        }
    }

    fn static_field(&self, name: &str, descriptor: &str) -> Result<&Field> {
        let field = self
            .fields
            .iter()
            .find(|field| field.name == name && field.descriptor == descriptor)
            .with_context(|| format!("static field {name}:{descriptor} not found"))?;
        if field.access_flags & 0x0008 == 0 {
            bail!("fvm-aot field {name}:{descriptor} is not static");
        }
        supported_field_descriptor(descriptor)?;
        Ok(field)
    }

    fn instance_field(&self, name: &str, descriptor: &str) -> Result<&Field> {
        let field = self
            .fields
            .iter()
            .find(|field| field.name == name && field.descriptor == descriptor)
            .with_context(|| format!("instance field {name}:{descriptor} not found"))?;
        if field.access_flags & 0x0008 != 0 {
            bail!("fvm-aot field {name}:{descriptor} is static, not an instance field");
        }
        supported_field_descriptor(descriptor)?;
        Ok(field)
    }

    fn static_method(&self, name: &str, descriptor: &str) -> Result<&Method> {
        let method = self.method(name, descriptor)?;
        if method.access_flags & 0x0008 == 0 {
            bail!("fvm-aot method {name}{descriptor} is not static");
        }
        Ok(method)
    }

    fn method(&self, name: &str, descriptor: &str) -> Result<&Method> {
        self.methods
            .iter()
            .find(|method| method.name == name && method.descriptor == descriptor)
            .with_context(|| format!("method {name}{descriptor} not found"))
    }

    fn static_method_opt(&self, name: &str, descriptor: &str) -> Option<&Method> {
        self.methods
            .iter()
            .find(|method| method.name == name && method.descriptor == descriptor)
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
                code = Some(Code {
                    max_locals,
                    bytes: code_reader.bytes(code_length)?.to_vec(),
                });
                let exception_table_length = code_reader.u2()? as usize;
                code_reader.skip(exception_table_length * 8)?;
                skip_attributes(&mut code_reader, constants)?;
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

fn skip_attributes(reader: &mut Reader<'_>, _constants: &[Option<Constant>]) -> Result<()> {
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

fn read_opcode(code: &[u8], pc: &mut usize) -> Result<u8> {
    read_code_u8(code, pc)
}

fn read_code_u8(code: &[u8], pc: &mut usize) -> Result<u8> {
    if *pc >= code.len() {
        bail!("truncated bytecode at pc {pc}");
    }
    let value = code[*pc];
    *pc += 1;
    Ok(value)
}

fn read_code_u16(code: &[u8], pc: &mut usize) -> Result<u16> {
    let high = read_code_u8(code, pc)?;
    let low = read_code_u8(code, pc)?;
    Ok(u16::from_be_bytes([high, low]))
}

fn read_code_i16(code: &[u8], pc: &mut usize) -> Result<i16> {
    let high = read_code_u8(code, pc)?;
    let low = read_code_u8(code, pc)?;
    Ok(i16::from_be_bytes([high, low]))
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    #[test]
    fn rejects_invalid_classfile() {
        let err = ClassFile::parse(b"nope").unwrap_err();
        assert!(err.to_string().contains("truncated") || err.to_string().contains("magic"));
    }

    #[test]
    fn compiles_simple_println_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotHello.java");
        std::fs::write(
            &src,
            r#"public final class AotHello {
    public static void main(String[] args) {
        System.out.println("hello fvm-aot");
    }
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("hello.jar");
        write_test_jar(&jar, &classes_dir.join("AotHello.class"));
        let output = temp.path().join("hello-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotHello".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let run = Command::new(output).output().unwrap();
        assert!(run.status.success());
        assert_eq!(String::from_utf8_lossy(&run.stdout), "hello fvm-aot\n");
    }

    #[test]
    fn compiles_computed_http_intrinsic_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotHttpEval.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &src,
            r#"import fvm.runtime.Http;

public final class AotHttpEval {
    static int port() {
        int base = 19000;
        int offset = 91;
        if (offset > 0) {
            return base + offset;
        }
        return 1;
    }

    static String body() {
        return "computed fvm-aot http";
    }

    public static void main(String[] args) {
        Http.respond(port(), body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("http.jar");
        write_test_jar_entries(
            &jar,
            "AotHttpEval",
            &[
                ("AotHttpEval.class", classes_dir.join("AotHttpEval.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("http-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotHttpEval".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19091);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("computed fvm-aot http"));
    }

    #[test]
    fn compiles_static_fields_and_clinit_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotStatic.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &src,
            r#"import fvm.runtime.Http;

public final class AotStatic {
    static int base = 19000;
    static int offset;
    static String body;

    static {
        offset = 92;
        body = "static fvm-aot http";
    }

    static int port() {
        return base + offset;
    }

    public static void main(String[] args) {
        Http.respond(port(), body);
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("static.jar");
        write_test_jar_entries(
            &jar,
            "AotStatic",
            &[
                ("AotStatic.class", classes_dir.join("AotStatic.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("static-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotStatic".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19092);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("static fvm-aot http"));
    }

    #[test]
    fn compiles_objects_and_arrays_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let src = src_dir.join("AotObjects.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &src,
            r#"import fvm.runtime.Http;

public final class AotObjects {
    int base;
    int[] offsets;
    String[] bodies;

    AotObjects(int base, String body) {
        this.base = base;
        this.offsets = new int[] { 40, 50 };
        this.bodies = new String[] { body };
    }

    int port() {
        return base + offsets[0] + offsets[1] + offsets.length - 2;
    }

    String body() {
        return bodies[0];
    }

    public static void main(String[] args) {
        AotObjects app = new AotObjects(19000, "object array fvm-aot http");
        Http.respond(app.port(), app.body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&src)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("objects.jar");
        write_test_jar_entries(
            &jar,
            "AotObjects",
            &[
                ("AotObjects.class", classes_dir.join("AotObjects.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("objects-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotObjects".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19090);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("object array fvm-aot http"));
    }

    #[test]
    fn compiles_multi_class_closed_world_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let app = src_dir.join("AotMulti.java");
        let config = src_dir.join("AotConfig.java");
        let handler = src_dir.join("AotHandler.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &app,
            r#"import fvm.runtime.Http;

public final class AotMulti {
    public static void main(String[] args) {
        AotConfig config = new AotConfig(19003, "multi class fvm-aot http");
        AotHandler handler = new AotHandler(config);
        Http.respond(handler.port(), handler.body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &config,
            r#"public final class AotConfig {
    int base;
    int[] offsets;
    String body;

    AotConfig(int base, String body) {
        this.base = base;
        this.offsets = new int[] { 30, 60 };
        this.body = body;
    }

    int port() {
        return base + offsets[0] + offsets[1];
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &handler,
            r#"public final class AotHandler {
    AotConfig config;
    String[] bodies;

    AotHandler(AotConfig config) {
        this.config = config;
        this.bodies = new String[] { config.body };
    }

    int port() {
        return config.port();
    }

    String body() {
        return bodies[0];
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&app)
            .arg(&config)
            .arg(&handler)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("multi.jar");
        write_test_jar_entries(
            &jar,
            "AotMulti",
            &[
                ("AotMulti.class", classes_dir.join("AotMulti.class")),
                ("AotConfig.class", classes_dir.join("AotConfig.class")),
                ("AotHandler.class", classes_dir.join("AotHandler.class")),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("multi-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotMulti".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19093);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("multi class fvm-aot http"));
    }

    #[test]
    fn compiles_interface_dispatch_and_string_concat_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let app = src_dir.join("AotDispatch.java");
        let responder = src_dir.join("AotResponder.java");
        let config = src_dir.join("AotDispatchConfig.java");
        let handler = src_dir.join("AotDispatchHandler.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &app,
            r#"import fvm.runtime.Http;

public final class AotDispatch {
    public static void main(String[] args) {
        AotResponder responder = new AotDispatchHandler(new AotDispatchConfig(19000, 94, "fvm"));
        Http.respond(responder.port(), responder.body());
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &responder,
            r#"public interface AotResponder {
    int port();
    String body();
}
"#,
        )
        .unwrap();
        std::fs::write(
            &config,
            r#"public final class AotDispatchConfig {
    int base;
    int offset;
    String name;

    AotDispatchConfig(int base, int offset, String name) {
        this.base = base;
        this.offset = offset;
        this.name = name;
    }

    int port() {
        return base + offset;
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &handler,
            r#"public final class AotDispatchHandler implements AotResponder {
    AotDispatchConfig config;

    AotDispatchHandler(AotDispatchConfig config) {
        this.config = config;
    }

    public int port() {
        return config.port();
    }

    public String body() {
        return "dispatch " + config.name + " #" + port();
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&app)
            .arg(&responder)
            .arg(&config)
            .arg(&handler)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("dispatch.jar");
        write_test_jar_entries(
            &jar,
            "AotDispatch",
            &[
                ("AotDispatch.class", classes_dir.join("AotDispatch.class")),
                ("AotResponder.class", classes_dir.join("AotResponder.class")),
                (
                    "AotDispatchConfig.class",
                    classes_dir.join("AotDispatchConfig.class"),
                ),
                (
                    "AotDispatchHandler.class",
                    classes_dir.join("AotDispatchHandler.class"),
                ),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("dispatch-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotDispatch".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19094);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("dispatch fvm #19094"));
    }

    #[test]
    fn compiles_string_object_array_core_methods_when_toolchain_is_available() {
        if !command_available("javac") || !command_available("cc") {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let runtime_dir = src_dir.join("fvm/runtime");
        let classes_dir = temp.path().join("classes");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::create_dir_all(&classes_dir).unwrap();
        let app = src_dir.join("AotCoreMethods.java");
        let http = runtime_dir.join("Http.java");
        std::fs::write(
            &app,
            r#"import fvm.runtime.Http;

public final class AotCoreMethods {
    static boolean enabled = true;
    static char marker = '!';

    int value;

    AotCoreMethods(int value) {
        this.value = value;
    }

    public static void main(String[] args) {
        String base = "fvm-core";
        String suffix = base.substring(4);
        boolean stringOk = enabled
            && base.length() == 8
            && !base.isEmpty()
            && base.charAt(3) == '-'
            && base.startsWith("fvm")
            && base.endsWith("core")
            && base.contains("m-c")
            && base.equals("fvm-core")
            && suffix.equals("core");

        AotCoreMethods app = new AotCoreMethods(7);
        Object same = app;
        Object sameAgain = app;
        Object other = new AotCoreMethods(7);
        boolean objectOk = same.equals(app)
            && !same.equals(other)
            && same.hashCode() == sameAgain.hashCode()
            && same.toString().startsWith("AotCoreMethods@");

        int[] ports = new int[] { 19000, 95 };
        int[] cloned = ports.clone();
        boolean arrayOk = !ports.equals(cloned)
            && ports.hashCode() != cloned.hashCode()
            && ports.toString().startsWith("[I@");

        String body = base + " " + suffix + " " + stringOk + " " + objectOk + " " + arrayOk + " " + marker;
        Http.respond(ports[0] + cloned[1], body);
    }
}
"#,
        )
        .unwrap();
        std::fs::write(
            &http,
            r#"package fvm.runtime;

public final class Http {
    private Http() {}
    public static void respond(int port, String body) {}
}
"#,
        )
        .unwrap();

        let javac = Command::new("javac")
            .arg("--release")
            .arg("17")
            .arg("-d")
            .arg(&classes_dir)
            .arg(&app)
            .arg(&http)
            .status()
            .unwrap();
        if !javac.success() {
            return;
        }

        let jar = temp.path().join("core-methods.jar");
        write_test_jar_entries(
            &jar,
            "AotCoreMethods",
            &[
                (
                    "AotCoreMethods.class",
                    classes_dir.join("AotCoreMethods.class"),
                ),
                (
                    "fvm/runtime/Http.class",
                    classes_dir.join("fvm/runtime/Http.class"),
                ),
            ],
        );
        let output = temp.path().join("core-methods-native");
        compile_jar(&CompileSpec {
            jar_path: jar,
            main_class: Some("AotCoreMethods".to_string()),
            output_path: output.clone(),
            cc: "cc".to_string(),
            dry_run: false,
        })
        .unwrap();

        let mut child = Command::new(&output).spawn().unwrap();
        let response = wait_http_response(19095);
        let _ = child.kill();
        let _ = child.wait();
        let response = response.unwrap();
        assert!(response.contains("HTTP/1.1 200 OK"));
        assert!(response.ends_with("fvm-core core true true true !"));
    }

    fn command_available(name: &str) -> bool {
        Command::new(name).arg("--version").output().is_ok()
    }

    fn write_test_jar(path: &Path, class_file: &Path) {
        write_test_jar_entries(
            path,
            "AotHello",
            &[("AotHello.class", class_file.to_path_buf())],
        );
    }

    fn write_test_jar_entries(path: &Path, main_class: &str, entries: &[(&str, PathBuf)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(format!("Manifest-Version: 1.0\nMain-Class: {main_class}\n").as_bytes())
            .unwrap();
        for (name, path) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(&std::fs::read(path).unwrap()).unwrap();
        }
        zip.finish().unwrap();
    }

    fn wait_http_response(port: u16) -> Result<String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
                stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
                let mut response = String::new();
                stream.read_to_string(&mut response)?;
                return Ok(response);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        bail!("timed out waiting for generated HTTP server on {port}")
    }
}
