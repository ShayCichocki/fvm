use super::ir::{
    BasicBlockId, FunctionIr, IrConst, IrInstr, IrType, MethodRef, RuntimeHelper, TrapReason,
    ValueId,
};
mod types;
mod values;

use anyhow::{Result, bail};
use std::collections::HashSet;
use types::{
    constant_type, descriptor_return_type, return_compatible, runtime_return_type,
    verify_descriptor_model_return, verify_supported_trap, verify_supported_type,
};
use values::ValueTypes;

pub(super) fn verify_function(function: &FunctionIr) -> Result<()> {
    Verifier::new(function)?.verify()
}

struct Verifier<'a> {
    function: &'a FunctionIr,
    block_ids: HashSet<BasicBlockId>,
    values: ValueTypes,
}

impl<'a> Verifier<'a> {
    fn new(function: &'a FunctionIr) -> Result<Self> {
        let mut block_ids = HashSet::new();
        for block in &function.blocks {
            if !block_ids.insert(block.id) {
                bail!(
                    "IR function `{}` declares duplicate block {}",
                    label(function),
                    block.id
                );
            }
        }
        Ok(Self {
            function,
            block_ids,
            values: ValueTypes::new(label(function)),
        })
    }

    fn verify(mut self) -> Result<()> {
        self.seed_params()?;
        self.verify_type("return type", &self.function.return_type)?;
        verify_descriptor_model_return(
            &self.label(),
            &self.function.descriptor,
            &self.function.return_type,
        )?;
        for block in &self.function.blocks {
            for instr in &block.instrs {
                self.verify_instr(block.id, instr)?;
            }
        }
        Ok(())
    }

    fn seed_params(&mut self) -> Result<()> {
        for param in &self.function.params {
            self.verify_type("parameter type", &param.ty)?;
            self.define_value(param.value, param.ty.clone(), true)?;
        }
        Ok(())
    }

    fn verify_instr(&mut self, block: BasicBlockId, instr: &IrInstr) -> Result<()> {
        match instr {
            IrInstr::Param(value, _local, ty) => {
                self.verify_type("parameter type", ty)?;
                self.define_value(*value, ty.clone(), true)
            }
            IrInstr::Constant(value, constant) => {
                self.define_value(*value, constant_type(constant), false)
            }
            IrInstr::Compare(value, _op, lhs, rhs) => {
                self.values.use_value(block, *lhs)?;
                if let Some(rhs) = rhs {
                    self.values.use_value(block, *rhs)?;
                }
                self.define_value(*value, IrType::Boolean, false)
            }
            IrInstr::Arithmetic(value, _op, lhs, rhs) => {
                self.values.require_int_like(block, *lhs)?;
                self.values.require_int_like(block, *rhs)?;
                self.define_value(*value, IrType::Int, false)
            }
            IrInstr::Unary(value, _op, input) => {
                self.values.require_int_like(block, *input)?;
                self.define_value(*value, IrType::Int, false)
            }
            IrInstr::Branch(target) => self.verify_target(block, *target),
            IrInstr::CondBranch(condition, then_target, else_target) => {
                self.values.use_value(block, *condition)?;
                self.verify_target(block, *then_target)?;
                self.verify_target(block, *else_target)
            }
            IrInstr::Call(value, method, args) => self.verify_call(block, *value, method, args),
            IrInstr::RuntimeCall(value, helper, args) => {
                for arg in args {
                    self.values.use_value(block, *arg)?;
                }
                match (value, runtime_return_type(helper)) {
                    (Some(value), Some(ty)) => self.define_value(*value, ty, false),
                    (None, None) => Ok(()),
                    (Some(_), None) | (None, Some(_)) => bail!(
                        "IR function `{}` has runtime helper return mismatch in {block}",
                        self.label()
                    ),
                }
            }
            IrInstr::Return(value) => self.verify_return(block, *value),
            IrInstr::FieldGet(value, field, receiver) => {
                if let Some(receiver) = receiver {
                    self.values.use_value(block, *receiver)?;
                }
                self.verify_type("field type", &field.ty)?;
                self.define_value(*value, field.ty.clone(), false)
            }
            IrInstr::FieldPut(field, receiver, value) => {
                if let Some(receiver) = receiver {
                    self.values.use_value(block, *receiver)?;
                }
                self.verify_type("field type", &field.ty)?;
                self.values.use_value(block, *value).map(|_| ())
            }
            IrInstr::ArrayLoad(value, array, index, element) => {
                self.values.use_value(block, *array)?;
                self.values.require_int_like(block, *index)?;
                self.verify_type("array element type", element)?;
                self.define_value(*value, element.clone(), false)
            }
            IrInstr::ArrayStore(array, index, value, element) => {
                self.values.use_value(block, *array)?;
                self.values.require_int_like(block, *index)?;
                self.verify_type("array element type", element)?;
                self.values.use_value(block, *value).map(|_| ())
            }
            IrInstr::ArrayLength(value, array) => {
                self.values.use_value(block, *array)?;
                self.define_value(*value, IrType::Int, false)
            }
            IrInstr::NewObject(value, class) => {
                self.define_value(*value, IrType::Object(class.clone()), false)
            }
            IrInstr::NewArray(value, element, length) => {
                self.verify_type("array element type", element)?;
                self.values.require_int_like(block, *length)?;
                self.define_value(*value, IrType::Array(Box::new(element.clone())), false)
            }
            IrInstr::ZeroCheck(value, reason) => {
                self.verify_trap(reason)?;
                self.values.require_int_like(block, *value).map(|_| ())
            }
            IrInstr::NullCheck(value, reason) => {
                self.verify_trap(reason)?;
                self.values.use_value(block, *value).map(|_| ())
            }
            IrInstr::BoundsCheck(index, length, reason) => {
                self.verify_trap(reason)?;
                self.values.require_int_like(block, *index)?;
                self.values.require_int_like(block, *length).map(|_| ())
            }
            IrInstr::ExceptionEdge(exception, target) => {
                self.values.use_value(block, *exception)?;
                self.verify_target(block, *target)
            }
            IrInstr::Trap(reason) => self.verify_trap(reason),
        }
    }

