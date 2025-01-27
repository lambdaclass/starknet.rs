use super::fee::{calculate_tx_fee, charge_fee, check_fee_bounds, run_post_execution_fee_checks};
use super::{
    check_account_tx_fields_version, get_tx_version, ResourceBounds, Transaction,
    VersionSpecificAccountTxFields,
};
use crate::core::contract_address::{compute_casm_class_hash, compute_sierra_class_hash};
use crate::definitions::constants::VALIDATE_RETDATA;
use crate::execution::execution_entry_point::ExecutionResult;
use crate::services::api::contract_classes::deprecated_contract_class::EntryPointType;

use crate::services::api::contract_classes::compiled_class::CompiledClass;
use crate::state::cached_state::CachedState;
use crate::state::contract_class_cache::ContractClassCache;
use crate::{
    core::transaction_hash::calculate_declare_transaction_hash,
    definitions::{
        block_context::BlockContext,
        constants::{INITIAL_GAS_COST, VALIDATE_DECLARE_ENTRY_POINT_SELECTOR},
        transaction_type::TransactionType,
    },
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallType, TransactionExecutionContext,
        TransactionExecutionInfo,
    },
    state::{
        state_api::{State, StateReader},
        ExecutionResourcesManager,
    },
    transaction::{
        error::TransactionError, invoke_function::verify_no_calls_to_other_contracts, Address,
        ClassHash,
    },
    utils::calculate_tx_resources,
};
use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
use cairo_lang_starknet_classes::contract_class::ContractClass as SierraContractClass;
use cairo_vm::Felt252;
use num_traits::Zero;
use std::fmt::Debug;
use std::sync::Arc;

