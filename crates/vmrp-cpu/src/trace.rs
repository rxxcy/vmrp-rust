use crate::ExecutionMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterWrite {
    pub index: usize,
    pub value: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepTrace {
    pub pc: u32,
    pub mode: ExecutionMode,
    pub opcode: u32,
    pub register_writes: Vec<RegisterWrite>,
}
