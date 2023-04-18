use cairo_rs::{types::relocatable::Relocatable, vm::vm_core::VirtualMachine};
use felt::Felt252;

use crate::{
    core::errors::syscall_handler_errors::SyscallHandlerError,
    utils::{get_big_int, get_relocatable, Address},
};

#[derive(Debug, PartialEq)]
pub(crate) enum SyscallRequest {
    StorageWrite(StorageWriteRequest),
    SendMessageToL1(SendMessageToL1SysCall),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StorageWriteRequest {
    pub(crate) reserved: Felt252,
    pub(crate) key: Felt252,
    pub(crate) value: Felt252,
}

impl From<StorageWriteRequest> for SyscallRequest {
    fn from(storage_write_request: StorageWriteRequest) -> SyscallRequest {
        SyscallRequest::StorageWrite(storage_write_request)
    }
}

// Arguments given in the syscall documentation
// https://github.com/starkware-libs/cairo-lang/blob/c954f154bbab04c3fb27f7598b015a9475fc628e/src/starkware/starknet/common/new_syscalls.cairo#L138
// to_address
// The recipient’s L1 address.

// payload
// The array containing the message payload -> relocatable
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SendMessageToL1SysCall {
    pub(crate) to_address: Address,
    pub(crate) payload_start: Relocatable,
    pub(crate) payload_end: Relocatable,
}

impl From<SendMessageToL1SysCall> for SyscallRequest {
    fn from(syscall: SendMessageToL1SysCall) -> Self {
        SyscallRequest::SendMessageToL1(syscall)
    }
}

// ~~~~~~~~~~~~~~~~~~~~~~~~~
//  FromPtr implementations
// ~~~~~~~~~~~~~~~~~~~~~~~~~

pub(crate) trait FromPtr {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError>;
}

impl FromPtr for StorageWriteRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let reserved = get_big_int(vm, syscall_ptr)?;
        let key = get_big_int(vm, &syscall_ptr + 1)?;
        let value = get_big_int(vm, &syscall_ptr + 2)?;

        Ok(StorageWriteRequest {
            reserved,
            key,
            value,
        }
        .into())
    }
}

impl FromPtr for SendMessageToL1SysCall {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let to_address = Address(get_big_int(vm, syscall_ptr)?);
        let payload_start = get_relocatable(vm, &syscall_ptr + 1)?;
        let payload_end = get_relocatable(vm, &syscall_ptr + 2)?;

        Ok(SendMessageToL1SysCall {
            to_address,
            payload_start,
            payload_end,
        }
        .into())
    }
}