#[cfg(feature = "cairo-native")]
use {
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

/// Represents a declare transaction in the starknet network.
/// Declare creates a blueprint of a contract class that is used to deploy instances of the contract
/// Declare is meant to be used with the new cairo contract syntax, starting from Cairo1.
#[derive(Debug, Clone)]
pub struct Declare {
    pub sender_address: Address,
    pub validate_entry_point_selector: Felt252,
    pub version: Felt252,
    pub account_tx_fields: VersionSpecificAccountTxFields,
    pub signature: Vec<Felt252>,
    pub nonce: Felt252,
    // maybe change this for ClassHash
    pub compiled_class_hash: Felt252,
    pub sierra_contract_class: Option<SierraContractClass>,
    pub sierra_class_hash: Felt252,
    pub hash_value: Felt252,
    pub casm_class: Option<CasmContractClass>,
    pub skip_validate: bool,
    pub skip_execute: bool,
    pub skip_fee_transfer: bool,
    pub skip_nonce_check: bool,
}

impl Declare {
    /// Creates a new instance of a [Declare].
    /// It will calculate the sierra class hash and the transaction hash.
    /// ## Parameters:
    /// - sierra_contract_class: The sierra contract class of the contract to declare
    /// - casm_contract_class: The casm contract class of the contract to declare. This is optional.
    /// - compiled_class_hash: the class hash of the contract compiled with Cairo1 or newer.
    /// - chain_id: Id of the network where is going to be declare, those can be: Mainnet, Testnet.
    /// - sender_address: The address of the account declaring the contract.
    /// - max_fee: refers to max amount of fee that a declare takes.
    /// - version: The version of cairo contract being declare.
    /// - signature: Array of felts with the signatures of the contract.
    /// - nonce: The nonce of the contract.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sierra_contract_class: &SierraContractClass,
        casm_contract_class: Option<CasmContractClass>,
        compiled_class_hash: Felt252,
        chain_id: Felt252,
        sender_address: Address,
        account_tx_fields: VersionSpecificAccountTxFields,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
    ) -> Result<Self, TransactionError> {
        let sierra_class_hash = compute_sierra_class_hash(sierra_contract_class)?;

        let hash_value = calculate_declare_transaction_hash(
            sierra_class_hash,
            compiled_class_hash,
            version,
            nonce,
            &sender_address,
            chain_id,
            &account_tx_fields,
        )?;

        Self::new_with_sierra_class_hash_and_tx_hash(
            Some(sierra_contract_class.clone()),
            sierra_class_hash,
            casm_contract_class,
            compiled_class_hash,
            sender_address,
            account_tx_fields,
            version,
            signature,
            nonce,
            hash_value,
        )
    }

    /// Creates a new instance of a declare with a precomputed sierra class hash and transaction hash.
    /// ## Parameters:
    /// - sierra_contract_class: The sierra contract class of the contract to declare
    /// - sierra_class_hash: The precomputed hash for the sierra contract
    /// - casm_contract_class: The casm contract class of the contract to declare. This is optional.
    /// - compiled_class_hash: the class hash of the contract compiled with Cairo1 or newer.
    /// - sender_address: The address of the account declaring the contract.
    /// - max_fee: refers to max amount of fee that a declare takes.
    /// - version: The version of cairo contract being declare.
    /// - signature: Array of felts with the signatures of the contract.
    /// - nonce: The nonce of the contract.
    /// - hash_value: The transaction hash_value.
    /// SAFETY: if `sierra_class_hash` doesn't correspond to the `sierra_contract_class` invariants
    /// may not hold.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_sierra_class_hash_and_tx_hash(
        sierra_contract_class: Option<SierraContractClass>,
        sierra_class_hash: Felt252,
        casm_contract_class: Option<CasmContractClass>,
        compiled_class_hash: Felt252,
        sender_address: Address,
        account_tx_fields: VersionSpecificAccountTxFields,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
        hash_value: Felt252,
    ) -> Result<Self, TransactionError> {
        let version = get_tx_version(version);
        check_account_tx_fields_version(&account_tx_fields, version)?;
        let validate_entry_point_selector = *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR;

        let declare = Declare {
            sierra_contract_class: sierra_contract_class.to_owned(),
            sierra_class_hash,
            sender_address,
            validate_entry_point_selector,
            version,
            account_tx_fields,
            signature,
            nonce,
            compiled_class_hash,
            hash_value,
            casm_class: casm_contract_class,
            skip_execute: false,
            skip_validate: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        Ok(declare)
    }

    // creates a new instance of a declare but without the computation of the transaction hash.
    /// ## Parameters:
    /// - sierra_contract_class: The sierra contract class of the contract to declare.
    /// - casm_contract_class: The casm contract class of the contract to declare. This is optional.
    /// - compiled_class_hash: the class hash of the contract compiled with Cairo1 or newer.
    /// - sender_address: The address of the account declaring the contract.
    /// - max_fee: refers to max amount of fee that a declare takes.
    /// - version: The version of cairo contract being declare.
    /// - signature: Array of felts with the signatures of the contract.
    /// - nonce: The nonce of the contract.
    /// - hash_value: The transaction hash.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tx_hash(
        sierra_contract_class: &SierraContractClass,
        casm_contract_class: Option<CasmContractClass>,
        compiled_class_hash: Felt252,
        sender_address: Address,
        account_tx_fields: VersionSpecificAccountTxFields,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
        hash_value: Felt252,
    ) -> Result<Self, TransactionError> {
        let sierra_class_hash = compute_sierra_class_hash(sierra_contract_class)?;

        Self::new_with_sierra_class_hash_and_tx_hash(
            Some(sierra_contract_class.clone()),
            sierra_class_hash,
            casm_contract_class,
            compiled_class_hash,
            sender_address,
            account_tx_fields,
            version,
            signature,
            nonce,
            hash_value,
        )
    }

    /// Creates a new instance of a [Declare] but without the computation of the sierra class hash.
    /// ## Parameters:
    /// - sierra_contract_class: The sierra contract class of the contract to declare
    /// - sierra_class_hash: The precomputed hash for the sierra contract
    /// - casm_contract_class: The casm contract class of the contract to declare. This is optional.
    /// - compiled_class_hash: the class hash of the contract compiled with Cairo1 or newer.
    /// - chain_id: Id of the network where is going to be declare, those can be: Mainnet, Testnet.
    /// - sender_address: The address of the account declaring the contract.
    /// - max_fee: refers to max amount of fee that a declare takes.
    /// - version: The version of cairo contract being declare.
    /// - signature: Array of felts with the signatures of the contract.
    /// - nonce: The nonce of the contract.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_sierra_class_hash(
        sierra_contract_class: Option<SierraContractClass>,
        sierra_class_hash: Felt252,
        casm_contract_class: Option<CasmContractClass>,
        compiled_class_hash: Felt252,
        chain_id: Felt252,
        sender_address: Address,
        account_tx_fields: VersionSpecificAccountTxFields,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
    ) -> Result<Self, TransactionError> {
        let hash_value = calculate_declare_transaction_hash(
            sierra_class_hash,
            compiled_class_hash,
            version,
            nonce,
            &sender_address,
            chain_id,
            &account_tx_fields,
        )?;

        Self::new_with_sierra_class_hash_and_tx_hash(
            sierra_contract_class,
            sierra_class_hash,
            casm_contract_class,
            compiled_class_hash,
            sender_address,
            account_tx_fields,
            version,
            signature,
            nonce,
            hash_value,
        )
    }

    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~
    //  Account Functions
    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~

    /// creates the a new TransactionExecutionContexts which represent the state of the net after executing the contract.
    /// ## Parameter:
    /// n_steps: the number of steps that are required to execute the contract.
    pub fn get_execution_context(&self, n_steps: u64) -> TransactionExecutionContext {
        TransactionExecutionContext::new(
            self.sender_address.clone(),
            self.hash_value,
            self.signature.clone(),
            self.account_tx_fields.clone(),
            self.nonce,
            n_steps,
            self.version,
        )
    }

    /// returns the calldata with which the contract is executed
    pub fn get_calldata(&self) -> Vec<Felt252> {
        let bytes = self.compiled_class_hash;
        Vec::from([bytes])
    }

    fn handle_nonce<S: State + StateReader>(&self, state: &mut S) -> Result<(), TransactionError> {
        let contract_address = &self.sender_address;
        let current_nonce = state.get_nonce_at(contract_address)?;
        if current_nonce != self.nonce && !self.skip_nonce_check {
            return Err(TransactionError::InvalidTransactionNonce(
                current_nonce.to_string(),
                self.nonce.to_string(),
            ));
        }

        state.increment_nonce(contract_address)?;

        Ok(())
    }

    fn check_fee_balance<S: State + StateReader>(
        &self,
        state: &mut S,
        block_context: &BlockContext,
    ) -> Result<(), TransactionError> {
        if self.account_tx_fields.max_fee().is_zero() {
            return Ok(());
        }
        // Check max fee is at least the estimated constant overhead.
        check_fee_bounds(
            &self.account_tx_fields,
            block_context,
            super::fee::AccountTxType::Declare,
        )?;
        // Check that the current balance is high enough to cover the max_fee
        let (balance_low, balance_high) = state.get_fee_token_balance(
            block_context,
            &self.sender_address,
            &self.account_tx_fields.fee_type(),
        )?;
        // The fee is at most 128 bits, while balance is 256 bits (split into two 128 bit words).
        if balance_high.is_zero() && balance_low < Felt252::from(self.account_tx_fields.max_fee()) {
            return Err(TransactionError::MaxFeeExceedsBalance(
                self.account_tx_fields.max_fee(),
                balance_low,
                balance_high,
            ));
        }
        Ok(())
    }

    /// Execute the validation of the contract in the cairo-vm. Returns a TransactionExecutionInfo if succesful.
    /// ## Parameter:
    /// - state: An state that implements the State and StateReader traits.
    /// - block_context: The block that contains the execution context
    #[tracing::instrument(level = "debug", ret, err, skip(self, state, block_context, program_cache), fields(
        tx_type = ?TransactionType::Declare,
        self.version = ?self.version,
        self.sierra_class_hash = ?self.sierra_class_hash,
        self.compiled_class_hash = ?self.compiled_class_hash,
        self.hash_value = ?self.hash_value,
        self.sender_address = ?self.sender_address,
        self.nonce = ?self.nonce,
    ))]
    pub fn execute<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        if !(self.version == Felt252::TWO || self.version == Felt252::THREE) {
            return Err(TransactionError::UnsupportedTxVersion(
                "Declare".to_string(),
                self.version,
                vec![2, 3],
            ));
        }

        self.handle_nonce(state)?;

        if !self.skip_fee_transfer {
            self.check_fee_balance(state, block_context)?;
        }

        let mut resources_manager = ExecutionResourcesManager::default();

        let execution_result = if self.skip_validate {
            ExecutionResult::default()
        } else {
            self.run_validate_entrypoint(
                state,
                block_context,
                &mut resources_manager,
                INITIAL_GAS_COST,
                #[cfg(feature = "cairo-native")]
                program_cache.clone(),
            )?
        };
        self.compile_and_store_casm_class(state)?;

        let storage_changes = state.count_actual_state_changes(Some((
            (block_context.get_fee_token_address_by_fee_type(&self.account_tx_fields.fee_type())),
            &self.sender_address,
        )))?;

        let actual_resources = calculate_tx_resources(
            resources_manager,
            &[execution_result.call_info.clone()],
            TransactionType::Declare,
            storage_changes,
            None,
            execution_result.n_reverted_steps,
        )?;

        let calculated_fee = calculate_tx_fee(
            &actual_resources,
            block_context,
            &self.account_tx_fields.fee_type(),
        )?;

        let mut tx_execution_context =
            self.get_execution_context(block_context.invoke_tx_max_n_steps);

        run_post_execution_fee_checks(
            state,
            &self.account_tx_fields,
            block_context,
            calculated_fee,
            &actual_resources,
            &self.sender_address,
            self.skip_fee_transfer,
        )?;

        let (fee_transfer_info, actual_fee) = charge_fee(
            state,
            calculated_fee,
            block_context,
            &mut tx_execution_context,
            self.skip_fee_transfer,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )?;

        let mut tx_exec_info = TransactionExecutionInfo::new_without_fee_info(
            execution_result.call_info,
            None,
            None,
            actual_resources,
            Some(TransactionType::Declare),
        );
        tx_exec_info.set_fee_info(actual_fee, fee_transfer_info);

        Ok(tx_exec_info)
    }

    pub(crate) fn compile_and_store_casm_class<S: State + StateReader>(
        &self,
        state: &mut S,
    ) -> Result<(), TransactionError> {
        let casm_class = match &self.casm_class {
            None => CasmContractClass::from_contract_class(
                self.sierra_contract_class
                    .clone()
                    .ok_or(TransactionError::DeclareNoSierraOrCasm)?,
                true,
                usize::MAX, // todo check if usize MAX is good here
            )
            .map_err(|e| TransactionError::SierraCompileError(e.to_string()))?,
            Some(casm_contract_class) => casm_contract_class.clone(),
        };

        let casm_class_hash = compute_casm_class_hash(&casm_class)?;
        if casm_class_hash != self.compiled_class_hash {
            return Err(TransactionError::InvalidCompiledClassHash(
                casm_class_hash.to_string(),
                self.compiled_class_hash.to_string(),
            ));
        }
        state
            .set_compiled_class_hash(&self.sierra_class_hash, &self.compiled_class_hash.clone())?;

        let compiled_contract_class = ClassHash::from(self.compiled_class_hash);
        state.set_contract_class(
            &compiled_contract_class,
            &CompiledClass::Casm {
                casm: Arc::new(casm_class),
                sierra: self
                    .sierra_contract_class
                    .as_ref()
                    .map(|contract_class| {
                        Result::<_, TransactionError>::Ok(Arc::new((
                            contract_class.extract_sierra_program().map_err(|e| {
                                TransactionError::CustomError(format!(
                                    "Sierra program extraction failed: {e}"
                                ))
                            })?,
                            contract_class.entry_points_by_type.clone(),
                        )))
                    })
                    .transpose()?,
            },
        )?;

        Ok(())
    }

    fn run_validate_entrypoint<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        resources_manager: &mut ExecutionResourcesManager,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<ExecutionResult, TransactionError> {
        let calldata = [self.compiled_class_hash].to_vec();

        let entry_point = ExecutionEntryPoint {
            contract_address: self.sender_address.clone(),
            entry_point_selector: self.validate_entry_point_selector,
            initial_gas: remaining_gas,
            entry_point_type: EntryPointType::External,
            calldata,
            caller_address: Address(Felt252::ZERO),
            code_address: None,
            class_hash: None,
            call_type: CallType::Call,
        };

        let mut tx_execution_context =
            self.get_execution_context(block_context.validate_max_n_steps);

        let execution_result = if self.skip_execute {
            ExecutionResult::default()
        } else {
            entry_point.execute(
                state,
                block_context,
                resources_manager,
                &mut tx_execution_context,
                true,
                block_context.validate_max_n_steps,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )?
        };

        // Validate the return data
        let class_hash = state.get_class_hash_at(&self.sender_address.clone())?;
        let contract_class = state
            .get_contract_class(&class_hash)
            .map_err(|_| TransactionError::MissingCompiledClass)?;
        if matches!(
            contract_class,
            CompiledClass::Casm {
                sierra: Some(_),
                ..
            }
        ) {
            // The account contract class is a Cairo 1.0 contract; the `validate` entry point should
            // return `VALID`.
            if !execution_result
                .call_info
                .as_ref()
                .map(|ci| &ci.retdata == &vec![*VALIDATE_RETDATA])
                .unwrap_or_default()
            {
                return Err(TransactionError::WrongValidateRetdata);
            }
        }

        if execution_result.call_info.is_some() {
            verify_no_calls_to_other_contracts(&execution_result.call_info)?;
        }

        Ok(execution_result)
    }

    // ---------------
    //   Simulation
    // ---------------
    pub fn create_for_simulation(
        &self,
        skip_validate: bool,
        skip_execute: bool,
        skip_fee_transfer: bool,
        ignore_max_fee: bool,
        skip_nonce_check: bool,
    ) -> Transaction {
        let tx = Declare {
            skip_validate,
            skip_execute,
            skip_fee_transfer,
            account_tx_fields: if ignore_max_fee {
                if let VersionSpecificAccountTxFields::Current(current) = &self.account_tx_fields {
                    let mut current_fields = current.clone();
                    current_fields.l1_resource_bounds = ResourceBounds {
                        max_amount: u64::MAX,
                        max_price_per_unit: u128::MAX,
                    };
                    VersionSpecificAccountTxFields::Current(current_fields)
                } else {
                    VersionSpecificAccountTxFields::new_deprecated(u128::MAX)
                }
            } else {
                self.account_tx_fields.clone()
            },
            skip_nonce_check,
            ..self.clone()
        };

        Transaction::Declare(Box::new(tx))
    }
}

