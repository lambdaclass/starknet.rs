use cairo_vm::Felt252;
use cairo_vm::{types::relocatable::Relocatable, vm::vm_core::VirtualMachine};
use num_traits::ToPrimitive;

use crate::transaction::Address;
use crate::{
    syscalls::syscall_handler_errors::SyscallHandlerError,
    utils::{get_big_int, get_integer, get_relocatable},
};
// TODO: maybe we could make FromPtr trait more general, making
//   it "move" the pointer received like they do in cairo-lang
// The size of the RequestHeader in VM memory
// ```
// struct RequestHeader {
//     // The syscall selector.
//     selector: Felt252,
//     // The amount of gas left before the syscall execution.
//     gas: Felt252,
// }
// ```

/// Abstracts every request variant for each syscall.
#[derive(Debug, PartialEq)]
pub(crate) enum SyscallRequest {
    /// Emits an event with a given set of keys and data.
    EmitEvent(EmitEventRequest),
    /// Calls the requested function in any previously declared class.
    LibraryCall(LibraryCallRequest),
    /// Calls a given contract.
    CallContract(CallContractRequest),
    /// Deploys a new instance of a previously declared class.
    Deploy(DeployRequest),
    /// Gets the number of the block in which the transaction is executed.
    GetBlockNumber,
    /// Gets information about the original transaction.
    GetExecutionInfo,
    /// Gets the value of a key in the storage of the calling contract.
    StorageRead(StorageReadRequest),
    /// Sets the value of a key in the storage of the calling contract.
    StorageWrite(StorageWriteRequest),
    /// Sends a message to L1.
    SendMessageToL1(SendMessageToL1Request),
    /// Gets the timestamp of the block in which the transaction is executed.
    GetBlockTimestamp(GetBlockTimestampRequest),
    /// Gets the hash value of a block.
    GetBlockHash(GetBlockHashRequest),
    /// Replaces the class of the calling contract.
    ReplaceClass(ReplaceClassRequest),
    /// Computes the Keccak256 hash of the given data.
    Keccak(KeccakRequest),
}

// ~~~~~~~~~~~~~~~~~~~~~~~~~
//  SyscallRequest variants
// ~~~~~~~~~~~~~~~~~~~~~~~~~

/// Gets the timestamp of the block in which the transaction is executed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GetBlockTimestampRequest {}

/// Deploys a new instance of a previously declared class.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeployRequest {
    // The hash of the class to deploy.
    pub(crate) class_hash: Felt252,
    // A salt for the new contract address calculation.
    pub(crate) salt: Felt252,
    // The calldata for the constructor.
    pub(crate) calldata_start: Relocatable,
    pub(crate) calldata_end: Relocatable,
    // Used for deterministic contract address deployment.
    pub(crate) deploy_from_zero: usize,
}

/// Gets the value of a key in the storage of the calling contract.
///
/// This system call provides direct access to any possible key in storage, in contrast
/// with `balance.read()`, which enables you to read storage variables that are defined
/// explicitly in the contract.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StorageReadRequest {
    /// The key associated with the requested storage value.
    pub(crate) key: [u8; 32],
    pub(crate) reserved: Felt252,
}

/// Emits an event with a given set of keys and data.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EmitEventRequest {
    /// The event's key segment start.
    pub(crate) keys_start: Relocatable,
    /// The event's key segment end.
    pub(crate) keys_end: Relocatable,
    /// The event's data segment start.
    pub(crate) data_start: Relocatable,
    /// The event's data segment end.
    pub(crate) data_end: Relocatable,
}

/// Calls a given contract.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CallContractRequest {
    /// A selector for a function within that contract.
    pub(crate) selector: Felt252,
    /// The address of the contract you want to call.
    pub(crate) contract_address: Address,
    /// The calldata segment start.
    pub(crate) calldata_start: Relocatable,
    /// The calldata segment end.
    pub(crate) calldata_end: Relocatable,
}

/// Calls the requested function in any previously declared class.
///
/// This system call replaces the known delegate call functionality from Ethereum,
/// with the important difference that there is only one contract involved.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LibraryCallRequest {
    /// The hash of the class you want to use.
    pub(crate) class_hash: Felt252,
    /// A selector for a function within that class.
    pub(crate) selector: Felt252,
    /// The calldata segment start.
    pub(crate) calldata_start: Relocatable,
    /// The calldata segment end.
    pub(crate) calldata_end: Relocatable,
}

