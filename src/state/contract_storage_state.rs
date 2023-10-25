use super::{
    cached_state::CachedState,
    contract_class_cache::ContractClassCache,
    state_api::{State, StateReader},
};
use crate::{
    core::errors::state_errors::StateError,
    utils::{Address, ClassHash},
};
use cairo_vm::felt::Felt252;
use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct ContractStorageState<'a, S: StateReader, C: ContractClassCache> {
    pub(crate) state: &'a mut CachedState<S, C>,
    pub(crate) contract_address: Address,
    /// Maintain all read request values in chronological order
    pub(crate) read_values: Vec<Felt252>,
    pub(crate) accessed_keys: HashSet<ClassHash>,
}

impl<'a, S: StateReader, C: ContractClassCache> ContractStorageState<'a, S, C> {
    pub(crate) fn new(state: &'a mut CachedState<S, C>, contract_address: Address) -> Self {
        Self {
            state,
            contract_address,
            read_values: Vec::new(),
            accessed_keys: HashSet::new(),
        }
    }

    pub(crate) fn read(&mut self, address: &ClassHash) -> Result<Felt252, StateError> {
        self.accessed_keys.insert(*address);
        let value = self
            .state
            .get_storage_at(&(self.contract_address.clone(), *address))?;

        self.read_values.push(value.clone());
        Ok(value)
    }

    pub(crate) fn write(&mut self, address: &ClassHash, value: Felt252) {
        self.accessed_keys.insert(*address);
        self.state
            .set_storage_at(&(self.contract_address.clone(), *address), value);
    }
}