#[cfg(test)]
mod tests {
    use super::Declare;
    use crate::core::contract_address::{compute_casm_class_hash, compute_sierra_class_hash};
    use crate::definitions::block_context::{BlockContext, StarknetChainId};
    use crate::definitions::constants::QUERY_VERSION_2;
    use crate::services::api::contract_classes::compiled_class::CompiledClass;
    use crate::state::state_api::StateReader;
    use crate::transaction::error::TransactionError;
    use crate::{
        state::{
            cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
            in_memory_state_reader::InMemoryStateReader,
        },
        transaction::{Address, ClassHash},
    };
    use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
    use cairo_vm::Felt252;

    use std::{fs::File, io::BufReader, path::PathBuf, sync::Arc};

    #[test]
    fn create_declare_v2_without_casm_contract_class_test() {
        // read file to create sierra contract class
        let version = Felt252::from(2);
        let path = PathBuf::from("starknet_programs/cairo2/fibonacci.sierra");

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
            serde_json::from_reader(reader).unwrap();
        let sender_address = Address(1.into());
        let casm_class =
            CasmContractClass::from_contract_class(sierra_contract_class.clone(), true, usize::MAX)
                .unwrap();
        let casm_class_hash = compute_casm_class_hash(&casm_class).unwrap();

        // create declare

        let declare = Declare::new_with_tx_hash(
            &sierra_contract_class,
            None,
            casm_class_hash,
            sender_address,
            Default::default(),
            version,
            [1.into()].to_vec(),
            Felt252::ZERO,
            Felt252::ONE,
        )
        .unwrap();

        // crate state to store casm contract class
        let casm_contract_class_cache = PermanentContractClassCache::default();
        let state_reader = Arc::new(InMemoryStateReader::default());
        let mut state = CachedState::new(state_reader, Arc::new(casm_contract_class_cache));

        // call compile and store
        assert!(declare.compile_and_store_casm_class(&mut state).is_ok());

        // test we  can retreive the data
        let expected_casm_class = CasmContractClass::from_contract_class(
            declare.sierra_contract_class.unwrap().clone(),
            true,
            usize::MAX,
        )
        .unwrap();
        let declare_compiled_class_hash = ClassHash::from(declare.compiled_class_hash);
        let casm_class = match state
            .get_contract_class(&declare_compiled_class_hash)
            .unwrap()
        {
            CompiledClass::Casm { casm, .. } => casm.as_ref().clone(),
            _ => unreachable!(),
        };

        assert_eq!(expected_casm_class, casm_class);
    }

