use std::error;

use cairo_rs::vm::errors::vm_errors::VirtualMachineError;
use thiserror::Error;

use crate::core::errors;
#[derive(Debug, PartialEq, Error)]
pub enum ExecutionError {
    #[error("Missing field for TxStruct")]
    MissingTxStructField,
    #[error("Expected an int value but get wrong data type")]
    NotAFeltValue,
    #[error("Expected a relocatable value but get wrong data type")]
    NotARelocatableValue,
    #[error("Error converting from {0} to {1}")]
    ErrorInDataConversion(String, String),
    #[error("Unexpected holes in the event order")]
    UnexpectedHolesInEventOrder,
    #[error("Unexpected holes in the L2-to-L1 message order.")]
    UnexpectedHolesL2toL1Messages,
    #[error("Trace is not enabled for this run")]
    TraceError,
    #[error("Call type {0} not implemented")]
    CallTypeNotImplemented(String),
    #[error("Attemp to return class hash with incorrect call type")]
    CallTypeIsNotDelegate,
    #[error("Attemp to return code address when is None")]
    AttempToUseNoneCodeAddress,
    #[error("error recovering class hash from storage")]
    FailToReadClassHash,
    #[error("error while fetching redata {0}")]
    RetdataError(String),
    #[error("Missing contract class after fetching")]
    MissigContractClass,
    #[error("Could not create cairo runner")]
    FailToCreateCairoRunner,
    #[error("Value should be a pointer but got an Int")]
    ExpectedPointer,
    #[error("contract address {0:?} not deployed")]
    NotDeployedContract([u8; 32]),
    #[error(transparent)]
    VmException(#[from] VirtualMachineError),
}
