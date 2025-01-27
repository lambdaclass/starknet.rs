use core::fmt;

use crate::{
    core::contract_address::compute_casm_class_hash,
    definitions::{
        block_context::{BlockContext, FeeType},
        constants::{QUERY_VERSION_0, QUERY_VERSION_1, QUERY_VERSION_2, QUERY_VERSION_3},
    },
    execution::TransactionExecutionInfo,
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState, contract_class_cache::ContractClassCache, state_api::StateReader,
    },
    utils::felt_to_hash,
};
pub use declare::Declare;
pub use declare_deprecated::DeclareDeprecated;
pub use deploy::Deploy;
pub use deploy_account::DeployAccount;
use error::TransactionError;
pub use invoke_function::InvokeFunction;
pub use l1_handler::L1Handler;

pub mod declare;
pub mod declare_deprecated;
pub mod deploy;
pub mod deploy_account;
pub mod error;
pub mod fee;
pub mod invoke_function;
pub mod l1_handler;

use cairo_vm::Felt252;
use serde::{Deserialize, Serialize};
use starknet_api::transaction::Resource;

#[cfg(feature = "cairo-native")]
use {
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

#[derive(Clone, PartialEq, Hash, Eq, Default, Serialize, Deserialize)]
pub struct Address(pub Felt252);

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}", self.0)
    }
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl From<Felt252> for Address {
    fn from(value: Felt252) -> Self {
        Self(value)
    }
}

impl From<&Felt252> for Address {
    fn from(value: &Felt252) -> Self {
        Self(*value)
    }
}

impl From<Address> for Felt252 {
    fn from(value: Address) -> Self {
        value.0
    }
}

impl From<&Address> for Felt252 {
    fn from(value: &Address) -> Self {
        value.0
    }
}

impl Address {
    pub fn from_hex_string(hex_string: &str) -> Option<Self> {
        Some(Self(Felt252::from_hex(hex_string).ok()?))
    }
}

#[derive(Clone, PartialEq, Hash, Default, Serialize, Deserialize, Copy)]
pub struct ClassHash(pub [u8; 32]);

impl ClassHash {
    pub fn new(bytes: [u8; 32]) -> Self {
        ClassHash(bytes)
    }

    pub fn to_bytes_be(&self) -> &[u8] {
        &self.0
    }

    pub fn as_slice(&self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Display for ClassHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hex_string = hex::encode(self.0);
        let trimmed_hex_string = hex_string.trim_start_matches('0');
        write!(f, "0x{}", trimmed_hex_string)?;
        Ok(())
    }
}

impl fmt::Debug for ClassHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl From<Felt252> for ClassHash {
    fn from(felt: Felt252) -> Self {
        felt_to_hash(&felt)
    }
}

impl From<[u8; 32]> for ClassHash {
    fn from(bytes: [u8; 32]) -> Self {
        ClassHash(bytes)
    }
}

impl Eq for ClassHash {}

impl PartialEq<[u8; 32]> for ClassHash {
    fn eq(&self, other: &[u8; 32]) -> bool {
        &self.0 == other
    }
}

impl ClassHash {
    pub fn from_hex_string(hex_string: String) -> Option<Self> {
        Some(Self(hex::decode(hex_string).ok()?.try_into().ok()?))
    }
}

pub type CompiledClassHash = ClassHash;

/// Represents a transaction inside the starknet network.
/// The transaction are actions that may modified the state of the network.
/// it can be one of:
/// - DeclareDeprecated
/// - Declare
/// - Deploy
/// - DeployAccount
/// - InvokeFunction
/// - L1Handler
pub enum Transaction {
    /// A deprecated declare transaction.
    DeclareDeprecated(DeclareDeprecated),
    /// A declare transaction.
    Declare(Box<Declare>),
    /// A deploy transaction.
    Deploy(Deploy),
    /// A deploy account transaction.
    DeployAccount(DeployAccount),
    /// An invoke transaction.
    InvokeFunction(InvokeFunction),
    /// An L1 handler transaction.
    L1Handler(L1Handler),
}