    #[test]
    fn create_declare_v2_with_casm_contract_class_test() {
        // read file to create sierra contract class
        let version = Felt252::from(2);
        let path = PathBuf::from("starknet_programs/cairo2/fibonacci.sierra");

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
            serde_json::from_reader(reader).unwrap();
        let sender_address = Address(1.into());
        let casm_class =
            CasmContractClass::from_contract_class(sierra_contract_class.clone(), true, usize::MAX)
                .unwrap();
        let casm_class_hash = compute_casm_class_hash(&casm_class).unwrap();

        // create declare

        let declare = Declare::new_with_tx_hash(
            &sierra_contract_class,
            Some(casm_class),
            casm_class_hash,
            sender_address,
            Default::default(),
            version,
            [1.into()].to_vec(),
            Felt252::ZERO,
            Felt252::ONE,
        )
        .unwrap();

        // crate state to store casm contract class
        let casm_contract_class_cache = PermanentContractClassCache::default();
        let state_reader = Arc::new(InMemoryStateReader::default());
        let mut state = CachedState::new(state_reader, Arc::new(casm_contract_class_cache));

        // call compile and store
        assert!(declare.compile_and_store_casm_class(&mut state).is_ok());

        // test we  can retreive the data
        let expected_casm_class = CasmContractClass::from_contract_class(
            declare.sierra_contract_class.unwrap(),
            true,
            usize::MAX,
        )
        .unwrap();
        let declare_compiled_class_hash = ClassHash::from(declare.compiled_class_hash);
        let casm_class = match state
            .get_contract_class(&declare_compiled_class_hash)
            .unwrap()
        {
            CompiledClass::Casm { casm, .. } => casm.as_ref().clone(),
            _ => unreachable!(),
        };

        assert_eq!(expected_casm_class, casm_class);
    }