/// Sets the value of a key in the storage of the calling contract.
///
/// This system call provides direct access to any possible key in storage,
/// in contrast with balance.write(), which enables you to write to storage variables
/// that are defined explicitly in the contract.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StorageWriteRequest {
    pub(crate) reserved: Felt252,
    /// The key associated with the requested storage value.
    pub(crate) key: Felt252,
    /// The value to write to the key.
    pub(crate) value: Felt252,
}

// Arguments given in the syscall documentation
// https://github.com/starkware-libs/cairo-lang/blob/c954f154bbab04c3fb27f7598b015a9475fc628e/src/starkware/starknet/common/new_syscalls.cairo#L138
// to_address
// The recipient’s L1 address.

// payload
// The array containing the message payload -> relocatable

/// Sends a message to L1.
///
/// This system call includes the message parameters as part of the proof’s output,
/// and exposes these parameters to the Starknet Core contract on L1 once the state update,
/// including the transaction, is received.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SendMessageToL1Request {
    /// The recipient’s L1 address.
    pub(crate) to_address: Address,
    /// The payload segment start.
    pub(crate) payload_start: Relocatable,
    /// The payload segment end.
    pub(crate) payload_end: Relocatable,
}

/// Gets the hash value of a block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GetBlockHashRequest {
    /// The block's number
    pub(crate) block_number: u64,
}

/// Replaces the class of the calling contract (i.e. the contract whose address is
/// returned by `get_contract_address` at the time the syscall is called) by the class
/// of the given hash.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReplaceClassRequest {
    /// The hash of the class that will replace the calling contract one.
    pub(crate) class_hash: Felt252,
}

/// Computes the Keccak256 hash of the given data.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeccakRequest {
    /// The input data start.
    pub(crate) input_start: Relocatable,
    /// The input data end.
    pub(crate) input_end: Relocatable,
}

// ~~~~~~~~~~~~~~~~~~~~~~~~~
//  Into<SyscallRequest> implementations
// ~~~~~~~~~~~~~~~~~~~~~~~~~

impl From<ReplaceClassRequest> for SyscallRequest {
    fn from(replace_class_request: ReplaceClassRequest) -> SyscallRequest {
        SyscallRequest::ReplaceClass(replace_class_request)
    }
}

impl From<GetBlockTimestampRequest> for SyscallRequest {
    fn from(get_block_timestamp: GetBlockTimestampRequest) -> SyscallRequest {
        SyscallRequest::GetBlockTimestamp(get_block_timestamp)
    }
}

impl From<EmitEventRequest> for SyscallRequest {
    fn from(emit_event_struct: EmitEventRequest) -> SyscallRequest {
        SyscallRequest::EmitEvent(emit_event_struct)
    }
}

impl From<CallContractRequest> for SyscallRequest {
    fn from(call_contract_request: CallContractRequest) -> SyscallRequest {
        SyscallRequest::CallContract(call_contract_request)
    }
}

impl From<LibraryCallRequest> for SyscallRequest {
    fn from(library_call_request: LibraryCallRequest) -> Self {
        SyscallRequest::LibraryCall(library_call_request)
    }
}

impl From<SendMessageToL1Request> for SyscallRequest {
    fn from(syscall: SendMessageToL1Request) -> Self {
        SyscallRequest::SendMessageToL1(syscall)
    }
}

impl From<StorageWriteRequest> for SyscallRequest {
    fn from(storage_write_request: StorageWriteRequest) -> SyscallRequest {
        SyscallRequest::StorageWrite(storage_write_request)
    }
}

impl From<StorageReadRequest> for SyscallRequest {
    fn from(storage_read_request: StorageReadRequest) -> SyscallRequest {
        SyscallRequest::StorageRead(storage_read_request)
    }
}

impl From<GetBlockHashRequest> for SyscallRequest {
    fn from(get_block_hash_request: GetBlockHashRequest) -> SyscallRequest {
        SyscallRequest::GetBlockHash(get_block_hash_request)
    }
}

impl From<KeccakRequest> for SyscallRequest {
    fn from(request: KeccakRequest) -> SyscallRequest {
        SyscallRequest::Keccak(request)
    }
}

// ~~~~~~~~~~~~~~~~~~~~~~~~~
//  FromPtr trait
// ~~~~~~~~~~~~~~~~~~~~~~~~~

