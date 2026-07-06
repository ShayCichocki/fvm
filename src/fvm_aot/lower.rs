mod branches;
mod bytecode;
mod calls;
mod metadata;
mod method;
mod state;

pub(super) use method::lower_method_to_ir;
