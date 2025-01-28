use super::{Address, Transaction};
use crate::{
    core::transaction_hash::{
        deprecated::deprecated_calculate_transaction_hash_common, TransactionHashPrefix,
    },
    definitions::{
        block_context::{BlockContext, FeeType},
        constants::L1_HANDLER_VERSION,
        transaction_type::TransactionType,
    },
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        TransactionExecutionContext, TransactionExecutionInfo,
    },
    services::api::contract_classes::deprecated_contract_class::EntryPointType,
    state::{
        cached_state::CachedState,
        contract_class_cache::ContractClassCache,
        state_api::{State, StateReader},
        ExecutionResourcesManager,
    },
    transaction::{error::TransactionError, fee::calculate_tx_fee},
    utils::calculate_tx_resources,
};
use cairo_vm::Felt252;
use getset::Getters;
use num_traits::Zero;

#[cfg(feature = "cairo-native")]
use {
    crate::transaction::ClassHash,
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

#[allow(dead_code)]
#[derive(Debug, Getters, Clone)]
/// Represents an L1Handler transaction in the StarkNet network.
pub struct L1Handler {
    #[getset(get = "pub")]
    hash_value: Felt252,
    #[getset(get = "pub")]
    contract_address: Address,
    entry_point_selector: Felt252,
    calldata: Vec<Felt252>,
    nonce: Option<Felt252>,
    paid_fee_on_l1: Option<Felt252>,
    skip_validate: bool,
    skip_execute: bool,
}

impl L1Handler {
    /// Constructor creates a new [L1Handler] instance.
    pub fn new(
        contract_address: Address,
        entry_point_selector: Felt252,
        calldata: Vec<Felt252>,
        nonce: Felt252,
        chain_id: Felt252,
        paid_fee_on_l1: Option<Felt252>,
    ) -> Result<L1Handler, TransactionError> {
        let hash_value = deprecated_calculate_transaction_hash_common(
            TransactionHashPrefix::L1Handler,
            L1_HANDLER_VERSION.into(),
            &contract_address,
            entry_point_selector,
            &calldata,
            0,
            chain_id,
            &[nonce],
        )?;

        L1Handler::new_with_tx_hash(
            contract_address,
            entry_point_selector,
            calldata,
            nonce,
            paid_fee_on_l1,
            hash_value,
        )
    }
    /// Creates a new [L1Handler] instance with a specified transaction hash.
    ///
    /// # Safety
    ///
    /// `tx_hash` will be assumed to be the same as would result from calling
    /// `deprecated_calculate_transaction_hash_common`. Non-compliance will result in silent misbehavior.
    pub fn new_with_tx_hash(
        contract_address: Address,
        entry_point_selector: Felt252,
        calldata: Vec<Felt252>,
        nonce: Felt252,
        paid_fee_on_l1: Option<Felt252>,
        tx_hash: Felt252,
    ) -> Result<L1Handler, TransactionError> {
        Ok(L1Handler {
            hash_value: tx_hash,
            contract_address,
            entry_point_selector,
            calldata,
            nonce: Some(nonce),
            paid_fee_on_l1,
            skip_execute: false,
            skip_validate: false,
        })
    }

    /// Applies self to 'state' by executing the L1-handler entry point.
    #[tracing::instrument(level = "debug", ret, err, skip(self, state, block_context, program_cache), fields(
        tx_type = ?TransactionType::L1Handler,
        self.hash_value = ?self.hash_value,
        self.contract_address = ?self.contract_address,
        self.entry_point_selector = ?self.entry_point_selector,
        self.nonce = ?self.nonce,
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
        let mut resources_manager = ExecutionResourcesManager::default();
        let entrypoint = ExecutionEntryPoint::new(
            self.contract_address.clone(),
            self.calldata.clone(),
            self.entry_point_selector,
            Address(0.into()),
            EntryPointType::L1Handler,
            None,
            None,
            remaining_gas,
        );

        let ExecutionResult {
            call_info,
            revert_error,
            n_reverted_steps,
        } = if self.skip_execute {
            ExecutionResult::default()
        } else {
            entrypoint.execute(
                state,
                block_context,
                &mut resources_manager,
                &mut self.get_execution_context(block_context.invoke_tx_max_n_steps)?,
                true,
                block_context.invoke_tx_max_n_steps,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )?
        };

        let changes = state.count_actual_state_changes(None)?;
        let actual_resources = calculate_tx_resources(
            resources_manager,
            &[call_info.clone()],
            TransactionType::L1Handler,
            changes,
            Some(self.get_payload_size()),
            n_reverted_steps,
        )?;

        // Enforce L1 fees.
        if block_context.enforce_l1_handler_fee {
            // Backward compatibility; Continue running the transaction even when
            // L1 handler fee is enforced, and paid_fee_on_l1 is None; If this is the case,
            // the transaction is an old transaction.
            if let Some(paid_fee) = self.paid_fee_on_l1 {
                let required_fee =
                    calculate_tx_fee(&actual_resources, block_context, &FeeType::Eth)?;
                // For now, assert only that any amount of fee was paid.
                if paid_fee.is_zero() {
                    return Err(TransactionError::FeeError(format!(
                        "Insufficient fee was paid. Expected: {required_fee};\n got: {paid_fee}."
                    )));
                };
            }
        }

        Ok(TransactionExecutionInfo::new_without_fee_info(
            None,
            call_info,
            revert_error,
            actual_resources,
            Some(TransactionType::L1Handler),
        ))
    }

    /// Returns the payload size of the corresponding L1-to-L2 message.
    pub fn get_payload_size(&self) -> usize {
        // The calldata includes the "from" field, which is not a part of the payload.
        // We thus subtract 1.
        self.calldata.len().saturating_sub(1)
    }

    /// Returns the execution context of the transaction.
    pub fn get_execution_context(
        &self,
        n_steps: u64,
    ) -> Result<TransactionExecutionContext, TransactionError> {
        Ok(TransactionExecutionContext::new(
            self.contract_address.clone(),
            self.hash_value,
            [].to_vec(),
            super::VersionSpecificAccountTxFields::new_deprecated(0),
            self.nonce.ok_or(TransactionError::MissingNonce)?,
            n_steps,
            L1_HANDLER_VERSION.into(),
        ))
    }

    /// Creates a L1Handler for simulation purposes.
    pub fn create_for_simulation(&self, skip_validate: bool, skip_execute: bool) -> Transaction {
        let tx = L1Handler {
            skip_validate,
            skip_execute,
            ..self.clone()
        };

        Transaction::L1Handler(tx)
    }

    /// Creates a `L1Handler` from a starknet api `L1HandlerTransaction`.
    pub fn from_sn_api_tx(
        tx: starknet_api::transaction::L1HandlerTransaction,
        tx_hash: Felt252,
        paid_fee_on_l1: Option<Felt252>,
    ) -> Result<Self, TransactionError> {
        L1Handler::new_with_tx_hash(
            Address(Felt252::from_bytes_be_slice(
                tx.contract_address.0.key().bytes(),
            )),
            Felt252::from_bytes_be_slice(tx.entry_point_selector.0.bytes()),
            tx.calldata
                .0
                .as_ref()
                .iter()
                .map(|f| Felt252::from_bytes_be_slice(f.bytes()))
                .collect(),
            Felt252::from_bytes_be_slice(tx.nonce.0.bytes()),
            paid_fee_on_l1,
            tx_hash,
        )
    }
}