pub(crate) trait FromPtr {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError>;
}

impl FromPtr for ReplaceClassRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        Ok(ReplaceClassRequest {
            class_hash: vm.get_integer(syscall_ptr)?.into_owned(),
        }
        .into())
    }
}

impl FromPtr for GetBlockTimestampRequest {
    fn from_ptr(
        _vm: &VirtualMachine,
        _syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        Ok(GetBlockTimestampRequest {}.into())
    }
}

impl FromPtr for GetBlockHashRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        Ok(GetBlockHashRequest {
            block_number: get_big_int(vm, syscall_ptr)?.to_u64().ok_or(
                SyscallHandlerError::Conversion("Felt252".to_string(), "u64".to_string()),
            )?,
        }
        .into())
    }
}

impl FromPtr for EmitEventRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let keys_start = get_relocatable(vm, syscall_ptr)?;
        let keys_end = get_relocatable(vm, (syscall_ptr + 1)?)?;
        let data_start = get_relocatable(vm, (syscall_ptr + 2)?)?;
        let data_end = get_relocatable(vm, (syscall_ptr + 3)?)?;

        Ok(EmitEventRequest {
            keys_start,
            keys_end,
            data_start,
            data_end,
        }
        .into())
    }
}

impl FromPtr for StorageReadRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let reserved = get_big_int(vm, syscall_ptr)?;
        let key = get_big_int(vm, (syscall_ptr + 1)?)?.to_bytes_be();
        Ok(StorageReadRequest { key, reserved }.into())
    }
}

impl FromPtr for DeployRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let class_hash = get_big_int(vm, syscall_ptr)?;
        let salt = get_big_int(vm, (syscall_ptr + 1)?)?;
        let calldata_start = get_relocatable(vm, (syscall_ptr + 2)?)?;
        let calldata_end = get_relocatable(vm, (syscall_ptr + 3)?)?;
        let deploy_from_zero = get_integer(vm, (syscall_ptr + 4)?)?;

        Ok(SyscallRequest::Deploy(DeployRequest {
            class_hash,
            salt,
            calldata_start,
            calldata_end,
            deploy_from_zero,
        }))
    }
}

impl FromPtr for CallContractRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let contract_address = Address(get_big_int(vm, syscall_ptr)?);
        let selector = get_big_int(vm, (syscall_ptr + 1)?)?;
        let calldata_start = get_relocatable(vm, (syscall_ptr + 2)?)?;
        let calldata_end = get_relocatable(vm, (syscall_ptr + 3)?)?;
        Ok(CallContractRequest {
            selector,
            contract_address,
            calldata_start,
            calldata_end,
        }
        .into())
    }
}

impl FromPtr for LibraryCallRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let class_hash = get_big_int(vm, syscall_ptr)?;
        let selector = get_big_int(vm, (syscall_ptr + 1)?)?;
        let calldata_start = get_relocatable(vm, (syscall_ptr + 2)?)?;
        let calldata_end = get_relocatable(vm, (syscall_ptr + 3)?)?;

        Ok(LibraryCallRequest {
            class_hash,
            selector,
            calldata_start,
            calldata_end,
        }
        .into())
    }
}

impl FromPtr for SendMessageToL1Request {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let to_address = Address(get_big_int(vm, syscall_ptr)?);
        let payload_start = get_relocatable(vm, (syscall_ptr + 1)?)?;
        let payload_end = get_relocatable(vm, (syscall_ptr + 2)?)?;

        Ok(SendMessageToL1Request {
            to_address,
            payload_start,
            payload_end,
        }
        .into())
    }
}

impl FromPtr for StorageWriteRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let reserved = get_big_int(vm, syscall_ptr)?;
        let key = get_big_int(vm, (syscall_ptr + 1)?)?;
        let value = get_big_int(vm, (syscall_ptr + 2)?)?;

        Ok(StorageWriteRequest {
            reserved,
            key,
            value,
        }
        .into())
    }
}

impl FromPtr for KeccakRequest {
    fn from_ptr(
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        let input_start = get_relocatable(vm, syscall_ptr)?;
        let input_end = get_relocatable(vm, (syscall_ptr + 1)?)?;

        Ok(KeccakRequest {
            input_start,
            input_end,
        }
        .into())
    }
}
