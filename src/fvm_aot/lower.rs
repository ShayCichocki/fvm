// allow: SIZE_OK - straight-line bytecode lowering is one opcode state machine before T15 splits branches.
use super::classfile::{ClassFile, Code, Method};
use super::diagnostics::unsupported_opcode_message;
use super::ir::{
    BasicBlockId, BasicBlockIr, FunctionIr, IrArithmeticOp, IrConst, IrInstr, IrParam, IrType,
    IrUnaryOp, TrapReason, ValueId,
};
use super::types::{JvmType, parse_method_descriptor};
use anyhow::{Context, Result, bail};

pub(super) fn lower_method_to_ir(class_file: &ClassFile, method: &Method) -> Result<FunctionIr> {
    if method.access_flags & 0x0008 == 0 {
        bail!(
            "fvm-aot lowerer needs static method: {}",
            method_label(class_file, method)
        );
    }
    let Some(code) = method.code.as_ref() else {
        bail!(
            "fvm-aot lowerer requires Code for {}",
            method_label(class_file, method)
        );
    };
    let (param_types, return_type) = parse_method_descriptor(&method.descriptor)?;
    Lowerer::new(class_file, method, code).lower(&param_types, &return_type)
}

struct Lowerer<'a> {
    class_file: &'a ClassFile,
    method: &'a Method,
    code: &'a Code,
    pc: usize,
    next_value: u32,
    locals: Vec<Option<ValueId>>,
    stack: Vec<ValueId>,
    instrs: Vec<IrInstr>,
}

impl<'a> Lowerer<'a> {
    fn new(class_file: &'a ClassFile, method: &'a Method, code: &'a Code) -> Self {
        Self {
            class_file,
            method,
            code,
            pc: 0,
            next_value: 0,
            locals: vec![None; usize::from(code.max_locals)],
            stack: Vec::new(),
            instrs: Vec::new(),
        }
    }

    fn lower(mut self, param_types: &[JvmType], return_type: &JvmType) -> Result<FunctionIr> {
        let params = self.lower_params(param_types)?;
        let return_type = self.ir_type_for_jvm(return_type, "return")?;
        let mut returned = false;
        while self.pc < self.code.bytes.len() && !returned {
            let opcode_pc = self.pc;
            let opcode = self.read_u8()?;
            returned = self.lower_opcode(opcode).with_context(|| {
                format!(
                    "fvm-aot lowerer bytecode error in {} at bci {} (opcode 0x{:02x})",
                    method_label(self.class_file, self.method),
                    opcode_pc,
                    opcode
                )
            })?;
        }
        if !returned {
            bail!(
                "fvm-aot lowerer method {} ended without return",
                method_label(self.class_file, self.method)
            );
        }
        let ir = FunctionIr {
            name: ir_name(self.class_file, self.method),
            params,
            return_type,
            blocks: vec![BasicBlockIr {
                id: BasicBlockId::new(0),
                instrs: self.instrs,
            }],
        };
        ir.verify()?;
        Ok(ir)
    }

    fn lower_params(&mut self, param_types: &[JvmType]) -> Result<Vec<IrParam>> {
        let mut params = Vec::with_capacity(param_types.len());
        for (local, ty) in param_types.iter().enumerate() {
            let ty = self.ir_type_for_jvm(ty, "parameter")?;
            let value = self.new_value();
            let local = u16::try_from(local).context("fvm-aot lowerer local index exceeded u16")?;
            self.store_local(local, value);
            self.instrs.push(IrInstr::Param(value, local, ty.clone()));
            params.push(IrParam { value, ty });
        }
        Ok(params)
    }

    fn lower_opcode(&mut self, opcode: u8) -> Result<bool> {
        match opcode {
            0x02 => self.push_int(-1),
            0x03..=0x08 => self.push_int(i32::from(opcode - 0x03)),
            0x10 => {
                let value = i32::from(i8::from_be_bytes([self.read_u8()?]));
                self.push_int(value);
            }
            0x11 => {
                let value = i32::from(self.read_i16()?);
                self.push_int(value);
            }
            0x12 | 0x13 => {
                let index = if opcode == 0x12 {
                    u16::from(self.read_u8()?)
                } else {
                    self.read_u16()?
                };
                self.push_int_constant(index)?;
            }
            0x15 | 0x1a..=0x1d => {
                let index = if opcode == 0x15 {
                    u16::from(self.read_u8()?)
                } else {
                    u16::from(opcode - 0x1a)
                };
                let value = self.load_local(index)?;
                self.stack.push(value);
            }
            0x36 | 0x3b..=0x3e => {
                let index = if opcode == 0x36 {
                    u16::from(self.read_u8()?)
                } else {
                    u16::from(opcode - 0x3b)
                };
                let value = self.pop_stack()?;
                self.store_local(index, value);
            }
            0x60 => self.push_binary(IrArithmeticOp::Add)?,
            0x64 => self.push_binary(IrArithmeticOp::Sub)?,
            0x68 => self.push_binary(IrArithmeticOp::Mul)?,
            0x6c => self.push_checked_binary(IrArithmeticOp::Div)?,
            0x70 => self.push_checked_binary(IrArithmeticOp::Rem)?,
            0x74 => self.push_unary_neg()?,
            0x84 => {
                let index = u16::from(self.read_u8()?);
                let delta = i32::from(i8::from_be_bytes([self.read_u8()?]));
                self.increment_local(index, delta)?;
            }
            0xac | 0xb1 => {
                let value = if opcode == 0xac {
                    Some(self.pop_stack()?)
                } else {
                    None
                };
                self.instrs.push(IrInstr::Return(value));
                return Ok(true);
            }
            other => bail!("{}", unsupported_opcode_message(other)),
        }
        Ok(false)
    }

