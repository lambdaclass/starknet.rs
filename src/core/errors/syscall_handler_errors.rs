use super::state_errors::StateError;
use cairo_vm::felt::Felt252;
use cairo_vm::{
    types::errors::math_errors::MathError,
    vm::errors::{
        hint_errors::HintError, memory_errors::MemoryError, vm_errors::VirtualMachineError,
    },
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyscallHandlerError {
    #[error("Unknown syscall: {0}")]
    UnknownSyscall(String),
    #[error("The selector '{0}' is not in the syscall handler's selector to syscall map")]
    SelectorNotInHandlerMap(String),
    #[error("The selector '{0}' does not have an associated cost")]
    SelectorDoesNotHaveAssociatedGas(String),
    #[error("Couldn't execute syscall: {0}")]
    ExecutionError(String),
    #[error("Couldn't convert Felt to usize")]
    FeltToUsizeFail,
    #[error("Couldn't convert Felt to u64")]
    FeltToU64Fail,
    #[error("Couldn't compute hash")]
    FailToComputeHash,
    #[error("Expected DesployRequestStruct")]
    ExpectedDeployRequestStruct,
    #[error("Expected StorageWriteSyscall")]
    ExpectedStorageWriteSyscall,
    #[error("Unsopported address domain: {0}")]
    UnsopportedAddressDomain(Felt252),
    #[error("Expected GetCallerAddressRequest")]
    ExpectedGetCallerAddressRequest,
    #[error("Expected SendMessageToL1")]
    ExpectedSendMessageToL1,
    #[error("Expected GetBlockTimestampRequest")]
    ExpectedGetBlockTimestampRequest,
    #[error("The deploy_from_zero field in the deploy system call must be 0 or 1, found: {0}")]
    DeployFromZero(usize),
    #[error("Hint not implemented: {0}")]
    NotImplemented(String),
    #[error("HintData is incorrect")]
    WrongHintData,
    #[error("Unknown hint")]
    UnknownHint,
    #[error("Iterator is not empty")]
    IteratorNotEmpty,
    #[error("Iterator is empty")]
    IteratorEmpty,
    #[error("List is empty")]
    ListIsEmpty,
    #[error("{0} should be None")]
    ShouldBeNone(String),
    #[error("Unexpected construct retdata")]
    UnexpectedConstructorRetdata,
    #[error("Key not found")]
    KeyNotFound,
    #[error("The requested syscall read was not of the expected type")]
    InvalidSyscallReadRequest,
    #[error("tx_info_ptr is None")]
    TxInfoPtrIsNone,
    #[error("Virtual machine error: {0}")]
    VirtualMachine(#[from] VirtualMachineError),
    #[error("Expected GetContractAddressRequest")]
    ExpectedGetContractAddressRequest,
    #[error("Expected CallContractRequest")]
    ExpectedCallContractRequest,
    #[error("Expected a LibraryCallRequest")]
    ExpectedLibraryCallRequest,
    #[error("Expected GetSequencerAddressRequest")]
    ExpectedGetSequencerAddressRequest,
    #[error("Memory error: {0}")]
    Memory(#[from] MemoryError),
    #[error("Expected GetTxSignatureRequest")]
    ExpectedGetTxSignatureRequest,
    #[error("Expected a ptr but received invalid data")]
    InvalidTxInfoPtr,
    #[error("Could not convert felt to u64")]
    InvalidFeltConversion,
    #[error("Could not compute hash")]
    ErrorComputingHash,
    #[error("Inconsistent start and end segment indices")]
    InconsistentSegmentIndices,
    #[error("Start offset greater than end offset")]
    StartOffsetGreaterThanEndOffset,
    #[error("Incorrect request in syscall {0}")]
    IncorrectSyscall(String),
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    MathError(#[from] MathError),
    #[error(transparent)]
    Hint(#[from] HintError),
    #[error("Unsupported address domain: {0}")]
    UnsupportedAddressDomain(String),
}