impl Transaction {
    /// returns the contract address of the transaction.
    pub fn contract_address(&self) -> Address {
        match self {
            Transaction::Deploy(tx) => tx.contract_address.clone(),
            Transaction::InvokeFunction(tx) => tx.contract_address().clone(),
            Transaction::DeclareDeprecated(tx) => tx.sender_address.clone(),
            Transaction::Declare(tx) => tx.sender_address.clone(),
            Transaction::DeployAccount(tx) => tx.contract_address().clone(),
            Transaction::L1Handler(tx) => tx.contract_address().clone(),
        }
    }

    /// execute the transaction in cairo-vm and returns a TransactionExecutionInfo structure.
    ///## Parameters:
    ///- state: a structure that implements State and StateReader traits.
    ///- block_context: The block context of the transaction that is about to be executed.
    ///- remaining_gas: The gas supplied to execute the transaction.
    pub fn execute<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        match self {
            Transaction::DeclareDeprecated(tx) => tx.execute(
                state,
                block_context,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            Transaction::Declare(tx) => tx.execute(
                state,
                block_context,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            Transaction::Deploy(tx) => tx.execute(
                state,
                block_context,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            Transaction::DeployAccount(tx) => tx.execute(
                state,
                block_context,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            Transaction::InvokeFunction(tx) => tx.execute(
                state,
                block_context,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            Transaction::L1Handler(tx) => tx.execute(
                state,
                block_context,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
        }
    }

    /// It creates a new transaction structure modifying the skip flags. It is meant to be used only to run a simulation
    ///## Parameters:
    ///- skip_validate: the transaction will not be verified.
    ///- skip_execute: the transaction will not be executed in the cairo vm.
    ///- skip_fee_transfer: the transaction will not pay the fee.
    pub fn create_for_simulation(
        &self,
        skip_validate: bool,
        skip_execute: bool,
        skip_fee_transfer: bool,
        ignore_max_fee: bool,
        skip_nonce_check: bool,
    ) -> Self {
        match self {
            Transaction::DeclareDeprecated(tx) => tx.create_for_simulation(
                skip_validate,
                skip_execute,
                skip_fee_transfer,
                ignore_max_fee,
                skip_nonce_check,
            ),
            Transaction::Declare(tx) => tx.create_for_simulation(
                skip_validate,
                skip_execute,
                skip_fee_transfer,
                ignore_max_fee,
                skip_nonce_check,
            ),
            Transaction::Deploy(tx) => {
                tx.create_for_simulation(skip_validate, skip_execute, skip_fee_transfer)
            }
            Transaction::DeployAccount(tx) => tx.create_for_simulation(
                skip_validate,
                skip_execute,
                skip_fee_transfer,
                ignore_max_fee,
                skip_nonce_check,
            ),
            Transaction::InvokeFunction(tx) => tx.create_for_simulation(
                skip_validate,
                skip_execute,
                skip_fee_transfer,
                ignore_max_fee,
                skip_nonce_check,
            ),
            Transaction::L1Handler(tx) => tx.create_for_simulation(skip_validate, skip_execute),
        }
    }
}

// Parses query tx versions into their normal counterpart
// This is used to execute old transactions an may be removed in the future as its not part of the current standard implementation
fn get_tx_version(version: Felt252) -> Felt252 {
    match version {
        version if version == *QUERY_VERSION_0 => Felt252::ZERO,
        version if version == *QUERY_VERSION_1 => Felt252::ONE,
        version if version == *QUERY_VERSION_2 => Felt252::TWO,
        version if version == *QUERY_VERSION_3 => Felt252::THREE,
        version => version,
    }
}

// Check that account_tx_fields is compatible with the tx's version
fn check_account_tx_fields_version(
    account_tx_fields: &VersionSpecificAccountTxFields,
    version: Felt252,
) -> Result<(), TransactionError> {
    match (account_tx_fields, version) {
        (VersionSpecificAccountTxFields::Deprecated(_), v) if v == Felt252::THREE => {
            Err(TransactionError::DeprecatedAccountTxFieldsVInV3TX)
        }
        (VersionSpecificAccountTxFields::Current(_), v) if v < Felt252::THREE => {
            Err(TransactionError::CurrentAccountTxFieldsInNonV3TX)
        }
        _ => Ok(()),
    }
}

/// Creates a `DeclareDeprecated or Declare` from a starknet api `DeclareTransaction`.
pub fn declare_tx_from_sn_api_transaction(
    tx: starknet_api::transaction::DeclareTransaction,
    tx_hash: Felt252,
    contract_class: CompiledClass,
) -> Result<Transaction, TransactionError> {
    let account_tx_fields = match &tx {
        starknet_api::transaction::DeclareTransaction::V0(tx) => {
            VersionSpecificAccountTxFields::Deprecated(tx.max_fee.0)
        }
        starknet_api::transaction::DeclareTransaction::V1(tx) => {
            VersionSpecificAccountTxFields::Deprecated(tx.max_fee.0)
        }
        starknet_api::transaction::DeclareTransaction::V2(tx) => {
            VersionSpecificAccountTxFields::Deprecated(tx.max_fee.0)
        }
        starknet_api::transaction::DeclareTransaction::V3(tx) => {
            VersionSpecificAccountTxFields::Current(CurrentAccountTxFields {
                l1_resource_bounds: tx
                    .resource_bounds
                    .0
                    .get(&Resource::L1Gas)
                    .map(|r| r.into())
                    .unwrap_or_default(),
                l2_resource_bounds: tx.resource_bounds.0.get(&Resource::L2Gas).map(|r| r.into()),
                tip: tx.tip.0,
                nonce_data_availability_mode: tx.nonce_data_availability_mode.into(),
                fee_data_availability_mode: tx.fee_data_availability_mode.into(),
                paymaster_data: tx
                    .paymaster_data
                    .0
                    .iter()
                    .map(|f| Felt252::from_bytes_be_slice(f.bytes()))
                    .collect(),
                account_deployment_data: tx
                    .account_deployment_data
                    .0
                    .iter()
                    .map(|f| Felt252::from_bytes_be_slice(f.bytes()))
                    .collect(),
            })
        }
    };
    let sender_address = Address(Felt252::from_bytes_be_slice(
        tx.sender_address().0.key().bytes(),
    ));
    let version = Felt252::from_bytes_be_slice(tx.version().0.bytes());
    let nonce = Felt252::from_bytes_be_slice(tx.nonce().0.bytes());

    let signature = tx
        .signature()
        .0
        .iter()
        .map(|f| Felt252::from_bytes_be_slice(f.bytes()))
        .collect();

    if version < Felt252::TWO {
        // Create Declare tx
        let contract_class = match contract_class {
            CompiledClass::Deprecated(cc) => cc.as_ref().clone(),
            _ => return Err(TransactionError::DeclareNoSierraOrCasm),
        };

        DeclareDeprecated::new_with_tx_hash(
            contract_class,
            sender_address,
            account_tx_fields.max_fee(),
            version,
            signature,
            nonce,
            tx_hash,
        )
        .map(Transaction::DeclareDeprecated)
    } else {
        let contract_class = match contract_class {
            CompiledClass::Casm { casm, .. } => casm.as_ref().clone(),
            _ => return Err(TransactionError::DeclareNoSierraOrCasm),
        };

        let compiled_class_hash = compute_casm_class_hash(&contract_class).unwrap();

        Declare::new_with_sierra_class_hash_and_tx_hash(
            None,
            Felt252::from_bytes_be_slice(tx.class_hash().0.bytes()),
            Some(contract_class),
            compiled_class_hash,
            sender_address,
            account_tx_fields,
            version,
            signature,
            nonce,
            tx_hash,
        )
        .map(|d| Transaction::Declare(Box::new(d)))
    }
}

#[derive(Clone, Debug, Default, Copy)]
pub enum DataAvailabilityMode {
    #[default]
    L1,
    L2,
}

impl From<DataAvailabilityMode> for starknet_api::data_availability::DataAvailabilityMode {
    fn from(val: DataAvailabilityMode) -> Self {
        match val {
            DataAvailabilityMode::L1 => starknet_api::data_availability::DataAvailabilityMode::L1,
            DataAvailabilityMode::L2 => starknet_api::data_availability::DataAvailabilityMode::L2,
        }
    }
}

impl From<starknet_api::data_availability::DataAvailabilityMode> for DataAvailabilityMode {
    fn from(value: starknet_api::data_availability::DataAvailabilityMode) -> Self {
        match value {
            starknet_api::data_availability::DataAvailabilityMode::L1 => Self::L1,
            starknet_api::data_availability::DataAvailabilityMode::L2 => Self::L2,
        }
    }
}

impl From<DataAvailabilityMode> for Felt252 {
    fn from(val: DataAvailabilityMode) -> Self {
        match val {
            DataAvailabilityMode::L1 => Felt252::ZERO,
            DataAvailabilityMode::L2 => Felt252::ONE,
        }
    }
}

impl From<DataAvailabilityMode> for u64 {
    fn from(val: DataAvailabilityMode) -> Self {
        match val {
            DataAvailabilityMode::L1 => 0,
            DataAvailabilityMode::L2 => 1,
        }
    }
}

impl From<DataAvailabilityMode> for u32 {
    fn from(val: DataAvailabilityMode) -> Self {
        match val {
            DataAvailabilityMode::L1 => 0,
            DataAvailabilityMode::L2 => 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ResourceBounds {
    pub max_amount: u64,
    pub max_price_per_unit: u128,
}

impl From<&starknet_api::transaction::ResourceBounds> for ResourceBounds {
    fn from(value: &starknet_api::transaction::ResourceBounds) -> Self {
        Self {
            max_amount: value.max_amount,
            max_price_per_unit: value.max_price_per_unit,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CurrentAccountTxFields {
    pub l1_resource_bounds: ResourceBounds,
    pub l2_resource_bounds: Option<ResourceBounds>,
    pub tip: u64,
    pub nonce_data_availability_mode: DataAvailabilityMode,
    pub fee_data_availability_mode: DataAvailabilityMode,
    pub paymaster_data: Vec<Felt252>,
    pub account_deployment_data: Vec<Felt252>,
}

#[derive(Clone, Debug)]
pub enum VersionSpecificAccountTxFields {
    // Deprecated fields only consist of max_fee
    Deprecated(u128),
    Current(CurrentAccountTxFields),
}

impl Default for VersionSpecificAccountTxFields {
    fn default() -> Self {
        Self::Deprecated(0)
    }
}

impl VersionSpecificAccountTxFields {
    pub fn new_deprecated(max_fee: u128) -> Self {
        Self::Deprecated(max_fee)
    }

    pub fn max_fee(&self) -> u128 {
        match self {
            Self::Deprecated(max_fee) => *max_fee,
            Self::Current(current) => {
                current.l1_resource_bounds.max_amount as u128
                    * current.l1_resource_bounds.max_price_per_unit
            }
        }
    }

    pub(crate) fn max_fee_for_execution_info(&self) -> u128 {
        match self {
            Self::Deprecated(max_fee) => *max_fee,
            Self::Current(_) => 0,
        }
    }

    pub fn fee_type(&self) -> FeeType {
        match self {
            Self::Deprecated(_) => FeeType::Eth,
            Self::Current(_) => FeeType::Strk,
        }
    }
}