    #[test]
    fn create_declare_v2_test_with_version_query() {
        // read file to create sierra contract class
        let version = *QUERY_VERSION_2;
        let path = PathBuf::from("starknet_programs/cairo2/fibonacci.sierra");

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
            serde_json::from_reader(reader).unwrap();
        let sierra_class_hash = compute_sierra_class_hash(&sierra_contract_class).unwrap();
        let sender_address = Address(1.into());
        let casm_class =
            CasmContractClass::from_contract_class(sierra_contract_class.clone(), true, usize::MAX)
                .unwrap();
        let casm_class_hash = compute_casm_class_hash(&casm_class).unwrap();

        // create declare tx

        let declare = Declare::new_with_sierra_class_hash_and_tx_hash(
            Some(sierra_contract_class),
            sierra_class_hash,
            Some(casm_class),
            casm_class_hash,
            sender_address,
            Default::default(),
            version,
            vec![],
            Felt252::ZERO,
            Felt252::ZERO,
        )
        .unwrap();

        // crate state to store casm contract class
        let casm_contract_class_cache = PermanentContractClassCache::default();
        let state_reader = Arc::new(InMemoryStateReader::default());
        let mut state = CachedState::new(state_reader, Arc::new(casm_contract_class_cache));

        // call compile and store
        assert!(declare.compile_and_store_casm_class(&mut state).is_ok());

        // test we  can retreive the data
        let expected_casm_class = CasmContractClass::from_contract_class(
            declare.sierra_contract_class.unwrap(),
            true,
            usize::MAX,
        )
        .unwrap();
        let declare_compiled_class_hash = ClassHash::from(declare.compiled_class_hash);
        let casm_class = match state
            .get_contract_class(&declare_compiled_class_hash)
            .unwrap()
        {
            CompiledClass::Casm { casm, .. } => casm.as_ref().clone(),
            _ => unreachable!(),
        };

        assert_eq!(expected_casm_class, casm_class);
    }

