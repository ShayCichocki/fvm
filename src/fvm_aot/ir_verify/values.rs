use super::{BasicBlockId, IrType, ValueId};
use anyhow::{Result, bail};
use std::collections::HashMap;

pub(super) struct ValueTypes {
    label: String,
    values: HashMap<ValueId, IrType>,
}

impl ValueTypes {
    pub(super) fn new(label: String) -> Self {
        Self {
            label,
            values: HashMap::new(),
        }
    }

    pub(super) fn define(
        &mut self,
        value: ValueId,
        ty: IrType,
        allow_existing: bool,
    ) -> Result<()> {
        if let Some(existing) = self.values.get(&value) {
            if allow_existing && existing == &ty {
                return Ok(());
            }
            bail!(
                "IR function `{}` defines {value} more than once",
                self.label
            );
        }
        self.values.insert(value, ty);
        Ok(())
    }

    pub(super) fn use_value(&self, block: BasicBlockId, value: ValueId) -> Result<IrType> {
        self.values.get(&value).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "IR function `{}` uses {value} in {block} before definition",
                self.label
            )
        })
    }

    pub(super) fn require_int_like(&self, block: BasicBlockId, value: ValueId) -> Result<IrType> {
        let ty = self.use_value(block, value)?;
        if matches!(ty, IrType::Int | IrType::Boolean | IrType::Char) {
            return Ok(ty);
        }
        bail!(
            "IR function `{}` expected int-compatible value for {value} in {block}, got {ty}",
            self.label
        )
    }
}