    fn verify_call(
        &mut self,
        block: BasicBlockId,
        value: Option<ValueId>,
        method: &MethodRef,
        args: &[ValueId],
    ) -> Result<()> {
        for arg in args {
            self.values.use_value(block, *arg)?;
        }
        let return_type = descriptor_return_type(&method.descriptor)?;
        self.verify_type("call return type", &return_type)?;
        match (value, return_type) {
            (Some(_), IrType::Void) => bail!(
                "IR function `{}` has call return mismatch for {}.{}{} in {block}",
                self.label(),
                method.class,
                method.name,
                method.descriptor
            ),
            (None, IrType::Void) => Ok(()),
            (None, _) => bail!(
                "IR function `{}` has call return mismatch for {}.{}{} in {block}",
                self.label(),
                method.class,
                method.name,
                method.descriptor
            ),
            (Some(value), ty) => self.define_value(value, ty, false),
        }
    }

    fn verify_return(&self, block: BasicBlockId, value: Option<ValueId>) -> Result<()> {
        match (&self.function.return_type, value) {
            (IrType::Void, None) => Ok(()),
            (IrType::Void, Some(value)) => bail!(
                "IR function `{}` return type mismatch in {block}: expected void, returned {}",
                self.label(),
                value
            ),
            (expected, None) => bail!(
                "IR function `{}` return type mismatch in {block}: expected {expected}, returned void",
                self.label()
            ),
            (expected, Some(value)) => {
                let actual = self.values.use_value(block, value)?;
                if return_compatible(expected, &actual) {
                    return Ok(());
                }
                bail!(
                    "IR function `{}` return type mismatch in {block}: expected {expected}, returned {actual}",
                    self.label()
                )
            }
        }
    }

    fn verify_target(&self, source: BasicBlockId, target: BasicBlockId) -> Result<()> {
        if self.block_ids.contains(&target) {
            return Ok(());
        }
        bail!(
            "IR function `{}` branches from {source} to missing target {target}",
            self.label()
        )
    }

    fn define_value(&mut self, value: ValueId, ty: IrType, allow_existing: bool) -> Result<()> {
        self.verify_type("value type", &ty)?;
        self.values.define(value, ty, allow_existing)
    }

    fn verify_type(&self, role: &str, ty: &IrType) -> Result<()> {
        verify_supported_type(&self.label(), role, ty)
    }

    fn verify_trap(&self, reason: &TrapReason) -> Result<()> {
        verify_supported_trap(&self.label(), reason)
    }

    fn label(&self) -> String {
        label(self.function)
    }
}

fn label(function: &FunctionIr) -> String {
    format!("{}{}", function.name, function.descriptor)
}