#[cfg(test)]
mod test {
    use crate::{
        definitions::{
            block_context::{BlockContext, GasPrices},
            transaction_type::TransactionType,
        },
        execution::{CallInfo, TransactionExecutionInfo},
        services::api::contract_classes::{
            compiled_class::CompiledClass,
            deprecated_contract_class::{ContractClass, EntryPointType},
        },
        state::{
            cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
            in_memory_state_reader::InMemoryStateReader, state_api::State,
        },
        transaction::{l1_handler::L1Handler, Address, ClassHash},
    };
    use std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    use cairo_vm::{vm::runners::cairo_runner::ExecutionResources, Felt252};

    /// Test the correct execution of the L1Handler.
    #[test]
    fn test_execute_l1_handler() {
        let l1_handler = L1Handler::new(
            Address(0.into()),
            Felt252::from_hex("0xc73f681176fc7b3f9693986fd7b14581e8d540519e27400e88b8713932be01")
                .unwrap(),
            vec![
                Felt252::from_hex("0x8359E4B0152ed5A731162D3c7B0D8D56edB165A0").unwrap(),
                1.into(),
                10.into(),
            ],
            0.into(),
            0.into(),
            Some(10000.into()),
        )
        .unwrap();

        // Instantiate CachedState
        let mut state_reader = InMemoryStateReader::default();
        // Set contract_class
        let class_hash: ClassHash = ClassHash([1; 32]);
        let contract_class = ContractClass::from_path("starknet_programs/l1l2.json").unwrap();
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
        block_context.block_info.gas_price = GasPrices::new(1, 0);

        let tx_exec = l1_handler
            .execute(
                &mut state,
                &block_context,
                100000,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        let expected_tx_exec = expected_tx_exec_info();
        assert_eq!(tx_exec, expected_tx_exec)
    }

    /// Helper function to construct the expected transaction execution info.
    /// Expected output of the L1Handler's execution.
    fn expected_tx_exec_info() -> TransactionExecutionInfo {
        TransactionExecutionInfo {
            validate_info: None,
            call_info: Some(CallInfo {
                caller_address: Address(0.into()),
                call_type: Some(crate::execution::CallType::Call),
                contract_address: Address(0.into()),
                code_address: None,
                class_hash: Some(ClassHash([
                    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                    1, 1, 1, 1, 1, 1,
                ])),
                entry_point_selector: Some(Felt252::from_dec_str(
                    "352040181584456735608515580760888541466059565068553383579463728554843487745"
                ).unwrap()),
                entry_point_type: Some(EntryPointType::L1Handler),
                calldata: vec![
                    Felt252::from_dec_str("749882478819638189522059655282096373471980381600").unwrap(),
                    1.into(),
                    10.into(),
                ],
                retdata: vec![],
                execution_resources: Some(ExecutionResources {
                    n_steps: 141,
                    n_memory_holes: 20,
                    builtin_instance_counter: HashMap::from([
                        ("range_check_builtin".to_string(), 6),
                        ("pedersen_builtin".to_string(), 2),
                    ]),
                }),
                events: vec![],
                l2_to_l1_messages: vec![],
                storage_read_values: vec![0.into(), 0.into()],
                accessed_storage_keys: HashSet::from([ClassHash([
                    4, 40, 11, 247, 0, 35, 63, 18, 141, 159, 101, 81, 182, 2, 213, 216, 100, 110,
                    5, 5, 101, 122, 13, 252, 204, 72, 77, 8, 58, 226, 194, 24,
                ])]),
                internal_calls: vec![],
                gas_consumed: 0,
                failure_flag: false,
            }),
            revert_error: None,
            fee_transfer_info: None,
            actual_fee: 0,
            actual_resources: HashMap::from([
                ("n_steps".to_string(), 1494),
                ("pedersen_builtin".to_string(), 13),
                ("range_check_builtin".to_string(), 25),
                ("l1_gas_usage".to_string(), 17675),
            ]),
            tx_type: Some(TransactionType::L1Handler),
        }
    }
}