    fn increment_local(&mut self, index: u16, delta: i32) -> Result<()> {
        let lhs = self.load_local(index)?;
        let rhs = self.new_value();
        self.instrs
            .push(IrInstr::Constant(rhs, IrConst::Int(delta)));
        let value = self.new_value();
        self.instrs
            .push(IrInstr::Arithmetic(value, IrArithmeticOp::Add, lhs, rhs));
        self.store_local(index, value);
        Ok(())
    }

    fn push_int_constant(&mut self, index: u16) -> Result<()> {
        let value = self.class_file.int_constant(index).with_context(|| {
            format!("fvm-aot lowerer only supports integer ldc constants at index {index}")
        })?;
        self.push_int(value);
        Ok(())
    }

    fn push_int(&mut self, value: i32) {
        let id = self.new_value();
        self.instrs.push(IrInstr::Constant(id, IrConst::Int(value)));
        self.stack.push(id);
    }

    fn push_binary(&mut self, op: IrArithmeticOp) -> Result<()> {
        let rhs = self.pop_stack()?;
        let lhs = self.pop_stack()?;
        let value = self.new_value();
        self.instrs.push(IrInstr::Arithmetic(value, op, lhs, rhs));
        self.stack.push(value);
        Ok(())
    }

    fn push_checked_binary(&mut self, op: IrArithmeticOp) -> Result<()> {
        let rhs = self.pop_stack()?;
        let lhs = self.pop_stack()?;
        self.instrs
            .push(IrInstr::ZeroCheck(rhs, TrapReason::DivideByZero));
        let value = self.new_value();
        self.instrs.push(IrInstr::Arithmetic(value, op, lhs, rhs));
        self.stack.push(value);
        Ok(())
    }

    fn push_unary_neg(&mut self) -> Result<()> {
        let input = self.pop_stack()?;
        let value = self.new_value();
        self.instrs
            .push(IrInstr::Unary(value, IrUnaryOp::Neg, input));
        self.stack.push(value);
        Ok(())
    }

    fn load_local(&self, index: u16) -> Result<ValueId> {
        self.locals
            .get(usize::from(index))
            .and_then(|value| *value)
            .with_context(|| format!("fvm-aot lowerer read uninitialized local {index}"))
    }

    fn store_local(&mut self, index: u16, value: ValueId) {
        let index = usize::from(index);
        if index >= self.locals.len() {
            self.locals.resize(index + 1, None);
        }
        self.locals[index] = Some(value);
    }

    fn pop_stack(&mut self) -> Result<ValueId> {
        self.stack.pop().context("fvm-aot lowerer stack underflow")
    }

    fn new_value(&mut self) -> ValueId {
        let value = ValueId::new(self.next_value);
        self.next_value += 1;
        value
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.pc >= self.code.bytes.len() {
            bail!("truncated bytecode at pc {}", self.pc);
        }
        let value = self.code.bytes[self.pc];
        self.pc += 1;
        Ok(value)
    }

    fn read_u16(&mut self) -> Result<u16> {
        let high = self.read_u8()?;
        let low = self.read_u8()?;
        Ok(u16::from_be_bytes([high, low]))
    }

    fn read_i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.read_u16()?.to_be_bytes()))
    }

    fn ir_type_for_jvm(&self, ty: &JvmType, role: &str) -> Result<IrType> {
        match ty {
            JvmType::Int => Ok(IrType::Int),
            JvmType::Boolean => Ok(IrType::Boolean),
            JvmType::Char => Ok(IrType::Char),
            JvmType::Void => Ok(IrType::Void),
            JvmType::String | JvmType::Object(_) | JvmType::Array(_) => bail!(
                "fvm-aot lowerer unsupported {role} reference type in {}; planned milestone: runtime-object-model",
                method_label(self.class_file, self.method)
            ),
            JvmType::Unsupported => bail!(
                "fvm-aot lowerer unsupported {role} primitive type in {}; required feature: primitive bytecode; planned milestone: primitive-completeness",
                method_label(self.class_file, self.method)
            ),
        }
    }
}

fn method_label(class_file: &ClassFile, method: &Method) -> String {
    format!(
        "{}.{}{}",
        class_file.this_name.replace('/', "."),
        method.name,
        method.descriptor
    )
}

fn ir_name(class_file: &ClassFile, method: &Method) -> String {
    format!("{}.{}", class_file.this_name.replace('/', "."), method.name)
}
