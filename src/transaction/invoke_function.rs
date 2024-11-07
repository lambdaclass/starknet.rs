use super::{
    check_account_tx_fields_version,
    fee::{calculate_tx_fee, charge_fee, check_fee_bounds, run_post_execution_fee_checks},
    get_tx_version, Address, CurrentAccountTxFields, ResourceBounds, Transaction,
    VersionSpecificAccountTxFields,
};
use crate::{
    core::transaction_hash::calculate_invoke_transaction_hash,
    definitions::{
        block_context::BlockContext,
        constants::{
            EXECUTE_ENTRY_POINT_SELECTOR, VALIDATE_ENTRY_POINT_SELECTOR, VALIDATE_RETDATA,
        },
        transaction_type::TransactionType,
    },
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        CallInfo, TransactionExecutionContext, TransactionExecutionInfo,
    },
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::EntryPointType,
    },
    state::{
        cached_state::CachedState,
        contract_class_cache::ContractClassCache,
        state_api::{State, StateReader},
        ExecutionResourcesManager, StateDiff,
    },
    transaction::error::TransactionError,
    utils::calculate_tx_resources,
};
use cairo_vm::Felt252;
use getset::Getters;
use num_traits::Zero;
use starknet_api::transaction::Resource;
use std::fmt::Debug;

