use crate::core::contract_address::compute_deprecated_class_hash;
use crate::core::transaction_hash::deprecated::calculate_declare_deprecated_transaction_hash;
use crate::definitions::block_context::{BlockContext, FeeType};
use crate::definitions::constants::VALIDATE_DECLARE_ENTRY_POINT_SELECTOR;
use crate::definitions::transaction_type::TransactionType;
use crate::services::api::contract_classes::deprecated_contract_class::EntryPointType;
use crate::state::cached_state::CachedState;
use crate::state::contract_class_cache::ContractClassCache;
use crate::state::state_api::{State, StateReader};
use crate::{
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        CallInfo, TransactionExecutionContext, TransactionExecutionInfo,
    },
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
    },
    state::ExecutionResourcesManager,
    transaction::error::TransactionError,
    utils::{calculate_tx_resources, felt_to_hash, verify_no_calls_to_other_contracts},
};
use cairo_vm::Felt252;
use num_traits::Zero;

use super::fee::{
    calculate_tx_fee, charge_fee, estimate_minimal_l1_gas, run_post_execution_fee_checks,
};
use super::{get_tx_version, Address, ClassHash, Transaction};
use std::fmt::Debug;
use std::sync::Arc;

#[cfg(feature = "cairo-native")]
use {
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

// ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
///  Represents an internal transaction in the StarkNet network that is a declaration of a Cairo
///  contract class.
#[derive(Debug, Clone)]
pub struct DeclareDeprecated {
    pub class_hash: ClassHash,
    pub sender_address: Address,
    pub validate_entry_point_selector: Felt252,
    pub version: Felt252,
    pub max_fee: u128,
    pub signature: Vec<Felt252>,
    pub nonce: Felt252,
    pub hash_value: Felt252,
    pub contract_class: ContractClass,
    pub skip_validate: bool,
    pub skip_execute: bool,
    pub skip_fee_transfer: bool,
    pub skip_nonce_check: bool,
}

// ------------------------------------------------------------
//                        Functions
// ------------------------------------------------------------
impl DeclareDeprecated {
    #[allow(clippy::too_many_arguments)]
    /// Constructor creates a new Declare instance.
    pub fn new(
        contract_class: ContractClass,
        chain_id: Felt252,
        sender_address: Address,
        max_fee: u128,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
    ) -> Result<Self, TransactionError> {
        let version = get_tx_version(version);
        let hash = compute_deprecated_class_hash(&contract_class)?;
        let class_hash = felt_to_hash(&hash);

        let hash_value = calculate_declare_deprecated_transaction_hash(
            &contract_class,
            chain_id,
            &sender_address,
            max_fee,
            version,
            nonce,
        )?;

        let validate_entry_point_selector = *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR;

        let internal_declare = DeclareDeprecated {
            class_hash,
            sender_address,
            validate_entry_point_selector,
            version,
            max_fee,
            signature,
            nonce,
            hash_value,
            contract_class,
            skip_execute: false,
            skip_validate: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        Ok(internal_declare)
    }

    /// Creates a new Declare instance with a given transaction hash.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tx_hash(
        contract_class: ContractClass,
        sender_address: Address,
        max_fee: u128,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
        hash_value: Felt252,
    ) -> Result<Self, TransactionError> {
        let version = get_tx_version(version);

        let hash = compute_deprecated_class_hash(&contract_class)?;
        let class_hash = felt_to_hash(&hash);

        let validate_entry_point_selector = *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR;

        let internal_declare = DeclareDeprecated {
            class_hash,
            sender_address,
            validate_entry_point_selector,
            version,
            max_fee,
            signature,
            nonce,
            hash_value,
            contract_class,
            skip_execute: false,
            skip_validate: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        Ok(internal_declare)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tx_and_class_hash(
        contract_class: ContractClass,
        sender_address: Address,
        max_fee: u128,
        version: Felt252,
        signature: Vec<Felt252>,
        nonce: Felt252,
        hash_value: Felt252,
        class_hash: ClassHash,
    ) -> Result<Self, TransactionError> {
        let version = get_tx_version(version);
        let validate_entry_point_selector = *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR;

        let internal_declare = DeclareDeprecated {
            class_hash,
            sender_address,
            validate_entry_point_selector,
            version,
            max_fee,
            signature,
            nonce,
            hash_value,
            contract_class,
            skip_execute: false,
            skip_validate: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        Ok(internal_declare)
    }

    /// Returns the calldata.
    pub fn get_calldata(&self) -> Vec<Felt252> {
        let bytes = Felt252::from_bytes_be(&self.class_hash.0);
        Vec::from([bytes])
    }

    /// Executes a call to the cairo-vm using the accounts_validation.cairo contract to validate
    /// the contract that is being declared. Then it returns the transaction execution info of the run.
    pub fn apply<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        // validate transaction
        let mut resources_manager = ExecutionResourcesManager::default();
        let validate_info = if self.skip_validate {
            None
        } else {
            self.run_validate_entrypoint(
                state,
                block_context,
                &mut resources_manager,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )?
        };
        let changes = state.count_actual_state_changes(Some((
            (block_context.get_fee_token_address_by_fee_type(&FeeType::Eth)),
            &self.sender_address,
        )))?;
        let actual_resources = calculate_tx_resources(
            resources_manager,
            &vec![validate_info.clone()],
            TransactionType::Declare,
            changes,
            None,
            0,
        )
        .map_err(|_| TransactionError::ResourcesCalculation)?;

        Ok(TransactionExecutionInfo::new_without_fee_info(
            validate_info,
            None,
            None,
            actual_resources,
            Some(TransactionType::Declare),
        ))
    }

    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~
    // Internal Account Functions
    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~

    /// Return the transaction execution context.
    pub fn get_execution_context(&self, n_steps: u64) -> TransactionExecutionContext {
        TransactionExecutionContext::new(
            self.sender_address.clone(),
            self.hash_value,
            self.signature.clone(),
            super::VersionSpecificAccountTxFields::new_deprecated(self.max_fee),
            self.nonce,
            n_steps,
            self.version,
        )
    }

    /// Runs the validation entry point for the contract that is being declared.
    pub fn run_validate_entrypoint<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        resources_manager: &mut ExecutionResourcesManager,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<Option<CallInfo>, TransactionError> {
        if self.version.is_zero() {
            return Ok(None);
        }

        let calldata = self.get_calldata();

        let entry_point = ExecutionEntryPoint::new(
            self.sender_address.clone(),
            calldata,
            self.validate_entry_point_selector,
            Address(Felt252::ZERO),
            EntryPointType::External,
            None,
            None,
            0,
        );

        let ExecutionResult { call_info, .. } = entry_point.execute(
            state,
            block_context,
            resources_manager,
            &mut self.get_execution_context(block_context.invoke_tx_max_n_steps),
            false,
            block_context.validate_max_n_steps,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )?;

        let call_info = call_info.ok_or(TransactionError::CallInfoIsNone)?;

        verify_no_calls_to_other_contracts(&call_info)
            .map_err(|_| TransactionError::UnauthorizedActionOnValidate)?;

        Ok(Some(call_info))
    }

    /// Handles the nonce value, verifies that the transaction nonce is correct.
    fn handle_nonce<S: State + StateReader>(&self, state: &mut S) -> Result<(), TransactionError> {
        if self.version.is_zero() {
            return Ok(());
        }

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
        if self.max_fee.is_zero() {
            return Ok(());
        }
        let minimal_l1_gas_amount =
            estimate_minimal_l1_gas(block_context, super::fee::AccountTxType::Declare)?;
        let minimal_fee =
            minimal_l1_gas_amount * block_context.get_gas_price_by_fee_type(&FeeType::Eth);
        // Check max fee is at least the estimated constant overhead.
        if self.max_fee < minimal_fee {
            return Err(TransactionError::MaxFeeTooLow(self.max_fee, minimal_fee));
        }
        // Check that the current balance is high enough to cover the max_fee
        let (balance_low, balance_high) =
            state.get_fee_token_balance(block_context, &self.sender_address, &FeeType::Eth)?;
        // The fee is at most 128 bits, while balance is 256 bits (split into two 128 bit words).
        if balance_high.is_zero() && balance_low < Felt252::from(self.max_fee) {
            return Err(TransactionError::MaxFeeExceedsBalance(
                self.max_fee,
                balance_low,
                balance_high,
            ));
        }
        Ok(())
    }

    /// Calculates actual fee used by the transaction using the execution
    /// info returned by apply(), then updates the transaction execution info with the data of the fee.
    #[tracing::instrument(level = "debug", ret, err, skip(self, state, block_context, program_cache), fields(
        tx_type = ?TransactionType::Declare,
        self.version = ?self.version,
        self.class_hash = ?self.class_hash,
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
        if !(self.version == Felt252::ZERO || self.version == Felt252::ONE) {
            return Err(TransactionError::UnsupportedTxVersion(
                "Declare".to_string(),
                self.version,
                vec![0, 1],
            ));
        }

        self.handle_nonce(state)?;

        if !self.skip_fee_transfer {
            self.check_fee_balance(state, block_context)?;
        }

        let mut tx_exec_info = self.apply(
            state,
            block_context,
            #[cfg(feature = "cairo-native")]
            program_cache.clone(),
        )?;

        let mut tx_execution_context =
            self.get_execution_context(block_context.invoke_tx_max_n_steps);

        let calculated_fee = calculate_tx_fee(
            &tx_exec_info.actual_resources,
            block_context,
            &tx_execution_context.account_tx_fields.fee_type(),
        )?;

        run_post_execution_fee_checks(
            state,
            &tx_execution_context.account_tx_fields,
            block_context,
            calculated_fee,
            &tx_exec_info.actual_resources,
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

        state.set_contract_class(
            &self.class_hash,
            &CompiledClass::Deprecated(Arc::new(self.contract_class.clone())),
        )?;

        tx_exec_info.set_fee_info(actual_fee, fee_transfer_info);

        Ok(tx_exec_info)
    }

    /// Creates a transaction for simulation.
    pub fn create_for_simulation(
        &self,
        skip_validate: bool,
        skip_execute: bool,
        skip_fee_transfer: bool,
        ignore_max_fee: bool,
        skip_nonce_check: bool,
    ) -> Transaction {
        let tx = DeclareDeprecated {
            skip_validate,
            skip_execute,
            skip_fee_transfer,
            // Keep the max_fee value for V0 for validation
            max_fee: if ignore_max_fee && !self.version.is_zero() {
                u128::MAX
            } else {
                self.max_fee
            },
            skip_nonce_check,
            ..self.clone()
        };

        Transaction::DeclareDeprecated(tx)
    }
}

// ---------------
//     Tests
// ---------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        definitions::{
            block_context::{BlockContext, GasPrices, StarknetChainId},
            constants::VALIDATE_DECLARE_ENTRY_POINT_SELECTOR,
            transaction_type::TransactionType,
        },
        execution::CallType,
        services::api::contract_classes::{
            compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
        },
        state::in_memory_state_reader::InMemoryStateReader,
        state::{cached_state::CachedState, contract_class_cache::PermanentContractClassCache},
        utils::felt_to_hash,
    };
    use cairo_vm::{vm::runners::cairo_runner::ExecutionResources, Felt252};

    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    /// This test verifies the declaration of a Fibonacci contract.
    #[test]
    fn declare_fibonacci() {
        // accounts contract class must be stored before running declaration of fibonacci
        let contract_class =
            ContractClass::from_path("starknet_programs/account_without_validation.json").unwrap();

        // Instantiate CachedState
        let contract_class_cache = PermanentContractClassCache::default();

        //  ------------ contract data --------------------
        let hash = compute_deprecated_class_hash(&contract_class).unwrap();
        let class_hash = ClassHash::from(hash);

        contract_class_cache.set_contract_class(
            class_hash,
            CompiledClass::Deprecated(Arc::new(contract_class.clone())),
        );

        // store sender_address
        let sender_address = Address(1.into());
        // this is not conceptually correct as the sender address would be an
        // Account contract (not the contract that we are currently declaring)
        // but for testing reasons its ok

        let mut state_reader = InMemoryStateReader::default();
        state_reader
            .address_to_class_hash_mut()
            .insert(sender_address.clone(), class_hash);
        state_reader
            .address_to_nonce_mut()
            .insert(sender_address, Felt252::ONE);

        let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

        //* ---------------------------------------
        //*    Test declare with previous data
        //* ---------------------------------------

        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // declare tx
        let internal_declare = DeclareDeprecated::new(
            fib_contract_class,
            chain_id,
            Address(Felt252::ONE),
            0,
            1.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();

        //* ---------------------------------------
        //              Expected result
        //* ---------------------------------------

        // Value generated from selector _validate_declare_
        let entry_point_selector = Some(*VALIDATE_DECLARE_ENTRY_POINT_SELECTOR);

        let class_hash_felt = compute_deprecated_class_hash(&contract_class).unwrap();
        let expected_class_hash = felt_to_hash(&class_hash_felt);

        // Calldata is the class hash represented as a Felt252
        let calldata = [Felt252::from_dec_str(
            "151449101692423517761547521693863750221386499114738230243355039033913267347",
        )
        .unwrap()]
        .to_vec();

        let validate_info = Some(CallInfo {
            caller_address: Address(0.into()),
            call_type: Some(CallType::Call),
            contract_address: Address(Felt252::ONE),
            entry_point_selector,
            entry_point_type: Some(EntryPointType::External),
            calldata,
            class_hash: Some(expected_class_hash),
            execution_resources: Some(ExecutionResources {
                n_steps: 12,
                ..Default::default()
            }),
            ..Default::default()
        });

        let actual_resources = HashMap::from([
            ("n_steps".to_string(), 2921),
            ("l1_gas_usage".to_string(), 862),
            ("range_check_builtin".to_string(), 63),
            ("pedersen_builtin".to_string(), 15),
        ]);
        let transaction_exec_info = TransactionExecutionInfo {
            validate_info,
            call_info: None,
            revert_error: None,
            fee_transfer_info: None,
            actual_fee: 0,
            actual_resources,
            tx_type: Some(TransactionType::Declare),
        };

        // ---------------------
        //      Comparison
        // ---------------------
        assert_eq!(
            internal_declare
                .apply(
                    &mut state,
                    &BlockContext::default(),
                    #[cfg(feature = "cairo-native")]
                    None,
                )
                .unwrap(),
            transaction_exec_info
        );
    }

    /// This test checks that redeclaring a contract class that has already been declared succeed.
    #[test]
    fn execute_class_already_declared_should_redeclare() {
        // accounts contract class must be stored before running declaration of fibonacci
        let path = PathBuf::from("starknet_programs/account_without_validation.json");
        let contract_class = ContractClass::from_path(path).unwrap();

        // Instantiate CachedState
        let contract_class_cache = PermanentContractClassCache::default();

        //  ------------ contract data --------------------
        let hash = compute_deprecated_class_hash(&contract_class).unwrap();
        let class_hash = felt_to_hash(&hash);

        contract_class_cache.set_contract_class(
            class_hash,
            CompiledClass::Deprecated(Arc::new(contract_class)),
        );

        // store sender_address
        let sender_address = Address(1.into());
        // this is not conceptually correct as the sender address would be an
        // Account contract (not the contract that we are currently declaring)
        // but for testing reasons its ok

        let mut state_reader = InMemoryStateReader::default();
        state_reader
            .address_to_class_hash_mut()
            .insert(sender_address.clone(), class_hash);
        state_reader
            .address_to_nonce_mut()
            .insert(sender_address, Felt252::ZERO);

        let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

        //* ---------------------------------------
        //*    Test declare with previous data
        //* ---------------------------------------

        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // Declare same class twice
        let internal_declare = DeclareDeprecated::new(
            fib_contract_class.clone(),
            chain_id,
            Address(Felt252::ONE),
            0,
            1.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();

        let second_internal_declare = DeclareDeprecated::new(
            fib_contract_class,
            chain_id,
            Address(Felt252::ONE),
            0,
            1.into(),
            Vec::new(),
            Felt252::ONE,
        )
        .unwrap();

        internal_declare
            .execute(
                &mut state,
                &BlockContext::default(),
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        assert!(state.get_contract_class(&class_hash).is_ok());

        second_internal_declare
            .execute(
                &mut state,
                &BlockContext::default(),
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        assert!(state.get_contract_class(&class_hash).is_ok());
    }

    /// This test verifies that executing the same transaction twice should fail.
    #[test]
    fn execute_transaction_twice_should_fail() {
        // accounts contract class must be stored before running declaration of fibonacci
        let path = PathBuf::from("starknet_programs/account_without_validation.json");
        let contract_class = ContractClass::from_path(path).unwrap();

        // Instantiate CachedState
        let contract_class_cache = PermanentContractClassCache::default();

        //  ------------ contract data --------------------
        let hash = compute_deprecated_class_hash(&contract_class).unwrap();
        let class_hash = felt_to_hash(&hash);

        contract_class_cache.set_contract_class(
            class_hash,
            CompiledClass::Deprecated(Arc::new(contract_class)),
        );

        // store sender_address
        let sender_address = Address(1.into());
        // this is not conceptually correct as the sender address would be an
        // Account contract (not the contract that we are currently declaring)
        // but for testing reasons its ok

        let mut state_reader = InMemoryStateReader::default();
        state_reader
            .address_to_class_hash_mut()
            .insert(sender_address.clone(), class_hash);
        state_reader
            .address_to_nonce_mut()
            .insert(sender_address, Felt252::ZERO);

        let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

        //* ---------------------------------------
        //*    Test declare with previous data
        //* ---------------------------------------

        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // Declare same class twice
        let internal_declare = DeclareDeprecated::new(
            fib_contract_class,
            chain_id,
            Address(Felt252::ONE),
            0,
            1.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();

        internal_declare
            .execute(
                &mut state,
                &BlockContext::default(),
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        let expected_error = internal_declare.execute(
            &mut state,
            &BlockContext::default(),
            #[cfg(feature = "cairo-native")]
            None,
        );

        // ---------------------
        //      Comparison
        // ---------------------

        assert!(expected_error.is_err());
        assert_matches!(
            expected_error.unwrap_err(),
            TransactionError::InvalidTransactionNonce(..)
        )
    }

    /// This test checks that a contract declaration should fail if there are no account contracts in the state.
    #[test]
    fn validate_transaction_should_fail() {
        // Instantiate CachedState
        let contract_class_cache = PermanentContractClassCache::default();

        let state_reader = Arc::new(InMemoryStateReader::default());

        let mut state = CachedState::new(state_reader, Arc::new(contract_class_cache));

        // There are no account contracts in the state, so the transaction should fail
        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        let internal_declare = DeclareDeprecated::new(
            fib_contract_class,
            chain_id,
            Address(Felt252::ONE),
            0,
            1.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();

        let internal_declare_error = internal_declare.execute(
            &mut state,
            &BlockContext::default(),
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert!(internal_declare_error.is_err());
        assert_matches!(
            internal_declare_error.unwrap_err(),
            TransactionError::NotDeployedContract(..)
        );
    }

    // This test verifies that a contract declaration should fail if the fee token contract is not set up.
    #[test]
    fn execute_transaction_charge_fee_should_fail() {
        // accounts contract class must be stored before running declaration of fibonacci
        let path = PathBuf::from("starknet_programs/account_without_validation.json");
        let contract_class = ContractClass::from_path(path).unwrap();

        // Instantiate CachedState
        let contract_class_cache = PermanentContractClassCache::default();

        //  ------------ contract data --------------------
        let hash = compute_deprecated_class_hash(&contract_class).unwrap();
        let class_hash = felt_to_hash(&hash);

        contract_class_cache.set_contract_class(
            class_hash,
            CompiledClass::Deprecated(Arc::new(contract_class)),
        );

        // store sender_address
        let sender_address = Address(1.into());
        // this is not conceptually correct as the sender address would be an
        // Account contract (not the contract that we are currently declaring)
        // but for testing reasons its ok

        let mut state_reader = InMemoryStateReader::default();
        state_reader
            .address_to_class_hash_mut()
            .insert(sender_address.clone(), class_hash);
        state_reader
            .address_to_nonce_mut()
            .insert(sender_address, Felt252::ZERO);

        let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

        //* ---------------------------------------
        //*    Test declare with previous data
        //* ---------------------------------------

        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // Use non-zero value so that the actual fee calculation is done
        let internal_declare = DeclareDeprecated::new(
            fib_contract_class,
            chain_id,
            Address(Felt252::ONE),
            10,
            1.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();

        // We expect a fee transfer failure because the fee token contract is not set up
        assert_matches!(
            internal_declare.execute(
                &mut state,
                &BlockContext::default(),
                #[cfg(feature = "cairo-native")]
                None,
            ),
            Err(TransactionError::MaxFeeExceedsBalance(_, _, _))
        );
    }

    #[test]
    fn declare_v1_with_validation_fee_higher_than_no_validation() {
        // accounts contract class must be stored before running declaration of fibonacci
        let contract_class = ContractClass::from_path("starknet_programs/Account.json").unwrap();

        // Instantiate CachedState
        let contract_class_cache = PermanentContractClassCache::default();

        //  ------------ contract data --------------------
        let hash = compute_deprecated_class_hash(&contract_class).unwrap();
        let class_hash = ClassHash::from(hash);

        contract_class_cache.set_contract_class(
            class_hash,
            CompiledClass::Deprecated(Arc::new(contract_class)),
        );

        // store sender_address
        let sender_address = Address(1.into());
        // this is not conceptually correct as the sender address would be an
        // Account contract (not the contract that we are currently declaring)
        // but for testing reasons its ok

        let mut state_reader = InMemoryStateReader::default();
        state_reader
            .address_to_class_hash_mut()
            .insert(sender_address.clone(), class_hash);
        state_reader
            .address_to_nonce_mut()
            .insert(sender_address.clone(), Felt252::ONE);

        let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));
        // Insert pubkey storage var to pass validation
        let storage_entry = &(
            sender_address,
            Felt252::from_dec_str(
                "1672321442399497129215646424919402195095307045612040218489019266998007191460",
            )
            .unwrap()
            .to_bytes_be(),
        );
        state.set_storage_at(
            storage_entry,
            Felt252::from_dec_str(
                "1735102664668487605176656616876767369909409133946409161569774794110049207117",
            )
            .unwrap(),
        );

        //* ---------------------------------------
        //*    Test declare with previous data
        //* ---------------------------------------

        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // declare tx
        // Signature & tx hash values are hand-picked for account validations to pass
        let mut declare =
            DeclareDeprecated::new(
                fib_contract_class,
                chain_id,
                Address(Felt252::ONE),
                60000,
                1.into(),
                vec![
                Felt252::from_dec_str(
                    "3086480810278599376317923499561306189851900463386393948998357832163236918254"
                ).unwrap(),
                Felt252::from_dec_str(
                    "598673427589502599949712887611119751108407514580626464031881322743364689811"
                ).unwrap(),
            ],
                Felt252::ONE,
            )
            .unwrap();
        declare.skip_fee_transfer = true;
        declare.hash_value = Felt252::from_dec_str("2718").unwrap();

        let simulate_declare = declare
            .clone()
            .create_for_simulation(true, false, true, false, false);

        // ---------------------
        //      Comparison
        // ---------------------
        let mut state_copy = state.clone_for_testing();
        let mut bock_context = BlockContext::default();
        bock_context.block_info_mut().gas_price = GasPrices::new(12, 0);
        assert!(
            declare
                .execute(
                    &mut state,
                    &bock_context,
                    #[cfg(feature = "cairo-native")]
                    None,
                )
                .unwrap()
                .actual_fee
                > simulate_declare
                    .execute(
                        &mut state_copy,
                        &bock_context,
                        0,
                        #[cfg(feature = "cairo-native")]
                        None,
                    )
                    .unwrap()
                    .actual_fee,
        );
    }

    #[test]
    fn declare_wrong_version() {
        let fib_contract_class =
            ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();

        let chain_id = StarknetChainId::TestNet.to_felt();

        // declare tx
        let internal_declare = DeclareDeprecated::new(
            fib_contract_class,
            chain_id,
            Address(Felt252::ONE),
            0,
            2.into(),
            Vec::new(),
            Felt252::ZERO,
        )
        .unwrap();
        let result = internal_declare.execute(
            &mut CachedState::<InMemoryStateReader, PermanentContractClassCache>::default(),
            &BlockContext::default(),
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert_matches!(
        result,
        Err(TransactionError::UnsupportedTxVersion(tx, ver, supp))
        if tx == "Declare" && ver == 2.into() && supp == vec![0, 1]);
    }
}
