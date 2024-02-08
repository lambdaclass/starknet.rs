use crate::{
    core::errors::state_errors::StateError,
    transaction::{Address, ClassHash, CompiledClassHash},
};
use cairo_vm::Felt252;
use getset::{Getters, MutGetters};
use std::collections::{HashMap, HashSet};

/// (contract_address, key)
// TODO: Change [u8; 32] to Felt252.
pub type StorageEntry = (Address, [u8; 32]);

/// Struct that keeps track of initial and written state of contracts
#[derive(Default, Clone, Debug, Eq, Getters, MutGetters, PartialEq)]
pub struct StateCache {
    // Reader's cached information; initial values, read before any write operation (per cell)
    #[get_mut = "pub"]
    pub(crate) class_hash_initial_values: HashMap<Address, ClassHash>,
    #[get_mut = "pub"]
    pub(crate) compiled_class_hash_initial_values: HashMap<ClassHash, CompiledClassHash>,
    #[getset(get = "pub", get_mut = "pub")]
    pub(crate) nonce_initial_values: HashMap<Address, Felt252>,
    #[getset(get = "pub", get_mut = "pub")]
    pub(crate) storage_initial_values: HashMap<StorageEntry, Felt252>,

    // Writer's cached information.
    #[get_mut = "pub"]
    pub(crate) class_hash_writes: HashMap<Address, ClassHash>,
    #[get_mut = "pub"]
    pub(crate) compiled_class_hash_writes: HashMap<ClassHash, CompiledClassHash>,
    #[get_mut = "pub"]
    pub(crate) nonce_writes: HashMap<Address, Felt252>,
    #[getset(get = "pub", get_mut = "pub")]
    pub(crate) storage_writes: HashMap<StorageEntry, Felt252>,
    #[get_mut = "pub"]
    pub(crate) class_hash_to_compiled_class_hash: HashMap<ClassHash, CompiledClassHash>,
}

impl StateCache {
    #[allow(clippy::too_many_arguments)]

    /// Create a new StateCache with given initial and written values for testing
    pub const fn new(
        class_hash_initial_values: HashMap<Address, ClassHash>,
        compiled_class_hash_initial_values: HashMap<ClassHash, CompiledClassHash>,
        nonce_initial_values: HashMap<Address, Felt252>,
        storage_initial_values: HashMap<StorageEntry, Felt252>,
        class_hash_writes: HashMap<Address, ClassHash>,
        compiled_class_hash_writes: HashMap<ClassHash, CompiledClassHash>,
        nonce_writes: HashMap<Address, Felt252>,
        storage_writes: HashMap<StorageEntry, Felt252>,
        class_hash_to_compiled_class_hash: HashMap<ClassHash, ClassHash>,
    ) -> Self {
        Self {
            class_hash_initial_values,
            compiled_class_hash_initial_values,
            nonce_initial_values,
            storage_initial_values,
            class_hash_writes,
            compiled_class_hash_writes,
            nonce_writes,
            storage_writes,
            class_hash_to_compiled_class_hash,
        }
    }

    /// Define a default state for testing
    pub(crate) fn default() -> Self {
        Self {
            class_hash_initial_values: HashMap::new(),
            compiled_class_hash_initial_values: HashMap::new(),
            nonce_initial_values: HashMap::new(),
            storage_initial_values: HashMap::new(),
            class_hash_writes: HashMap::new(),
            compiled_class_hash_writes: HashMap::new(),
            nonce_writes: HashMap::new(),
            storage_writes: HashMap::new(),
            class_hash_to_compiled_class_hash: HashMap::new(),
        }
    }

    /// Creates a new instance of `StateCache` for testing purposes with the provided initial values and writes.
    #[allow(clippy::too_many_arguments)]
    pub const fn new_for_testing(
        class_hash_initial_values: HashMap<Address, ClassHash>,
        compiled_class_hash_initial_values: HashMap<ClassHash, CompiledClassHash>,
        nonce_initial_values: HashMap<Address, Felt252>,
        storage_initial_values: HashMap<StorageEntry, Felt252>,
        class_hash_writes: HashMap<Address, ClassHash>,
        compiled_class_hash_writes: HashMap<ClassHash, CompiledClassHash>,
        nonce_writes: HashMap<Address, Felt252>,
        storage_writes: HashMap<StorageEntry, Felt252>,
        class_hash_to_compiled_class_hash: HashMap<ClassHash, CompiledClassHash>,
    ) -> Self {
        Self {
            class_hash_initial_values,
            compiled_class_hash_initial_values,
            nonce_initial_values,
            storage_initial_values,
            class_hash_writes,
            compiled_class_hash_writes,
            nonce_writes,
            storage_writes,
            class_hash_to_compiled_class_hash,
        }
    }

    /// Get the class hash for a given address
    pub(crate) fn get_class_hash(&self, contract_address: &Address) -> Option<&ClassHash> {
        if self.class_hash_writes.contains_key(contract_address) {
            return self.class_hash_writes.get(contract_address);
        }
        self.class_hash_initial_values.get(contract_address)
    }

    /// Get the compiled hash for a given class hash
    #[allow(dead_code)]
    pub(crate) fn get_compiled_class_hash(
        &self,
        class_hash: &ClassHash,
    ) -> Option<&CompiledClassHash> {
        if self.compiled_class_hash_writes.contains_key(class_hash) {
            return self.compiled_class_hash_writes.get(class_hash);
        }
        self.compiled_class_hash_initial_values.get(class_hash)
    }

    /// Get the nonce for a given address
    pub(crate) fn get_nonce(&self, contract_address: &Address) -> Option<&Felt252> {
        if self.nonce_writes.contains_key(contract_address) {
            return self.nonce_writes.get(contract_address);
        }
        self.nonce_initial_values.get(contract_address)
    }

