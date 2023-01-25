use std::collections::HashMap;

use felt::Felt;
use num_traits::Zero;

use crate::{
    core::errors::state_errors::StateError, services::api::contract_class::ContractClass,
    utils::Address,
};

use super::{
    state_api::{State, StateReader},
    state_api_objects::BlockInfo,
    state_cache::{StateCache, StorageEntry},
};

pub(crate) type ContractClassCache = HashMap<[u8; 32], ContractClass>;

pub(crate) const UNINITIALIZED_CLASS_HASH: &[u8; 32] = b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";

#[derive(Debug, Clone, Default)]
pub(crate) struct CachedState<T: StateReader + Clone> {
    pub(crate) block_info: BlockInfo,
    pub(crate) state_reader: T,
    pub(crate) cache: StateCache,
    pub(crate) contract_classes: Option<ContractClassCache>,
}

impl<T: StateReader + Clone> CachedState<T> {
    pub(crate) fn new(
        block_info: BlockInfo,
        state_reader: T,
        contract_class_cache: Option<ContractClassCache>,
    ) -> Self {
        Self {
            block_info,
            cache: StateCache::default(),
            contract_classes: contract_class_cache,
            state_reader,
        }
    }

    pub(crate) fn block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    pub(crate) fn update_block_info(&mut self, block_info: BlockInfo) {
        self.block_info = block_info;
    }

    pub(crate) fn set_contract_classes(
        &mut self,
        contract_classes: ContractClassCache,
    ) -> Result<(), StateError> {
        if self.contract_classes.is_some() {
            return Err(StateError::AssignedContractClassCache);
        }
        self.contract_classes = Some(contract_classes);
        Ok(())
    }

    pub(crate) fn get_contract_classes(&self) -> Result<&ContractClassCache, StateError> {
        self.contract_classes
            .as_ref()
            .ok_or(StateError::MissingContractClassCache)
    }

    ///Apply updates to parent state
    pub(crate) fn apply(&mut self, parent: &mut CachedState<T>) {
        // TODO assert: if self.state_reader == parent
        parent.block_info = self.block_info.clone();
        parent.cache.update_writes_from_other(&self.cache);
    }
}

impl<T: StateReader + Clone> StateReader for CachedState<T> {
    fn get_contract_class(&mut self, class_hash: &[u8; 32]) -> Result<ContractClass, StateError> {
        if !(self.get_contract_classes()?.contains_key(class_hash)) {
            let contract_class = self.state_reader.get_contract_class(class_hash)?;
            self.set_contract_class(class_hash, &contract_class)?;
        }
        Ok(self
            .get_contract_classes()?
            .get(class_hash)
            .ok_or(StateError::MissingContractClassCache)?
            .to_owned())
    }

    fn get_class_hash_at(&mut self, contract_address: &Address) -> Result<&[u8; 32], StateError> {
        if self.cache.get_class_hash(contract_address).is_none() {
            let class_hash = self.state_reader.get_class_hash_at(contract_address)?;
            self.cache
                .class_hash_initial_values
                .insert(contract_address.clone(), *class_hash);
        }

        self.cache
            .get_class_hash(contract_address)
            .ok_or_else(|| StateError::NoneClassHash(contract_address.clone()))
    }

    fn get_nonce_at(&mut self, contract_address: &Address) -> Result<&Felt, StateError> {
        if self.cache.get_nonce(contract_address).is_none() {
            let nonce = self.state_reader.get_nonce_at(contract_address)?;
            self.cache
                .nonce_initial_values
                .insert(contract_address.clone(), nonce.clone());
        }
        self.cache
            .get_nonce(contract_address)
            .ok_or_else(|| StateError::NoneNonce(contract_address.clone()))
    }

    fn get_storage_at(&mut self, storage_entry: &StorageEntry) -> Result<&Felt, StateError> {
        if self.cache.get_storage(storage_entry).is_none() {
            let value = self.state_reader.get_storage_at(storage_entry)?;
            self.cache
                .storage_initial_values
                .insert(storage_entry.clone(), value.clone());
        }

        self.cache
            .get_storage(storage_entry)
            .ok_or_else(|| StateError::NoneStorage(storage_entry.clone()))
    }
}

impl<T: StateReader + Clone> State for CachedState<T> {
    fn block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    fn set_contract_class(
        &mut self,
        class_hash: &[u8; 32],
        contract_class: &ContractClass,
    ) -> Result<(), StateError> {
        self.contract_classes
            .as_mut()
            .ok_or(StateError::MissingContractClassCache)?
            .insert(*class_hash, contract_class.clone());

        Ok(())
    }

    fn deploy_contract(
        &mut self,
        contract_address: Address,
        class_hash: [u8; 32],
    ) -> Result<(), StateError> {
        if contract_address == Address(0.into()) {
            return Err(StateError::ContractAddressOutOfRangeAddress(
                contract_address.clone(),
            ));
        }

        let current_class_hash = self.get_class_hash_at(&contract_address)?;

        if current_class_hash == UNINITIALIZED_CLASS_HASH {
            return Err(StateError::ContractAddressUnavailable(
                contract_address.clone(),
            ));
        }
        self.cache
            .class_hash_writes
            .insert(contract_address, class_hash);

        Ok(())
    }

