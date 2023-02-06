use std::{collections::HashMap, hash::Hash};

use felt::Felt;

use crate::{
    business_logic::{
        execution::objects::{Event, TransactionExecutionInfo},
        fact_state::in_memory_state_reader::InMemoryStateReader,
        state::{cached_state::CachedState, state_api::State, state_api_objects::BlockInfo},
        transaction::{internal_objects::InternalDeploy, state_objects::InternalStateTransaction},
    },
    definitions::{
        constants::TRANSACTION_VERSION,
        general_config::{self, StarknetGeneralConfig},
    },
    services::api::{
        contract_class::ContractClass,
        messages::{self, StarknetMessageToL1},
    },
    starknet_storage::dict_storage::DictStorage,
    utils::Address,
};

use super::starknet_state_error::StarknetStateError;

/// StarkNet testing object. Represents a state of a StarkNet network.
pub(crate) struct StarknetState {
    pub(crate) state: CachedState<InMemoryStateReader>,
    pub(crate) general_config: StarknetGeneralConfig,
    l2_to_l1_messages: HashMap<Vec<u8>, usize>,
    l2_to_l1_messages_log: Vec<StarknetMessageToL1>,
    events: Vec<Event>,
}

impl StarknetState {
    pub fn empty(config: Option<StarknetGeneralConfig>) -> Self {
        let general_config = config.unwrap_or_default();
        let state_reader = InMemoryStateReader::new(DictStorage::new(), DictStorage::new());

        let block_info = BlockInfo::empty(general_config.sequencer_address.clone());
        let state = CachedState::new(block_info, state_reader, Some(HashMap::new()));

        let l2_to_l1_messages = HashMap::new();
        let l2_to_l1_messages_log = Vec::new();

        let events = Vec::new();
        StarknetState {
            state,
            general_config,
            l2_to_l1_messages,
            l2_to_l1_messages_log,
            events,
        }
    }

    /// Declares a contract class.
    /// Returns the class hash and the execution info.
    /// Args:
    /// contract_class - a compiled StarkNet contract returned by compile_starknet_files()
    pub fn declare(&self, contract_class: ContractClass) -> (Vec<u8>, TransactionExecutionInfo) {
        todo!()
    }

    /// Deploys a contract. Returns the contract address and the execution info.
    /// Args:
    /// contract_class - a compiled StarkNet contract returned by compile_starknet_files().
    /// contract_address_salt - If supplied, a hexadecimal string or an integer representing
    /// the salt to use for deploying. Otherwise, the salt is randomized.

    // TODO: ask for contract_address_salt
    pub fn deploy(
        &mut self,
        contract_class: ContractClass,
        constructor_calldata: Vec<Felt>,
        contract_address_salt: Address,
    ) -> Result<(Address, TransactionExecutionInfo), StarknetStateError> {
        let chain_id = self.general_config.starknet_os_config.chain_id.as_u64()?;
        let mut tx = InternalDeploy::new(
            contract_address_salt,
            contract_class.clone(),
            constructor_calldata,
            chain_id,
            TRANSACTION_VERSION,
        )?;

        self.state
            .set_contract_class(&tx.contract_hash, &contract_class)?;

        let tx_execution_info = self.execute_tx(&mut tx);
        Ok((tx.contract_address, tx_execution_info))
    }

    pub fn execute_tx(&mut self, tx: &mut InternalDeploy) -> TransactionExecutionInfo {
        let state_copy = self.state.copy_and_apply();
        let tx_execution_info = tx.apply_state_updates(state_copy, self.general_config.clone());
        self.add_messages_and_events(&tx_execution_info);
        tx_execution_info
    }

    pub fn add_messages_and_events(
        &mut self,
        exec_info: &TransactionExecutionInfo,
    ) -> Result<(), StarknetStateError> {
        for msg in exec_info.get_sorted_l2_to_l1_messages()? {
            let starknet_message =
                StarknetMessageToL1::new(msg.from_address, msg.to_address, msg.payload);

            self.l2_to_l1_messages_log.push(starknet_message.clone());
            let message_hash = starknet_message.get_hash();

            if self.l2_to_l1_messages.contains_key(&message_hash) {
                let val = self.l2_to_l1_messages.get(&message_hash).unwrap();
                self.l2_to_l1_messages.insert(message_hash, val + 1);
            } else {
                self.l2_to_l1_messages.insert(message_hash, 1);
            }
        }

        let mut events = exec_info.get_sorted_events()?;
        self.events.append(&mut events);
        Ok(())
    }
}
