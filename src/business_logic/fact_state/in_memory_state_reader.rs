use super::contract_state::ContractState;
use crate::{
    business_logic::state::{state_api::StateReader, state_cache::StorageEntry},
    core::errors::state_errors::StateError,
    services::api::contract_class::ContractClass,
    starknet_storage::{dict_storage::DictStorage, storage::Storage},
    utils::Address,
};
use felt::Felt;
use getset::MutGetters;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, MutGetters)]
pub struct InMemoryStateReader {
    pub(crate) ffc: DictStorage,
    #[getset(get_mut = "pub")]
    pub(crate) contract_states: HashMap<Address, ContractState>,
    pub(crate) contract_class_storage: DictStorage,
}

impl InMemoryStateReader {
    pub fn new(ffc: DictStorage, contract_class_storage: DictStorage) -> Self {
        Self {
            ffc,
            contract_states: HashMap::new(),
            contract_class_storage,
        }
    }

    fn get_contract_state(
        &mut self,
        contract_address: &Address,
    ) -> Result<&ContractState, StateError> {
        if !self.contract_states.contains_key(contract_address) {
            let result = self
                .ffc
                .get_contract_state(&contract_address.to_32_bytes()?)?;
            self.contract_states
                .insert(contract_address.clone(), result);
        }

        self.contract_states
            .get(contract_address)
            .ok_or_else(|| StateError::NoneContractState(contract_address.clone()))
    }
}

impl StateReader for InMemoryStateReader {
    fn get_contract_class(&mut self, class_hash: &[u8; 32]) -> Result<ContractClass, StateError> {
        let contract_class = self.contract_class_storage.get_contract_class(class_hash)?;
        contract_class.validate()?;
        Ok(contract_class)
    }
    fn get_class_hash_at(&mut self, contract_address: &Address) -> Result<&[u8; 32], StateError> {
        Ok(&self.get_contract_state(contract_address)?.contract_hash)
    }

    fn get_nonce_at(&mut self, contract_address: &Address) -> Result<&Felt, StateError> {
        Ok(&self.get_contract_state(contract_address)?.nonce)
    }

    fn get_storage_at(&mut self, storage_entry: &StorageEntry) -> Result<&Felt, StateError> {
        let contract_state = self.get_contract_state(&storage_entry.0)?;
        contract_state
            .storage_keys
            .get(&storage_entry.1)
            .ok_or(StateError::NoneStoragLeaf(storage_entry.1))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        services::api::contract_class::{ContractEntryPoint, EntryPointType},
        starknet_storage::dict_storage::DictStorage,
    };
    use cairo_rs::types::program::Program;

    #[test]
    fn get_contract_state_test() {
        let mut state_reader = InMemoryStateReader::new(DictStorage::new(), DictStorage::new());

        let contract_address = Address(32123.into());
        let contract_state = ContractState::new([1; 32], Felt::new(109), HashMap::new());

        state_reader
            .ffc
            .set_contract_state(&contract_address.to_32_bytes().unwrap(), &contract_state)
            .unwrap();

        assert_eq!(
            state_reader.get_contract_state(&contract_address),
            Ok(&contract_state)
        );
        assert_eq!(
            state_reader.get_class_hash_at(&contract_address),
            Ok(&contract_state.contract_hash)
        );
        assert_eq!(
            state_reader.get_nonce_at(&contract_address),
            Ok(&contract_state.nonce)
        );
        assert_eq!(
            state_reader.contract_states,
            HashMap::from([(contract_address, contract_state)])
        );
    }

    #[test]
    fn get_contract_class_test() {
        let mut state_reader = InMemoryStateReader::new(DictStorage::new(), DictStorage::new());

        let contract_class_key = [0; 32];
        let contract_class = ContractClass::new(
            Program::default(),
            HashMap::from([(
                EntryPointType::Constructor,
                vec![ContractEntryPoint::default()],
            )]),
            None,
        )
        .expect("Error creating contract class");

        state_reader
            .contract_class_storage
            .set_contract_class(&[0; 32], &contract_class)
            .unwrap();

        assert_eq!(
            state_reader.get_contract_class(&contract_class_key),
            Ok(contract_class)
        );
    }
}
