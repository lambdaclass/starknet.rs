use crate::{
    core::errors::{
        contract_address_errors::ContractAddressError, hash_errors::HashError,
        state_errors::StateError,
    },
    definitions::transaction_type::TransactionType,
    execution::os_usage::OsResources,
    syscalls::syscall_handler_errors::SyscallHandlerError,
    utils::ClassHash,
};
use cairo_vm::{
    types::{
        errors::{math_errors::MathError, program_errors::ProgramError},
        relocatable::Relocatable,
    },
    vm::errors::{
        cairo_run_errors::CairoRunError, memory_errors::MemoryError, runner_errors::RunnerError,
        trace_errors::TraceError, vm_errors::VirtualMachineError,
    },
    Felt252,
};
use starknet::core::types::FromByteArrayError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("Nonce is None")]
    MissingNonce,
    #[error("The max_fee field in Declare transactions of version 0 must be 0")]
    InvalidMaxFee,
    #[error("The nonce field in Declare transactions of version 0 must be 0.")]
    InvalidNonce,
    #[error("Couldn't convert from {0} to {1}")]
    Conversion(String, String),
    #[error("The signature field in Declare transactions of version 0 must be an empty list.")]
    InvalidSignature,
    #[error("An InvokeFunction transaction (version != 0) must have a nonce.")]
    InvokeFunctionNonZeroMissingNonce,
    #[error("An InvokeFunction transaction (version = 0) cannot have a nonce.")]
    InvokeFunctionZeroHasNonce,
    #[error("Invalid transaction nonce. Expected: {0} got {1}")]
    InvalidTransactionNonce(String, String),
    #[error("Actual fee exceeds max fee. Actual: {0}, Max: {1}")]
    ActualFeeExceedsMaxFee(u128, u128),
    #[error("Fee transfer failure: {0}")]
    FeeTransferError(Box<TransactionError>),
    #[error("{0}")]
    FeeError(String),
    #[error("Cairo resource names must be contained in fee weights dict")]
    ResourcesError,
    #[error("Could not calculate resources")]
    ResourcesCalculation,
    #[error(transparent)]
    ContractAddress(#[from] ContractAddressError),
    #[error(transparent)]
    Syscall(#[from] SyscallHandlerError),
    #[error(transparent)]
    HashError(#[from] HashError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("Calling other contracts during validate execution is forbidden")]
    UnauthorizedActionOnValidate,
    #[error("Class hash {0:?} already declared")]
    ClassAlreadyDeclared(ClassHash),
    #[error("Expected a relocatable value but got an integer")]
    NotARelocatableValue,
    #[error("Unexpected holes in the event order")]
    UnexpectedHolesInEventOrder,
    #[error("Unexpected holes in the L2-to-L1 message order.")]
    UnexpectedHolesL2toL1Messages,
    #[error("Attemp to return class hash with incorrect call type")]
    CallTypeIsNotDelegate,
    #[error("Attemp to return code address when it is None")]
    AttempToUseNoneCodeAddress,
    #[error("Error recovering class hash from storage")]
    FailToReadClassHash,
    #[error("Missing compiled class after fetching")]
    MissingCompiledClass,
    #[error("Contract address {0:?} is not deployed")]
    NotDeployedContract(ClassHash),
    #[error("Non-unique entry points are not possible in a ContractClass object")]
    NonUniqueEntryPoint,
    #[error("Requested entry point was not found")]
    EntryPointNotFound,
    #[error("Ptr result diverges after calculating final stacks")]
    OsContextPtrNotEqual,
    #[error("Empty OS context")]
    EmptyOsContext,
    #[error("Illegal OS ptr offset")]
    IllegalOsPtrOffset,
    #[error("Invalid pointer fetched from memory expected maybe relocatable but got None")]
    InvalidPtrFetch,
    #[error("Segment base pointer must be zero; got {0}")]
    InvalidSegBasePtrOffset(usize),
    #[error("Invalid segment size; expected usize but got None")]
    InvalidSegmentSize,
    #[error("Invalid stop pointer for segment; expected {0}, found {1}")]
    InvalidStopPointer(Relocatable, Relocatable),
    #[error("Invalid entry point types")]
    InvalidEntryPoints,
    #[error("Expected a Felt value got a Relocatable")]
    NotAFelt,
    #[error("Out of bounds write to a read-only segment.")]
    OutOfBound,
    #[error("Call to another contract has been done")]
    InvalidContractCall,
    #[error("The sender address field in Declare transactions of version 0")]
    InvalidSenderAddress,
    #[error(transparent)]
    TraceException(#[from] TraceError),
    #[error(transparent)]
    MemoryException(#[from] MemoryError),
    #[error("Missing initial_fp")]
    MissingInitialFp,
    #[error("Transaction context is invalid")]
    InvalidTxContext,
    #[error("{0:?}")]
    SierraCompileError(String),
    #[error("Invalid builtin found in contract class: {0}")]
    InvalidBuiltinContractClass(String),
    #[error("The hash of sierra contract classs is not equal to compiled class hash")]
    NotEqualClassHash,
    #[error(transparent)]
    Vm(#[from] VirtualMachineError),
    #[error(transparent)]
    CairoRunner(#[from] CairoRunError),
    #[error(transparent)]
    Runner(#[from] RunnerError),
    #[error("Transaction type {0:?} not found in OsResources: {1:?}")]
    NoneTransactionType(TransactionType, OsResources),
    #[error(transparent)]
    MathError(#[from] MathError),
    #[error(transparent)]
    ProgramError(#[from] ProgramError),
    #[error("Cannot pass calldata to a contract with no constructor")]
    EmptyConstructorCalldata,
    #[error("Invalid Block number")]
    InvalidBlockNumber,
    #[error("Invalid Block timestamp")]
    InvalidBlockTimestamp,
    #[error("{0:?}")]
    CustomError(String),
    #[error("call info is None")]
    CallInfoIsNone,
    #[error("Unsupported version {0:?}")]
    UnsupportedVersion(String),
    #[error("Invalid compiled class, expected class hash: {0}, but received: {1}")]
    InvalidCompiledClassHash(String, String),
    #[error(transparent)]
    FromByteArrayError(#[from] FromByteArrayError),
    #[error("DeclareV2 transaction has neither Sierra nor Casm contract class set")]
    DeclareV2NoSierraOrCasm,
    #[error("Unsupported {0} transaction version: {1}. Supported versions:{2:?}")]
    UnsupportedTxVersion(String, Felt252, Vec<usize>),
    #[error("The `validate` entry point should return `VALID`.")]
    WrongValidateRetdata,
    #[error("Max fee ({0}) is too low. Minimum fee: {1}.")]
    MaxFeeTooLow(u128, u128),
    #[error("Max l1 gas amount ({0}) is too low. Minimum l1 gas amount: {1}.")]
    MaxL1GasAmountTooLow(u64, u128),
    #[error("Max l1 gas price ({0}) is too low. Actual l1 gas price: {1}.")]
    MaxL1GasPriceTooLow(u128, u128),
    #[error("Max fee ({0}) exceeds balance (Uint256({1}, {2})).")]
    MaxFeeExceedsBalance(u128, Felt252, Felt252),
    #[error("V3 Transactions can't be created with deprecated account tx fields")]
    DeprecatedAccountTxFieldsVInV3TX,
    #[error("Non V3 Transactions can't be created with non deprecated account tx fields")]
    CurrentAccountTxFieldsInNonV3TX,
    // Variant used to detect revert errors in revertible transactions
    #[error(transparent)]
    FeeCheck(#[from] FeeCheckError),
}

#[derive(Debug, Error)]
// Enum used to detect revert errors in revertible transactions post-execution checks
pub enum FeeCheckError {
    #[error("Insufficient fee token balance")]
    InsufficientFeeTokenBalance,
    #[error("Calculated l1 gas amount ({0}) exceeds max l1 gas amount ({1})")]
    L1GasAmountExceedsMax(u128, u64),
    #[error("Calculated fee ({0}) exceeds max fee ({1})")]
    FeeExceedsMax(u128, u128),
}
