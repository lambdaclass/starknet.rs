use cairo_rs::{types::relocatable::Relocatable, vm::vm_core::VirtualMachine};
use felt::Felt252;
use num_traits::Zero;

use crate::core::errors::{state_errors::StateError, syscall_handler_errors::SyscallHandlerError};

use super::{
    syscall_request::{FromPtr, StorageReadRequest, SyscallRequest},
    syscall_response::{ResponseBody, SyscallResponse},
};

pub(crate) trait SyscallHandler {
    fn storage_read(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
        remaining_gas: u64,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let request = match self.read_and_validate_syscall_request("storage_read", vm, syscall_ptr)
        {
            Ok(SyscallRequest::StorageRead(storage_read_request)) => storage_read_request,
            _ => return Err(SyscallHandlerError::InvalidSyscallReadRequest),
        };

        if request.reserved != Felt252::zero() {
            return Err(SyscallHandlerError::UnsupportedAddressDomain(
                request.reserved.to_string(),
            ));
        }

        let value = self._storage_read(request.key)?;

        Ok(SyscallResponse {
            gas: remaining_gas,
            body: Some(ResponseBody::StorageReadResponse { value: Some(value) }),
        })
    }

    fn _storage_read(&mut self, key: [u8; 32]) -> Result<Felt252, StateError>;

    fn read_and_validate_syscall_request(
        &mut self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError>;

    fn read_syscall_request(
        &self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        match syscall_name {
            "storage_read" => StorageReadRequest::from_ptr(vm, syscall_ptr),
            _ => Err(SyscallHandlerError::UnknownSyscall(
                syscall_name.to_string(),
            )),
        }
    }
}
