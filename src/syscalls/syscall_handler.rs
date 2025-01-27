use super::business_logic_syscall_handler::BusinessLogicSyscallHandler;
use crate::state::{contract_class_cache::ContractClassCache, state_api::StateReader};
use crate::transaction::error::TransactionError;
use cairo_lang_casm::{
    hints::{Hint, StarknetHint},
    operand::{CellRef, DerefOrImmediate, Register, ResOperand},
};
use cairo_vm::vm::runners::cairo_runner::{ResourceTracker, RunResources};
use cairo_vm::{
    hint_processor::{
        cairo_1_hint_processor::hint_processor::Cairo1HintProcessor,
        hint_processor_definition::{HintProcessorLogic, HintReference},
    },
    types::{
        errors::math_errors::MathError, exec_scope::ExecutionScopes, relocatable::Relocatable,
    },
    vm::{
        errors::{hint_errors::HintError, vm_errors::VirtualMachineError},
        vm_core::VirtualMachine,
    },
    Felt252,
};
use std::{any::Any, boxed::Box, collections::HashMap};

pub(crate) trait HintProcessorPostRun {
    /// Performs post run syscall related tasks (if any).
    fn post_run(
        &self,
        _runner: &mut VirtualMachine,
        _syscall_stop_ptr: Relocatable,
    ) -> Result<(), TransactionError>;
}

pub(crate) struct SyscallHintProcessor<'a, S: StateReader, C: ContractClassCache> {
    pub(crate) cairo1_hint_processor: Cairo1HintProcessor,
    pub(crate) syscall_handler: BusinessLogicSyscallHandler<'a, S, C>,
    pub(crate) run_resources: RunResources,
}

impl<'a, S: StateReader, C: ContractClassCache> SyscallHintProcessor<'a, S, C> {
    pub fn new(
        syscall_handler: BusinessLogicSyscallHandler<'a, S, C>,
        hints: &[(usize, Vec<Hint>)],
        run_resources: RunResources,
    ) -> Self {
        SyscallHintProcessor {
            cairo1_hint_processor: Cairo1HintProcessor::new(hints, run_resources.clone(), false),
            syscall_handler,
            run_resources,
        }
    }
}

impl<'a, S: StateReader, C: ContractClassCache> HintProcessorLogic
    for SyscallHintProcessor<'a, S, C>
{
    fn execute_hint(
        &mut self,
        vm: &mut VirtualMachine,
        exec_scopes: &mut ExecutionScopes,
        hint_data: &Box<dyn Any>,
        _constants: &HashMap<String, Felt252>,
    ) -> Result<(), HintError> {
        let hints: &Vec<Hint> = hint_data.downcast_ref().ok_or(HintError::WrongHintData)?;
        for hint in hints {
            match hint {
                Hint::Core(_core_hint) => {
                    self.cairo1_hint_processor.execute(vm, exec_scopes, hint)?
                }
                Hint::Starknet(starknet_hint) => match starknet_hint {
                    StarknetHint::SystemCall { system } => {
                        let syscall_ptr = as_relocatable(vm, system)?;
                        self.syscall_handler
                            .syscall(
                                vm,
                                syscall_ptr,
                                // TODO: Get the program_cache somehow.
                                #[cfg(feature = "cairo-native")]
                                None,
                            )
                            .map_err(|err| {
                                HintError::CustomHint(
                                    format!("Syscall handler invocation error: {err}")
                                        .into_boxed_str(),
                                )
                            })?;
                    }
                    other => {
                        return Err(HintError::UnknownHint(
                            format!("{:?}", other).into_boxed_str(),
                        ))
                    }
                },
            };
        }
        Ok(())
    }

    // Ignores all data except for the code that should contain
    fn compile_hint(
        &self,
        //Block of hint code as String
        hint_code: &str,
        //Ap Tracking Data corresponding to the Hint
        ap_tracking_data: &cairo_vm::serde::deserialize_program::ApTracking,
        //Map from variable name to reference id number
        //(may contain other variables aside from those used by the hint)
        reference_ids: &HashMap<String, usize>,
        //List of all references (key corresponds to element of the previous dictionary)
        references: &[HintReference],
    ) -> Result<Box<dyn Any>, VirtualMachineError> {
        self.cairo1_hint_processor.compile_hint(
            hint_code,
            ap_tracking_data,
            reference_ids,
            references,
        )
    }
}

impl<'a, S: StateReader, C: ContractClassCache> ResourceTracker for SyscallHintProcessor<'a, S, C> {
    fn consumed(&self) -> bool {
        self.run_resources.consumed()
    }

    fn consume_step(&mut self) {
        self.run_resources.consume_step()
    }

    fn get_n_steps(&self) -> Option<usize> {
        self.run_resources.get_n_steps()
    }

    fn run_resources(&self) -> &RunResources {
        &self.run_resources
    }
}

impl<'a, S: StateReader, C: ContractClassCache> HintProcessorPostRun
    for SyscallHintProcessor<'a, S, C>
{
    fn post_run(
        &self,
        runner: &mut VirtualMachine,
        syscall_stop_ptr: Relocatable,
    ) -> Result<(), crate::transaction::error::TransactionError> {
        self.syscall_handler.post_run(runner, syscall_stop_ptr)
    }
}

// TODO: These four functions were copied from cairo-rs in
// hint_processor/cairo-1-hint-processor/hint_processor_utils.rs as these functions are private.
// They will became public soon and then we have to remove this ones and use the ones in cairo-rs instead
fn as_relocatable(vm: &VirtualMachine, value: &ResOperand) -> Result<Relocatable, HintError> {
    let (base, offset) = extract_buffer(value)?;
    get_ptr(vm, base, &offset).map_err(HintError::from)
}

fn extract_buffer(buffer: &ResOperand) -> Result<(&CellRef, Felt252), HintError> {
    let (cell, base_offset) = match buffer {
        ResOperand::Deref(cell) => (cell, 0.into()),
        ResOperand::BinOp(bin_op) => {
            if let DerefOrImmediate::Immediate(val) = &bin_op.b {
                // TODO
                // Remove this unwrap()
                (&bin_op.a, Felt252::from(&val.value))
            } else {
                return Err(HintError::CustomHint("Failed to extract buffer, expected ResOperand of BinOp type to have Inmediate b value".to_owned().into_boxed_str()));
            }
        }
        _ => {
            return Err(HintError::CustomHint(
                "Illegal argument for a buffer."
                    .to_string()
                    .into_boxed_str(),
            ))
        }
    };
    Ok((cell, base_offset))
}

fn get_ptr(
    vm: &VirtualMachine,
    cell: &CellRef,
    offset: &Felt252,
) -> Result<Relocatable, VirtualMachineError> {
    Ok((vm.get_relocatable(cell_ref_to_relocatable(cell, vm)?)? + offset)?)
}

fn cell_ref_to_relocatable(
    cell_ref: &CellRef,
    vm: &VirtualMachine,
) -> Result<Relocatable, MathError> {
    let base = match cell_ref.register {
        Register::AP => vm.get_ap(),
        Register::FP => vm.get_fp(),
    };
    base + (cell_ref.offset as i32)
}
