use super::{BasicBlockId, IrType, ValueId};
use anyhow::{Result, bail};
use std::collections::{HashMap, HashSet};

/// Tracks value definitions with CFG-aware scoping.
///
/// Every value is defined at most once across the whole function (SSA
/// single-assignment). A value may only be *used* in the block that defines it;
/// cross-block dataflow travels exclusively through block parameters (the phi
/// equivalent), so a value produced in one block is out of scope in every other
/// block. Function parameters are the sole exception: the entry block dominates
/// every block, so they stay in scope everywhere.
///
/// This replaces the previous global value map, which silently accepted a value
/// defined in one block being used in an unrelated sibling — the exact
/// miscompile shape the block-parameter IR (Phase 1, P1.1) exists to eliminate.
pub(super) struct ValueScope {
    label: String,
    /// Type of every value defined anywhere in the function. Doubles as the
    /// single-assignment registry: a value present here is already defined.
    types: HashMap<ValueId, IrType>,
    /// Values that dominate every block (function parameters).
    global: HashSet<ValueId>,
    /// Values usable in the block currently being verified.
    in_scope: HashSet<ValueId>,
}

impl ValueScope {
    pub(super) fn new(label: String) -> Self {
        Self {
            label,
            types: HashMap::new(),
            global: HashSet::new(),
            in_scope: HashSet::new(),
        }
    }

    /// Define a value that is in scope in every block (a function parameter).
    pub(super) fn define_global(&mut self, value: ValueId, ty: IrType) -> Result<()> {
        if self.types.contains_key(&value) {
            bail!(
                "IR function `{}` defines {value} more than once",
                self.label
            );
        }
        self.types.insert(value, ty);
        self.global.insert(value);
        self.in_scope.insert(value);
        Ok(())
    }

    /// Reset the current scope to the globally-visible values before verifying a
    /// new block. Block parameters and locally-defined values are added on top.
    pub(super) fn enter_block(&mut self) {
        self.in_scope = self.global.clone();
    }

    /// Define a value produced within the current block (a block parameter or an
    /// instruction result). `allow_existing` tolerates re-declaring a value with
    /// the same type, which the entry block does when it re-emits function
    /// parameters as `Param` instructions.
    pub(super) fn define(
        &mut self,
        value: ValueId,
        ty: IrType,
        allow_existing: bool,
    ) -> Result<()> {
        match self.types.get(&value) {
            Some(existing) if allow_existing && existing == &ty => {}
            Some(_) => bail!(
                "IR function `{}` defines {value} more than once",
                self.label
            ),
            None => {
                self.types.insert(value, ty);
            }
        }
        self.in_scope.insert(value);
        Ok(())
    }

    pub(super) fn use_value(&self, block: BasicBlockId, value: ValueId) -> Result<IrType> {
        if self.in_scope.contains(&value) {
            return Ok(self.types[&value].clone());
        }
        if self.types.contains_key(&value) {
            bail!(
                "IR function `{}` uses {value} in {block} but it is out of scope there; a value defined in another block reaches this one only as a block parameter",
                self.label
            );
        }
        bail!(
            "IR function `{}` uses {value} in {block} before definition",
            self.label
        )
    }

    pub(super) fn require_int_like(&self, block: BasicBlockId, value: ValueId) -> Result<IrType> {
        let ty = self.use_value(block, value)?;
        if ty.is_int_like() {
            return Ok(ty);
        }
        bail!(
            "IR function `{}` expected int-compatible value for {value} in {block}, got {ty}",
            self.label
        )
    }

    pub(super) fn require_reference(&self, block: BasicBlockId, value: ValueId) -> Result<IrType> {
        let ty = self.use_value(block, value)?;
        if matches!(ty, IrType::Object(_) | IrType::Array(_)) {
            return Ok(ty);
        }
        bail!(
            "IR function `{}` expected reference value for {value} in {block}, got {ty}",
            self.label
        )
    }
}