    fn increment_nonce(&mut self, contract_address: &Address) -> Result<(), StateError> {
        let new_nonce = self.get_nonce_at(contract_address)? + 1;
        self.cache
            .nonce_writes
            .insert(contract_address.clone(), new_nonce);
        Ok(())
    }

    fn update_block_info(&mut self, block_info: BlockInfo) {
        self.block_info = block_info;
    }

    fn set_storage_at(&mut self, storage_entry: &StorageEntry, value: Felt) {
        self.cache
            .storage_writes
            .insert(storage_entry.clone(), value);
    }
}

#[cfg(test)]
mod tests {
    use cairo_rs::types::program::Program;
    use felt::NewFelt;

    use crate::{
        business_logic::fact_state::{
            contract_state::ContractState, in_memory_state_reader::InMemoryStateReader,
        },
        services::api::contract_class::{ContractEntryPoint, EntryPointType},
        starknet_storage::{dict_storage::DictStorage, storage::Storage},
    };

    use super::*;

    #[test]
    fn get_class_hash_and_nonce_from_state_reader() {
        let mut state_reader =
            InMemoryStateReader::new(HashMap::new(), DictStorage::new(), DictStorage::new());

        let contract_address = Address(32123.into());
        let contract_state = ContractState::create([8; 32], Felt::new(109), HashMap::new());

        state_reader
            .global_state_root
            .insert(contract_address.clone(), [0; 32]);
        state_reader
            .ffc
            .set_contract_state(&[0; 32], &contract_state);

        let mut cached_state = CachedState::new(BlockInfo::default(), state_reader, None);

        assert_eq!(
            cached_state.get_class_hash_at(&contract_address),
            Ok(&contract_state.contract_hash)
        );
        assert_eq!(
            cached_state.get_nonce_at(&contract_address),
            Ok(&contract_state.nonce)
        );
        cached_state.increment_nonce(&contract_address);
        assert_eq!(
            cached_state.get_nonce_at(&contract_address),
            Ok(&(contract_state.nonce + Felt::new(1)))
        );
    }

    #[test]
    fn get_contract_class_from_state_reader() {
        let mut state_reader =
            InMemoryStateReader::new(HashMap::new(), DictStorage::new(), DictStorage::new());

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
            .set_contract_class(&[0; 32], &contract_class);

        let mut cached_state = CachedState::new(BlockInfo::default(), state_reader, None);

        cached_state.set_contract_classes(HashMap::new());
        assert!(cached_state.contract_classes.is_some());

        assert_eq!(
            cached_state.get_contract_class(&[0; 32]),
            cached_state.state_reader.get_contract_class(&[0; 32])
        );
    }

    #[test]
    fn apply_cached_state_test() {
        let mut block_info = BlockInfo::default();
        block_info.block_number += 11;
        let mut cached_state = CachedState::new(
            block_info.clone(),
            InMemoryStateReader::new(HashMap::new(), DictStorage::new(), DictStorage::new()),
            None,
        );
        let mut parent_cached_state = CachedState::new(
            BlockInfo::default(),
            InMemoryStateReader::new(HashMap::new(), DictStorage::new(), DictStorage::new()),
            None,
        );

        cached_state.apply(&mut parent_cached_state);
        assert_eq!(parent_cached_state.block_info(), &block_info)
    }

    #[test]
    fn cached_state_storage_test() {
        let mut cached_state = CachedState::new(
            BlockInfo::default(),
            InMemoryStateReader::new(HashMap::new(), DictStorage::new(), DictStorage::new()),
            None,
        );

        let storage_entry: StorageEntry = (Address(31.into()), [0; 32]);
        let value = Felt::new(10);
        cached_state.set_storage_at(&storage_entry, value.clone());

        assert_eq!(cached_state.get_storage_at(&storage_entry), Ok(&value));

        let storage_entry_2: StorageEntry = (Address(150.into()), [1; 32]);
        assert!(cached_state.get_storage_at(&storage_entry_2).is_err());
    }

    #[test]
    fn cached_state_deploy_contract_test() {
        let mut state_reader =
            InMemoryStateReader::new(HashMap::new(), DictStorage::new(), DictStorage::new());

        let contract_address = Address(32123.into());
        let contract_state = ContractState::create([8; 32], Felt::new(109), HashMap::new());

        state_reader
            .global_state_root
            .insert(contract_address.clone(), [0; 32]);
        state_reader
            .ffc
            .set_contract_state(&[0; 32], &contract_state);

        let mut cached_state = CachedState::new(BlockInfo::default(), state_reader, None);

        assert!(cached_state
            .deploy_contract(contract_address, [10; 32])
            .is_ok())
    }
}