    #[test]
    fn create_declare_v2_with_casm_contract_class_none_test() {
        // read file to create sierra contract class
        let version = Felt252::from(2);
        let path = PathBuf::from("starknet_programs/cairo2/fibonacci.sierra");

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
            serde_json::from_reader(reader).unwrap();
        let sender_address = Address(1.into());
        let casm_class =
            CasmContractClass::from_contract_class(sierra_contract_class.clone(), true, usize::MAX)
                .unwrap();
        let casm_class_hash = compute_casm_class_hash(&casm_class).unwrap();

        // create declare tx

        let declare = Declare::new_with_tx_hash(
            &sierra_contract_class,
            None,
            casm_class_hash,
            sender_address,
            Default::default(),
            version,
            [1.into()].to_vec(),
            Felt252::ZERO,
            Felt252::ONE,
        )
        .unwrap();

        // crate state to store casm contract class
        let casm_contract_class_cache = PermanentContractClassCache::default();
        let state_reader = Arc::new(InMemoryStateReader::default());
        let mut state = CachedState::new(state_reader, Arc::new(casm_contract_class_cache));

        // call compile and store
        assert!(declare.compile_and_store_casm_class(&mut state).is_ok());

        // test we  can retreive the data
        let expected_casm_class = CasmContractClass::from_contract_class(
            declare.sierra_contract_class.unwrap().clone(),
            true,
            usize::MAX,
        )
        .unwrap();
        let declare_compiled_class_hash = ClassHash::from(declare.compiled_class_hash);
        let casm_class = match state
            .get_contract_class(&declare_compiled_class_hash)
            .unwrap()
        {
            CompiledClass::Casm { casm, .. } => casm.as_ref().clone(),
            _ => unreachable!(),
        };

        assert_eq!(expected_casm_class, casm_class);
    }