    /// Get the storage for a given storage entry
    pub(crate) fn get_storage(&self, storage_entry: &StorageEntry) -> Option<&Felt252> {
        if self.storage_writes.contains_key(storage_entry) {
            return self.storage_writes.get(storage_entry);
        }
        self.storage_initial_values.get(storage_entry)
    }

    /// Update written values
    pub(crate) fn update_writes(
        &mut self,
        address_to_class_hash: &HashMap<Address, ClassHash>,
        class_hash_to_compiled_class_hash: &HashMap<ClassHash, CompiledClassHash>,
        address_to_nonce: &HashMap<Address, Felt252>,
        storage_updates: &HashMap<StorageEntry, Felt252>,
    ) {
        self.class_hash_writes.extend(address_to_class_hash.clone());
        self.compiled_class_hash_writes
            .extend(class_hash_to_compiled_class_hash.clone());
        self.nonce_writes.extend(address_to_nonce.clone());
        self.storage_writes.extend(storage_updates.clone());
    }

    /// Set initial values
    pub fn set_initial_values(
        &mut self,
        address_to_class_hash: &HashMap<Address, ClassHash>,
        class_hash_to_compiled_class: &HashMap<ClassHash, CompiledClassHash>,
        address_to_nonce: &HashMap<Address, Felt252>,
        storage_updates: &HashMap<StorageEntry, Felt252>,
    ) -> Result<(), StateError> {
        if !(self.class_hash_initial_values.is_empty()
            && self.class_hash_writes.is_empty()
            && self.nonce_initial_values.is_empty()
            && self.nonce_writes.is_empty()
            && self.storage_initial_values.is_empty()
            && self.storage_writes.is_empty())
        {
            return Err(StateError::StateCacheAlreadyInitialized);
        }
        self.update_writes(
            address_to_class_hash,
            class_hash_to_compiled_class,
            address_to_nonce,
            storage_updates,
        );
        Ok(())
    }

    // TODO: Remove warning inhibitor when finally used.
    /// Get all contract addresses that have been accessed
    #[allow(dead_code)]
    pub(crate) fn get_accessed_contract_addresses(&self) -> HashSet<Address> {
        let mut set: HashSet<Address> = HashSet::with_capacity(self.class_hash_writes.len());
        set.extend(self.class_hash_writes.keys().cloned());
        set.extend(self.nonce_writes.keys().cloned());
        set.extend(self.storage_writes.keys().map(|x| x.0.clone()));
        set
    }

    pub fn update_initial_values(&mut self) {
        for (k, v) in self.nonce_writes.iter() {
            self.nonce_initial_values.insert(k.clone(), *v);
        }

        for (k, v) in self.class_hash_writes.iter() {
            self.class_hash_initial_values.insert(k.clone(), *v);
        }

        for (k, v) in self.compiled_class_hash_writes.iter() {
            self.compiled_class_hash_initial_values.insert(*k, *v);
        }

        for (k, v) in self.storage_writes.iter() {
            self.storage_initial_values.insert(k.clone(), *v);
        }

        self.nonce_writes = HashMap::new();
        self.class_hash_writes = HashMap::new();
        self.compiled_class_hash_writes = HashMap::new();
        self.storage_writes = HashMap::new();
    }
}

/// Unit tests for StateCache
#[cfg(test)]
mod tests {

    use crate::{
        core::contract_address::compute_deprecated_class_hash,
        services::api::contract_classes::deprecated_contract_class::ContractClass,
    };

    use super::*;

    #[test]
    fn state_chache_set_initial_values() {
        let mut state_cache = StateCache::default();
        let address_to_class_hash = HashMap::from([(Address(10.into()), ClassHash([8; 32]))]);
        let contract_class =
            ContractClass::from_path("starknet_programs/raw_contract_classes/class_with_abi.json")
                .unwrap();
        let compiled_class_bytes = compute_deprecated_class_hash(&contract_class)
            .unwrap()
            .to_bytes_be();
        let class_hash_to_compiled_class_hash =
            HashMap::from([(ClassHash([8; 32]), ClassHash(compiled_class_bytes))]);
        let address_to_nonce = HashMap::from([(Address(9.into()), 12.into())]);
        let storage_updates = HashMap::from([((Address(4.into()), [1; 32]), 18.into())]);

        assert!(state_cache
            .set_initial_values(
                &address_to_class_hash,
                &class_hash_to_compiled_class_hash,
                &address_to_nonce,
                &storage_updates
            )
            .is_ok());

        assert_eq!(state_cache.class_hash_writes, address_to_class_hash);
        assert_eq!(
            state_cache.compiled_class_hash_writes,
            class_hash_to_compiled_class_hash
        );
        assert_eq!(state_cache.nonce_writes, address_to_nonce);
        assert_eq!(state_cache.storage_writes, storage_updates);

        assert_eq!(
            state_cache.get_accessed_contract_addresses(),
            HashSet::from([Address(10.into()), Address(9.into()), Address(4.into())])
        );

        state_cache.update_initial_values();

        assert_eq!(state_cache.class_hash_writes, HashMap::new());
        assert_eq!(state_cache.compiled_class_hash_writes, HashMap::new());
        assert_eq!(state_cache.nonce_writes, HashMap::new());
        assert_eq!(state_cache.storage_writes, HashMap::new());

        assert_eq!(state_cache.class_hash_initial_values, address_to_class_hash);
        assert_eq!(
            state_cache.compiled_class_hash_initial_values,
            class_hash_to_compiled_class_hash
        );
        assert_eq!(state_cache.nonce_initial_values, address_to_nonce);
        assert_eq!(state_cache.storage_initial_values, storage_updates);
    }
}
