use crate::syscalls::syscall_handler_errors::SyscallHandlerError;
use cairo_vm::Felt252;
use cairo_vm::{
    hint_processor::builtin_hint_processor::{
        builtin_hint_processor_definition::HintProcessorData,
        hint_utils::{get_integer_from_var_name, insert_value_from_var_name},
    },
    vm::{errors::hint_errors::HintError, vm_core::VirtualMachine},
};

use std::collections::HashMap;

pub fn addr_bound_prime(
    vm: &mut VirtualMachine,
    hint_data: &HintProcessorData,
    constants: &HashMap<String, Felt252>,
) -> Result<(), SyscallHandlerError> {
    let addr_bound = constants
        .get("starkware.starknet.common.storage.ADDR_BOUND")
        .ok_or(HintError::MissingConstant(
            "starkware.starknet.common.storage.ADDR_BOUND".into(),
        ))?;

    let lower_bound = Felt252::TWO.pow(250u32);
    let upper_bound = Felt252::TWO.pow(251u32);
    if !(&lower_bound < addr_bound && addr_bound <= &upper_bound) {
        return Err(HintError::AssertionFailed(
            "normalize_address() cannot be used with the current constants."
                .to_string()
                .into_boxed_str(),
        )
        .into());
    }

    let addr = get_integer_from_var_name("addr", vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
    let is_small = if addr.as_ref() < addr_bound {
        Felt252::ONE
    } else {
        Felt252::ZERO
    };

    insert_value_from_var_name(
        "is_small",
        is_small,
        vm,
        &hint_data.ids_data,
        &hint_data.ap_tracking,
    )?;

    Ok(())
}

pub fn addr_is_250(
    vm: &mut VirtualMachine,
    hint_data: &HintProcessorData,
) -> Result<(), SyscallHandlerError> {
    let addr = get_integer_from_var_name("addr", vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
    let is_250 = if addr.as_ref().bits() <= 250 {
        Felt252::ONE
    } else {
        Felt252::ZERO
    };

    insert_value_from_var_name(
        "is_250",
        is_250,
        vm,
        &hint_data.ids_data,
        &hint_data.ap_tracking,
    )?;

    Ok(())
}