#[cfg(feature = "cairo-native")]
use {
    crate::transaction::ClassHash,
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

/// Represents an InvokeFunction transaction in the starknet network.
#[derive(Debug, Getters, Clone)]
pub struct InvokeFunction {
    #[getset(get = "pub")]
    contract_address: Address,
    entry_point_selector: Felt252,
    #[allow(dead_code)]
    entry_point_type: EntryPointType,
    calldata: Vec<Felt252>,
    tx_type: TransactionType,
    version: Felt252,
    validate_entry_point_selector: Felt252,
    #[getset(get = "pub")]
    hash_value: Felt252,
    #[getset(get = "pub")]
    signature: Vec<Felt252>,
    account_tx_fields: VersionSpecificAccountTxFields,
    nonce: Option<Felt252>,
    skip_validation: bool,
    skip_execute: bool,
    skip_fee_transfer: bool,
    skip_nonce_check: bool,
}

impl InvokeFunction {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        contract_address: Address,
        entry_point_selector: Felt252,
        account_tx_fields: VersionSpecificAccountTxFields,
        version: Felt252,
        calldata: Vec<Felt252>,
        signature: Vec<Felt252>,
        chain_id: Felt252,
        nonce: Option<Felt252>,
    ) -> Result<Self, TransactionError> {
        let hash_value = calculate_invoke_transaction_hash(
            chain_id,
            &contract_address,
            entry_point_selector,
            version,
            nonce,
            &calldata,
            &account_tx_fields,
        )?;

        InvokeFunction::new_with_tx_hash(
            contract_address,
            entry_point_selector,
            account_tx_fields,
            version,
            calldata,
            signature,
            nonce,
            hash_value,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_tx_hash(
        contract_address: Address,
        entry_point_selector: Felt252,
        account_tx_fields: VersionSpecificAccountTxFields,
        version: Felt252,
        calldata: Vec<Felt252>,
        signature: Vec<Felt252>,
        nonce: Option<Felt252>,
        hash_value: Felt252,
    ) -> Result<Self, TransactionError> {
        let version = get_tx_version(version);
        check_account_tx_fields_version(&account_tx_fields, version)?;

        let validate_entry_point_selector = *VALIDATE_ENTRY_POINT_SELECTOR;

        Ok(InvokeFunction {
            contract_address,
            entry_point_selector,
            entry_point_type: EntryPointType::External,
            calldata,
            tx_type: TransactionType::InvokeFunction,
            version,
            account_tx_fields,
            signature,
            validate_entry_point_selector,
            nonce,
            hash_value,
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        })
    }

    /// Creates a `InvokeFunction` from a starknet api `InvokeTransaction`.
    pub fn from_invoke_transaction(
        tx: starknet_api::transaction::InvokeTransaction,
        tx_hash: Felt252,
    ) -> Result<Self, TransactionError> {
        let account_tx_fields = match &tx {
            starknet_api::transaction::InvokeTransaction::V0(tx) => {
                VersionSpecificAccountTxFields::Deprecated(tx.max_fee.0)
            }
            starknet_api::transaction::InvokeTransaction::V1(tx) => {
                VersionSpecificAccountTxFields::Deprecated(tx.max_fee.0)
            }
            starknet_api::transaction::InvokeTransaction::V3(tx) => {
                VersionSpecificAccountTxFields::Current(CurrentAccountTxFields {
                    l1_resource_bounds: tx
                        .resource_bounds
                        .0
                        .get(&Resource::L1Gas)
                        .map(|r| r.into())
                        .unwrap_or_default(),
                    l2_resource_bounds: tx
                        .resource_bounds
                        .0
                        .get(&Resource::L2Gas)
                        .map(|r| r.into()),
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
        let entry_point_selector = match &tx {
            starknet_api::transaction::InvokeTransaction::V0(tx) => {
                Felt252::from_bytes_be_slice(tx.entry_point_selector.0.bytes())
            }
            starknet_api::transaction::InvokeTransaction::V1(_)
            | starknet_api::transaction::InvokeTransaction::V3(_) => *EXECUTE_ENTRY_POINT_SELECTOR,
        };
        let contract_address = Address(Felt252::from_bytes_be_slice(
            tx.sender_address().0.key().bytes(),
        ));
        let version = Felt252::from_bytes_be_slice(tx.version().0.bytes());
        let nonce = if version.is_zero() {
            None
        } else {
            Some(Felt252::from_bytes_be_slice(tx.nonce().0.bytes()))
        };

        let signature = tx
            .signature()
            .0
            .iter()
            .map(|f| Felt252::from_bytes_be_slice(f.bytes()))
            .collect();
        let calldata = tx
            .calldata()
            .0
            .as_ref()
            .iter()
            .map(|f| Felt252::from_bytes_be_slice(f.bytes()))
            .collect();

        InvokeFunction::new_with_tx_hash(
            contract_address,
            entry_point_selector,
            account_tx_fields,
            version,
            calldata,
            signature,
            nonce,
            tx_hash,
        )
    }

    fn get_execution_context(
        &self,
        n_steps: u64,
    ) -> Result<TransactionExecutionContext, TransactionError> {
        Ok(TransactionExecutionContext::new(
            self.contract_address.clone(),
            self.hash_value,
            self.signature.clone(),
            self.account_tx_fields.clone(),
            if self.version.is_zero() {
                Felt252::ZERO
            } else {
                self.nonce.ok_or(TransactionError::MissingNonce)?
            },
            n_steps,
            self.version,
        ))
    }

    /// Execute the validation entrypoint of the contract and returns the call info.
    /// ## Parameters:
    /// - state: A state that implements the [`State`] and [`StateReader`] traits.
    /// - resources_manager: the resources that are in use by the contract
    /// - block_context: The block's execution context
    pub(crate) fn run_validate_entrypoint<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        resources_manager: &mut ExecutionResourcesManager,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<Option<CallInfo>, TransactionError> {
        if self.version.is_zero() || self.skip_validation {
            return Ok(None);
        }

        let call = ExecutionEntryPoint::new(
            self.contract_address.clone(),
            self.calldata.clone(),
            self.validate_entry_point_selector,
            Address(0.into()),
            EntryPointType::External,
            None,
            None,
            remaining_gas,
        );

        let ExecutionResult { call_info, .. } = call.execute(
            state,
            block_context,
            resources_manager,
            &mut self.get_execution_context(block_context.validate_max_n_steps)?,
            false,
            block_context.validate_max_n_steps,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )?;

        // Validate the return data
        let class_hash = state.get_class_hash_at(&self.contract_address)?;
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
            if !call_info
                .as_ref()
                .map(|ci| ci.retdata == vec![*VALIDATE_RETDATA])
                .unwrap_or_default()
            {
                return Err(TransactionError::WrongValidateRetdata);
            }
        }

        let call_info = verify_no_calls_to_other_contracts(&call_info)
            .map_err(|_| TransactionError::InvalidContractCall)?;

        Ok(Some(call_info))
    }

    /// Builds the transaction execution context and executes the entry point.
    /// Returns the CallInfo.
    fn run_execute_entrypoint<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        resources_manager: &mut ExecutionResourcesManager,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<ExecutionResult, TransactionError> {
        let call = ExecutionEntryPoint::new(
            self.contract_address.clone(),
            self.calldata.clone(),
            self.entry_point_selector,
            Address(Felt252::ZERO),
            EntryPointType::External,
            None,
            None,
            remaining_gas,
        );
        call.execute(
            state,
            block_context,
            resources_manager,
            &mut self.get_execution_context(block_context.invoke_tx_max_n_steps)?,
            true,
            block_context.invoke_tx_max_n_steps,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )
    }

    /// Execute a call to the cairo-vm using the accounts_validation.cairo contract to validate
    /// the contract that is being declared. Then it returns the transaction execution info of the run.
    /// ## Parameters
    /// - state: A state that implements the [`State`] and [`StateReader`] traits.
    /// - block_context: The block's execution context.
    /// - remaining_gas: The amount of gas that the transaction disposes.
    pub fn apply<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        mut remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        let mut resources_manager = ExecutionResourcesManager::default();
        let validate_info = if self.skip_validation {
            None
        } else {
            self.run_validate_entrypoint(
                state,
                block_context,
                &mut resources_manager,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache.clone(),
            )?
        };

        if let Some(call_info) = &validate_info {
            remaining_gas -= call_info.gas_consumed;
        }

        // Execute transaction
        let ExecutionResult {
            call_info,
            revert_error,
            n_reverted_steps,
        } = if self.skip_execute {
            ExecutionResult::default()
        } else {
            self.run_execute_entrypoint(
                state,
                block_context,
                &mut resources_manager,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )?
        };
        let changes = state.count_actual_state_changes(Some((
            (block_context.get_fee_token_address_by_fee_type(&self.account_tx_fields.fee_type())),
            &self.contract_address,
        )))?;
        let actual_resources = calculate_tx_resources(
            resources_manager,
            &[call_info.clone(), validate_info.clone()],
            self.tx_type,
            changes,
            None,
            n_reverted_steps,
        )?;
        let transaction_execution_info = TransactionExecutionInfo::new_without_fee_info(
            validate_info,
            call_info,
            revert_error,
            actual_resources,
            Some(self.tx_type),
        );
        Ok(transaction_execution_info)
    }

    /// Calculates actual fee used by the transaction using the execution info returned by apply(),
    /// then updates the transaction execution info with the data of the fee.
    /// ## Parameters
    /// - state: A state that implements the [`State`] and [`StateReader`] traits.
    /// - block_context: The block's execution context.
    /// - remaining_gas: The amount of gas that the transaction disposes.
    #[tracing::instrument(level = "debug", ret, err, skip(self, state, block_context, program_cache), fields(
        tx_type = ?TransactionType::InvokeFunction,
        self.version = ?self.version,
        self.hash_value = ?self.hash_value,
        self.contract_address = ?self.contract_address,
        self.entry_point_selector = ?self.entry_point_selector,
        self.entry_point_type = ?self.entry_point_type,
    ))]
    pub fn execute<S: StateReader, C: ContractClassCache>(
        &self,
        state: &mut CachedState<S, C>,
        block_context: &BlockContext,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        if !(self.version == Felt252::ZERO
            || self.version == Felt252::ONE
            || self.version == Felt252::THREE)
        {
            return Err(TransactionError::UnsupportedTxVersion(
                "Invoke".to_string(),
                self.version,
                vec![0, 1, 3],
            ));
        }

        self.handle_nonce(state)?;

        if !self.skip_fee_transfer {
            self.check_fee_balance(state, block_context)?;
        }

        let mut transactional_state = state.create_transactional()?;

        let tx_exec_info = self.apply(
            &mut transactional_state,
            block_context,
            remaining_gas,
            #[cfg(feature = "cairo-native")]
            program_cache.clone(),
        );
        #[cfg(feature = "replay_benchmark")]
        // Add initial values to cache despite tx outcome
        {
            state.cache_mut().storage_initial_values_mut().extend(
                transactional_state
                    .cache()
                    .storage_initial_values
                    .clone()
                    .into_iter(),
            );
            state.cache_mut().class_hash_initial_values_mut().extend(
                transactional_state
                    .cache()
                    .class_hash_initial_values
                    .clone()
                    .into_iter(),
            );
        }
        let mut tx_exec_info = tx_exec_info?;

        let actual_fee = calculate_tx_fee(
            &tx_exec_info.actual_resources,
            block_context,
            &self.account_tx_fields.fee_type(),
        )?;

        if let Some(revert_error) = tx_exec_info.revert_error.clone() {
            // execution error
            tx_exec_info = tx_exec_info.to_revert_error(&revert_error);
        } else {
            match run_post_execution_fee_checks(
                &mut transactional_state,
                &self.account_tx_fields,
                block_context,
                actual_fee,
                &tx_exec_info.actual_resources,
                self.contract_address(),
                self.skip_fee_transfer,
            ) {
                Ok(_) => {
                    state.apply_state_update(&StateDiff::from_cached_state(
                        transactional_state.cache(),
                    )?)?;
                }
                Err(TransactionError::FeeCheck(error)) => {
                    tx_exec_info = tx_exec_info.to_revert_error(&error.to_string());
                }
                error => error?,
            }
        }

        let mut tx_execution_context =
            self.get_execution_context(block_context.invoke_tx_max_n_steps)?;

        let (fee_transfer_info, actual_fee) = charge_fee(
            state,
            actual_fee,
            block_context,
            &mut tx_execution_context,
            self.skip_fee_transfer,
            #[cfg(feature = "cairo-native")]
            program_cache.clone(),
        )?;

        tx_exec_info.set_fee_info(actual_fee, fee_transfer_info);

        Ok(tx_exec_info)
    }

    fn handle_nonce<S: State + StateReader>(&self, state: &mut S) -> Result<(), TransactionError> {
        if self.version.is_zero() {
            return Ok(());
        }

        let contract_address = self.contract_address();

        let current_nonce = state.get_nonce_at(contract_address)?;
        match &self.nonce {
            None => {
                // TODO: Remove this once we have a better way to handle the nonce.
                Ok(())
            }
            Some(nonce) => {
                if !self.skip_nonce_check && nonce != &current_nonce {
                    return Err(TransactionError::InvalidTransactionNonce(
                        current_nonce.to_string(),
                        nonce.to_string(),
                    ));
                }
                state.increment_nonce(contract_address)?;
                Ok(())
            }
        }
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
            super::fee::AccountTxType::Invoke,
        )?;
        // Check that the current balance is high enough to cover the max_fee
        let (balance_low, balance_high) = state.get_fee_token_balance(
            block_context,
            self.contract_address(),
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

    // Simulation function

    pub fn create_for_simulation(
        &self,
        skip_validation: bool,
        skip_execute: bool,
        skip_fee_transfer: bool,
        ignore_max_fee: bool,
        skip_nonce_check: bool,
    ) -> Transaction {
        let tx = InvokeFunction {
            skip_validation,
            skip_execute,
            skip_fee_transfer,
            skip_nonce_check,
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
            ..self.clone()
        };

        Transaction::InvokeFunction(tx)
    }
}

// ------------------------------------
//  Invoke internal functions utils
// ------------------------------------

pub fn verify_no_calls_to_other_contracts(
    call_info: &Option<CallInfo>,
) -> Result<CallInfo, TransactionError> {
    let call_info = call_info.clone().ok_or(TransactionError::CallInfoIsNone)?;
    let invoked_contract_address = call_info.contract_address.clone();
    for internal_call in call_info.gen_call_topology() {
        if internal_call.contract_address != invoked_contract_address {
            return Err(TransactionError::UnauthorizedActionOnValidate);
        }
    }
    Ok(call_info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        definitions::{block_context::GasPrices, constants::VALIDATE_DECLARE_ENTRY_POINT_SELECTOR},
        services::api::contract_classes::{
            compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
        },
        state::{
            cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
            in_memory_state_reader::InMemoryStateReader,
        },
        transaction::ClassHash,
        utils::calculate_sn_keccak,
    };
    use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;

    use pretty_assertions_sorted::{assert_eq, assert_eq_sorted};
    use starknet_api::{
        core::{ContractAddress, Nonce, PatriciaKey},
        hash::{StarkFelt, StarkHash},
        transaction::{Fee, InvokeTransaction, InvokeTransactionV1, TransactionSignature},
    };
    use std::sync::Arc;

    #[test]
    fn test_from_invoke_transaction() {
        // https://starkscan.co/tx/0x05b6cf416d56e7c7c519b44e6d06a41657ff6c6a3f2629044fac395e6d200ac4
        // result 0x05b6cf416d56e7c7c519b44e6d06a41657ff6c6a3f2629044fac395e6d200ac4
        let tx = InvokeTransaction::V1(InvokeTransactionV1 {
            sender_address: ContractAddress(
                PatriciaKey::try_from(
                    StarkHash::try_from(
                        "0x00c4658311841a69ce121543af332622bc243cf5593fc4aaf822481c7b7f183d",
                    )
                    .unwrap(),
                )
                .unwrap(),
            ),
            max_fee: Fee(49000000000000),
            signature: TransactionSignature(vec![
                StarkFelt::try_from(
                    "0x18315db8eb360a82ea11f302d6a6a35a11b9df1dc220ec1376c4d4604770dd4",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x5e8642259ac8e99c84cdf88c17385698150eb11dccfb3036ecc2b97c0903d27",
                )
                .unwrap(),
            ]),
            nonce: Nonce(StarkFelt::from(22u32)),
            calldata: starknet_api::transaction::Calldata(Arc::new(vec![
                StarkFelt::try_from("0x1").unwrap(),
                StarkFelt::try_from(
                    "0x0454f0bd015e730e5adbb4f080b075fdbf55654ff41ee336203aa2e1ac4d4309",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x032a99297e1d12a9b91d4f90d5dd4b160d93c84a9e3b4daa916fec14ec852e05",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x0000000000000000000000000000000000000000000000000000000000000002",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x0000000000000000000000000000000000000000000000000000000000000002",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x0383538353434346334616431626237363933663435643237376236313461663",
                )
                .unwrap(),
                StarkFelt::try_from(
                    "0x0393762666334373463313762393535303530383563613961323435643965666",
                )
                .unwrap(),
            ])),
        });

        assert!(InvokeFunction::from_invoke_transaction(tx, Felt252::ONE).is_ok())
    }

    #[test]
    fn test_invoke_apply_without_fees() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: Felt252::from_hex(
                "112e35f48499939272000bd72eb840e502ca4c3aefa8800992e8defb746e0c9",
            )
            .unwrap(),
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into(), 1.into(), 10.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 0.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: Some(0.into()),
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let mut transactional = state.create_transactional().unwrap();
        // Invoke result
        let result = internal_invoke_function
            .apply(
                &mut transactional,
                &BlockContext::default(),
                0,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();
        state
            .apply_state_update(&StateDiff::from_cached_state(transactional.cache()).unwrap())
            .unwrap();

        assert_eq!(result.tx_type, Some(TransactionType::InvokeFunction));
        assert_eq!(
            result.call_info.as_ref().unwrap().class_hash,
            Some(class_hash)
        );
        assert_eq!(
            result.call_info.as_ref().unwrap().entry_point_selector,
            Some(internal_invoke_function.entry_point_selector)
        );
        assert_eq!(
            result.call_info.as_ref().unwrap().calldata,
            internal_invoke_function.calldata
        );
        assert_eq!(result.call_info.unwrap().retdata, vec![Felt252::from(144)]);
    }

    #[test]
    fn test_invoke_execute() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: Felt252::from_hex(
                "0x112e35f48499939272000bd72eb840e502ca4c3aefa8800992e8defb746e0c9",
            )
            .unwrap(),
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into(), 1.into(), 10.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 0.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: Some(0.into()),
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let result = internal_invoke_function
            .execute(
                &mut state,
                &BlockContext::default(),
                0,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        assert_eq!(result.tx_type, Some(TransactionType::InvokeFunction));
        assert_eq!(
            result.call_info.as_ref().unwrap().class_hash,
            Some(class_hash)
        );
        assert_eq!(
            result.call_info.as_ref().unwrap().entry_point_selector,
            Some(internal_invoke_function.entry_point_selector)
        );
        assert_eq!(
            result.call_info.as_ref().unwrap().calldata,
            internal_invoke_function.calldata
        );
        assert_eq!(result.call_info.unwrap().retdata, vec![Felt252::from(144)]);
    }

    #[test]
    fn test_apply_invoke_entrypoint_not_found_should_fail() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: (*EXECUTE_ENTRY_POINT_SELECTOR),
            entry_point_type: EntryPointType::External,
            calldata: Vec::new(),
            tx_type: TransactionType::InvokeFunction,
            version: 0.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: Some(0.into()),
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/amm.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let mut transactional = state.create_transactional().unwrap();
        let expected_error = internal_invoke_function.apply(
            &mut transactional,
            &BlockContext::default(),
            0,
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert!(expected_error.is_err());
        assert_matches!(
            expected_error.unwrap_err(),
            TransactionError::EntryPointNotFound(_)
        );
    }

    #[test]
    fn test_apply_v0_with_no_nonce() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: Felt252::from_hex(
                "0x112e35f48499939272000bd72eb840e502ca4c3aefa8800992e8defb746e0c9",
            )
            .unwrap(),
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into(), 1.into(), 10.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 0.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: None,
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let mut transactional = state.create_transactional().unwrap();
        // Invoke result
        let result = internal_invoke_function
            .apply(
                &mut transactional,
                &BlockContext::default(),
                0,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();
        state
            .apply_state_update(&StateDiff::from_cached_state(transactional.cache()).unwrap())
            .unwrap();

        assert_eq!(result.tx_type, Some(TransactionType::InvokeFunction));
        assert_eq!(
            result.call_info.as_ref().unwrap().class_hash,
            Some(class_hash)
        );
        assert_eq!(
            result.call_info.as_ref().unwrap().entry_point_selector,
            Some(internal_invoke_function.entry_point_selector)
        );
        assert_eq!(
            result.call_info.as_ref().unwrap().calldata,
            internal_invoke_function.calldata
        );
        assert_eq!(result.call_info.unwrap().retdata, vec![Felt252::from(144)]);
    }

    #[test]
    fn test_run_validate_entrypoint_nonce_is_none_should_fail() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: (*EXECUTE_ENTRY_POINT_SELECTOR),
            entry_point_type: EntryPointType::External,
            calldata: Vec::new(),
            tx_type: TransactionType::InvokeFunction,
            version: 1.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: None,
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/amm.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let mut transactional = state.create_transactional().unwrap();
        // Invoke result
        let expected_error = internal_invoke_function.apply(
            &mut transactional,
            &BlockContext::default(),
            0,
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert!(expected_error.is_err());
        assert_matches!(expected_error.unwrap_err(), TransactionError::MissingNonce);
    }

    #[test]
    // Test fee calculation is done correctly but payment to sequencer fails due
    // to the token contract not being deployed
    fn test_invoke_with_non_deployed_fee_token_should_fail() {
        let contract_address = Address(0.into());

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();
        // Set contract_state
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address.clone(), nonce);

        let internal_invoke_function = InvokeFunction {
            contract_address,
            entry_point_selector: Felt252::from_hex(
                "0x112e35f48499939272000bd72eb840e502ca4c3aefa8800992e8defb746e0c9",
            )
            .unwrap(),
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into(), 1.into(), 10.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 1.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: VersionSpecificAccountTxFields::new_deprecated(1000),
            nonce: Some(0.into()),
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let block_context = BlockContext::default();

        let result = internal_invoke_function.execute(
            &mut state,
            &block_context,
            0,
            #[cfg(feature = "cairo-native")]
            None,
        );
        assert!(result.is_err());
        assert_matches!(
            result.unwrap_err(),
            TransactionError::MaxFeeExceedsBalance(_, _, _)
        );
    }

    #[test]
    fn test_execute_invoke_actual_fee_exceeded_max_fee_should_fail() {
        let max_fee = 5;
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR,
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 0.into(),
            validate_entry_point_selector: *VALIDATE_ENTRY_POINT_SELECTOR,
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: VersionSpecificAccountTxFields::new_deprecated(max_fee),
            nonce: Some(0.into()),
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: true,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class =
            ContractClass::from_path("starknet_programs/account_without_validation.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let mut block_context = BlockContext::default();
        block_context.starknet_os_config.gas_price = GasPrices::new(1, 0);

        let tx_info = internal_invoke_function
            .execute(
                &mut state,
                &block_context,
                0,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();
        let expected_actual_fee = 899;
        let expected_tx_info = tx_info.clone().to_revert_error(
            format!(
                "Calculated fee ({}) exceeds max fee ({})",
                expected_actual_fee, max_fee
            )
            .as_str(),
        );

        assert_eq_sorted!(tx_info, expected_tx_info);
    }

    #[test]
    fn test_execute_invoke_twice_should_fail() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: *VALIDATE_ENTRY_POINT_SELECTOR,
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into(), 1.into(), 1.into(), 1.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 1.into(),
            validate_entry_point_selector: *VALIDATE_ENTRY_POINT_SELECTOR,
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: Some(0.into()),
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class =
            ContractClass::from_path("starknet_programs/account_without_validation.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        internal_invoke_function
            .execute(
                &mut state,
                &BlockContext::default(),
                0,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        let expected_error = internal_invoke_function.execute(
            &mut state,
            &BlockContext::default(),
            0,
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert!(expected_error.is_err());
        assert_matches!(
            expected_error.unwrap_err(),
            TransactionError::InvalidTransactionNonce(..)
        )
    }

    #[test]
    fn test_execute_inovoke_nonce_missing_should_fail() {
        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector: Felt252::from_hex(
                "0x112e35f48499939272000bd72eb840e502ca4c3aefa8800992e8defb746e0c9",
            )
            .unwrap(),
            entry_point_type: EntryPointType::External,
            calldata: vec![1.into(), 1.into(), 10.into()],
            tx_type: TransactionType::InvokeFunction,
            version: 1.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: None,
            skip_validation: false,
            skip_execute: false,
            skip_fee_transfer: false,
            skip_nonce_check: false,
        };

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/fibonacci.json").unwrap();
        // Set contract_state
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let mut state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        state
            .set_contract_class(
                &class_hash,
                &CompiledClass::Deprecated(Arc::new(contract_class)),
            )
            .unwrap();

        let expected_error = internal_invoke_function.execute(
            &mut state,
            &BlockContext::default(),
            0,
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert!(expected_error.is_err());
        assert_matches!(expected_error.unwrap_err(), TransactionError::MissingNonce)
    }

    #[test]
    // the test should try to make verify_no_calls_to_other_contracts fail
    fn verify_no_calls_to_other_contracts_should_fail() {
        let mut call_info = CallInfo::default();
        let mut internal_calls = Vec::new();
        let internal_call = CallInfo {
            contract_address: Address(1.into()),
            ..Default::default()
        };
        internal_calls.push(internal_call);
        call_info.internal_calls = internal_calls;

        let expected_error = verify_no_calls_to_other_contracts(&Some(call_info));

        assert!(expected_error.is_err());
        assert_matches!(
            expected_error.unwrap_err(),
            TransactionError::UnauthorizedActionOnValidate
        );
    }

    #[test]
    fn test_reverted_transaction_wrong_entry_point() {
        let entry_point_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"factorial_"));

        let internal_invoke_function = InvokeFunction {
            contract_address: Address(0.into()),
            entry_point_selector,
            entry_point_type: EntryPointType::External,
            calldata: vec![],
            tx_type: TransactionType::InvokeFunction,
            version: 0.into(),
            validate_entry_point_selector: 0.into(),
            hash_value: 0.into(),
            signature: Vec::new(),
            account_tx_fields: Default::default(),
            nonce: Some(0.into()),
            skip_validation: true,
            skip_execute: false,
            skip_fee_transfer: true,
            skip_nonce_check: false,
        };

        let mut state_reader = InMemoryStateReader::default();
        let class_hash: ClassHash = ClassHash([1; 32]);
        let program_data = include_bytes!("../../starknet_programs/cairo2/factorial.casm");
        let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
        let contract_address = Address(0.into());
        let nonce = Felt252::ZERO;

        state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);
        state_reader
            .class_hash_to_compiled_class_hash
            .insert(class_hash, class_hash);
        // last is necessary so the transactional state can cache the class

        let casm_contract_class_cache = PermanentContractClassCache::default();

        casm_contract_class_cache.set_contract_class(
            class_hash,
            CompiledClass::Casm {
                casm: Arc::new(contract_class),
                sierra: None,
            },
        );

        let mut state =
            CachedState::new(Arc::new(state_reader), Arc::new(casm_contract_class_cache));

        let state_before_execution = state.clone_for_testing();

        let result = internal_invoke_function
            .execute(
                &mut state,
                &BlockContext::default(),
                0,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        assert!(result.call_info.is_none());
        assert_eq!(
            result.revert_error,
            Some(format!(
                "Requested entry point with selector {entry_point_selector:#x} was not found"
            ))
        );
        assert_eq_sorted!(
            state.cache.class_hash_writes,
            state_before_execution.cache.class_hash_writes
        );
        assert_eq_sorted!(
            state.cache.compiled_class_hash_writes,
            state_before_execution.cache.compiled_class_hash_writes
        );
        assert_eq_sorted!(
            state.cache.nonce_writes,
            state_before_execution.cache.nonce_writes
        );
        assert_eq_sorted!(
            state.cache.storage_writes,
            state_before_execution.cache.storage_writes
        );
        assert_eq_sorted!(
            state.cache.class_hash_to_compiled_class_hash,
            state_before_execution
                .cache
                .class_hash_to_compiled_class_hash
        );
    }

    #[test]
    fn invoke_wrong_version() {
        // declare tx
        let internal_declare = InvokeFunction::new(
            Address(Felt252::ONE),
            Felt252::ONE,
            VersionSpecificAccountTxFields::new_deprecated(9000),
            2.into(),
            vec![],
            vec![],
            Felt252::ONE,
            Some(Felt252::ZERO),
        )
        .unwrap();
        let result = internal_declare.execute(
            &mut CachedState::<InMemoryStateReader, PermanentContractClassCache>::default(),
            &BlockContext::default(),
            u128::MAX,
            #[cfg(feature = "cairo-native")]
            None,
        );

        assert_matches!(
        result,
        Err(TransactionError::UnsupportedTxVersion(tx, ver, supp))
        if tx == "Invoke" && ver == 2.into() && supp == vec![0, 1, 3]);
    }
}