    #[test]
    fn create_declare_v2_wrong_casm_class_hash_test() {
        // read file to create sierra contract class
        let version = Felt252::from(2);
        let path = PathBuf::from("starknet_programs/cairo2/fibonacci.sierra");

        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
            serde_json::from_reader(reader).unwrap();
        let sender_address = Address(1.into());
        let casm_class =
            CasmContractClass::from_contract_class(sierra_contract_class.clone(), true, usize::MAX)
                .unwrap();
        let casm_class_hash = compute_casm_class_hash(&casm_class).unwrap();

        let sended_class_hash = Felt252::from(5);
        // create declare

        let declare = Declare::new_with_tx_hash(
            &sierra_contract_class,
            None,
            sended_class_hash,
            sender_address,
            Default::default(),
            version,
            [1.into()].to_vec(),
            Felt252::ZERO,
            Felt252::ONE,
        )
        .unwrap();

        // crate state to store casm contract class
        let casm_contract_class_cache = PermanentContractClassCache::default();
        let state_reader = Arc::new(InMemoryStateReader::default());
        let mut state = CachedState::new(state_reader, Arc::new(casm_contract_class_cache));

        let expected_err = format!(
            "Invalid compiled class, expected class hash: {}, but received: {}",
            casm_class_hash, sended_class_hash
        );
        assert_eq!(
            declare
                .compile_and_store_casm_class(&mut state)
                .unwrap_err()
                .to_string(),
            expected_err
        );
    }

    #[test]
    fn declarev2_wrong_version() {
        let path = PathBuf::from("starknet_programs/cairo2/fibonacci.sierra");
        let file = File::open(path).unwrap();
        let reader = BufReader::new(file);
        let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
            serde_json::from_reader(reader).unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // declare tx
        let declare = Declare::new(
            &sierra_contract_class,
            None,
            Felt252::ONE,
            chain_id,
            Address(Felt252::ONE),
            Default::default(),
            1.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();
        let result = declare.execute(
            &mut CachedState::<InMemoryStateReader, PermanentContractClassCache>::default(),
            &BlockContext::default(),
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert_matches!(
        result,
        Err(TransactionError::UnsupportedTxVersion(tx, ver, supp))
        if tx == "Declare" && ver == 1.into() && supp == vec![2, 3]);
    }
}
