use self::{
    cached_state::CachedState, contract_class_cache::ContractClassCache, state_api::StateReader,
    state_cache::StateCache,
};
use crate::{
    core::errors::state_errors::StateError,
    definitions::block_context::GasPrices,
    transaction::{error::TransactionError, Address, ClassHash, CompiledClassHash},
    utils::{get_keys, to_cache_state_storage_mapping, to_state_diff_storage_mapping},
};
use cairo_vm::{vm::runners::cairo_runner::ExecutionResources, Felt252};
use getset::Getters;
use std::{collections::HashMap, sync::Arc};

pub mod cached_state;
pub mod contract_class_cache;
pub(crate) mod contract_storage_state;
pub mod in_memory_state_reader;
pub mod state_api;
pub mod state_cache;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockInfo {
    /// The sequence number of the last block created.
    pub block_number: u64,
    /// Timestamp of the beginning of the last block creation attempt.
    pub block_timestamp: u64,
    /// L1 gas price measured at the beginning of the last block creation attempt.
    pub gas_price: GasPrices,
    /// The sequencer address of this block.
    pub sequencer_address: Address,
}

impl BlockInfo {
    /// Creates an empty BlockInfo with given sequencer address.
    pub const fn empty(sequencer_address: Address) -> Self {
        BlockInfo {
            block_number: 0, // To do: In cairo-lang, this value is set to -1
            block_timestamp: 0,
            gas_price: GasPrices {
                strk_l1_gas_price: 0,
                eth_l1_gas_price: 0,
            },
            sequencer_address,
        }
    }

    /// Validates that the progression from the current block to the next is legal.
    pub const fn validate_legal_progress(
        &self,
        next_block_info: &BlockInfo,
    ) -> Result<(), TransactionError> {
        if self.block_number + 1 != next_block_info.block_number {
            return Err(TransactionError::InvalidBlockNumber);
        }

        if self.block_timestamp >= next_block_info.block_timestamp {
            return Err(TransactionError::InvalidBlockTimestamp);
        }

        Ok(())
    }
}

/// Provides a default implementation for `BlockInfo`.
impl Default for BlockInfo {
    fn default() -> Self {
        Self {
            block_number: 0,
            block_timestamp: 0,
            gas_price: Default::default(),
            sequencer_address: Address(0.into()),
        }
    }
}

/// Manages execution resources and keeps track of syscall invocations.
#[derive(Clone, Debug, Default)]
pub struct ExecutionResourcesManager {
    /// Counter for each syscall invocation.
    pub(crate) syscall_counter: HashMap<String, u64>,
    /// Represents the resources used for Cairo execution.
    pub(crate) cairo_usage: ExecutionResources,
}

impl ExecutionResourcesManager {
    /// Initializes a new `ExecutionResourcesManager` with given syscalls and cairo usage.
    pub fn new(syscalls: Vec<String>, cairo_usage: ExecutionResources) -> Self {
        let mut syscall_counter = HashMap::new();
        for syscall in syscalls {
            syscall_counter.insert(syscall, 0);
        }
        ExecutionResourcesManager {
            syscall_counter,
            cairo_usage,
        }
    }

    /// Increments the syscall counter for a given syscall name by a specified amount.
    pub fn increment_syscall_counter(&mut self, syscall_name: &str, amount: u64) {
        *self
            .syscall_counter
            .entry(syscall_name.to_string())
            .or_default() += amount
    }

    /// Returns the current count for a given syscall name.
    pub fn get_syscall_counter(&self, syscall_name: &str) -> Option<u64> {
        self.syscall_counter
            .get(syscall_name)
            .map(ToOwned::to_owned)
    }
}

/// Represents a difference in state between two points in time.
#[derive(Default, Clone, PartialEq, Eq, Debug, Getters)]
#[getset(get = "pub")]
pub struct StateDiff {
    /// Mapping of address to class hash.
    pub(crate) address_to_class_hash: HashMap<Address, ClassHash>,
    /// Mapping of address to nonce value.
    pub(crate) address_to_nonce: HashMap<Address, Felt252>,
    /// Mapping of class hash to its compiled representation.
    pub(crate) class_hash_to_compiled_class: HashMap<ClassHash, CompiledClassHash>,
    /// Represents changes in storage values for different addresses.
    pub(crate) storage_updates: HashMap<Address, HashMap<Felt252, Felt252>>,
}

impl StateDiff {
    /// Constructs a new StateDiff.
    pub const fn new(
        address_to_class_hash: HashMap<Address, ClassHash>,
        address_to_nonce: HashMap<Address, Felt252>,
        class_hash_to_compiled_class: HashMap<ClassHash, CompiledClassHash>,
        storage_updates: HashMap<Address, HashMap<Felt252, Felt252>>,
    ) -> Self {
        StateDiff {
            address_to_class_hash,
            address_to_nonce,
            class_hash_to_compiled_class,
            storage_updates,
        }
    }

    /// Creates a `StateDiff` from a cached state.
    pub fn from_cached_state(state_cache: &StateCache) -> Result<Self, StateError> {
        let substracted_maps = &state_cache.storage_writes;
        let storage_updates = to_state_diff_storage_mapping(substracted_maps);

        let address_to_nonce = state_cache.nonce_writes.clone();
        let class_hash_to_compiled_class = state_cache.compiled_class_hash_writes.clone();
        let address_to_class_hash = state_cache.class_hash_writes.clone();

        Ok(StateDiff {
            address_to_class_hash,
            address_to_nonce,
            class_hash_to_compiled_class,
            storage_updates,
        })
    }

    /// Converts the current `StateDiff` to a `CachedState`.
    pub fn to_cached_state<T, C>(
        &self,
        state_reader: Arc<T>,
        contract_class_cache: Arc<C>,
    ) -> Result<CachedState<T, C>, StateError>
    where
        T: StateReader + Clone,
        C: ContractClassCache,
    {
        let mut cache_state = CachedState::new(state_reader, contract_class_cache);
        let cache_storage_mapping = to_cache_state_storage_mapping(&self.storage_updates);

        cache_state.cache_mut().set_initial_values(
            &self.address_to_class_hash,
            &self.class_hash_to_compiled_class,
            &self.address_to_nonce,
            &cache_storage_mapping,
        )?;
        Ok(cache_state)
    }

    /// Combines the current state diff with another to form a single cumulative diff.
    pub fn squash(&mut self, other: StateDiff) -> Self {
        self.address_to_class_hash
            .extend(other.address_to_class_hash);
        let address_to_class_hash = self.address_to_class_hash.clone();

        self.address_to_nonce.extend(other.address_to_nonce);
        let address_to_nonce = self.address_to_nonce.clone();

        self.class_hash_to_compiled_class
            .extend(other.class_hash_to_compiled_class);
        let class_hash_to_compiled_class = self.class_hash_to_compiled_class.clone();

        let mut storage_updates = HashMap::new();

        let addresses: Vec<&Address> = get_keys(&self.storage_updates, &other.storage_updates);

        for address in addresses {
            let default: HashMap<Felt252, Felt252> = HashMap::new();
            let mut map_a = self
                .storage_updates
                .get(address)
                .unwrap_or(&default)
                .to_owned();
            let map_b = other
                .storage_updates
                .get(address)
                .unwrap_or(&default)
                .to_owned();
            map_a.extend(map_b);
            storage_updates.insert(address.clone(), map_a.clone());
        }

        StateDiff {
            address_to_class_hash,
            address_to_nonce,
            class_hash_to_compiled_class,
            storage_updates,
        }
    }
}

/// Validates that block progression from a default block to the next one is legal.
#[test]
fn test_validate_legal_progress() {
    let first_block = BlockInfo::default();
    let next_block: BlockInfo = BlockInfo {
        block_number: 1,
        block_timestamp: 1,
        ..Default::default()
    };

    assert!(first_block.validate_legal_progress(&next_block).is_ok())
}

#[cfg(test)]
mod test {
    use super::StateDiff;
    use crate::{
        state::{
            cached_state::CachedState,
            contract_class_cache::PermanentContractClassCache,
            in_memory_state_reader::InMemoryStateReader,
            state_api::StateReader,
            state_cache::{StateCache, StorageEntry},
        },
        transaction::{Address, ClassHash},
    };
    use cairo_vm::Felt252;
    use std::{collections::HashMap, sync::Arc};

    /// Ensures that a StateDiff constructed from a CachedState without any updates has no storage updates.
    #[test]
    fn test_from_cached_state_without_updates() {
        let mut state_reader = InMemoryStateReader::default();

        let contract_address = Address(32123.into());
        let class_hash = ClassHash([9; 32]);
        let nonce = Felt252::from(42);

        state_reader
            .address_to_class_hash
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let cached_state = CachedState::new(
            Arc::new(state_reader),
            Arc::new(PermanentContractClassCache::default()),
        );

        let diff = StateDiff::from_cached_state(&cached_state.cache).unwrap();

        assert_eq!(0, diff.storage_updates.len());
    }

    /// Tests that a new ExecutionResourcesManager starts with zero syscall counters.
    #[test]
    fn execution_resources_manager_should_start_with_zero_syscall_counter() {
        let execution_resources_manager = super::ExecutionResourcesManager::new(
            vec!["syscall1".to_string(), "syscall2".to_string()],
            Default::default(),
        );

        assert_eq!(
            execution_resources_manager.get_syscall_counter("syscall1"),
            Some(0)
        );
        assert_eq!(
            execution_resources_manager.get_syscall_counter("syscall2"),
            Some(0)
        );
    }

    /// Ensures that incrementing a syscall counter of the `ExecutionResourcesManager` by one works as expected.
    #[test]
    fn execution_resources_manager_should_increment_one_to_the_syscall_counter() {
        let mut execution_resources_manager = super::ExecutionResourcesManager::new(
            vec!["syscall1".to_string(), "syscall2".to_string()],
            Default::default(),
        );

        execution_resources_manager.increment_syscall_counter("syscall1", 1);

        assert_eq!(
            execution_resources_manager.get_syscall_counter("syscall1"),
            Some(1)
        );
        assert_eq!(
            execution_resources_manager.get_syscall_counter("syscall2"),
            Some(0)
        );
    }

    /// Verifies that converting a `StateDiff` back to a `CachedState` results in an equivalent `CachedState` to the original.
    #[test]
    fn execution_resources_manager_should_add_syscall_if_not_present() {
        let mut execution_resources_manager = super::ExecutionResourcesManager::default();

        execution_resources_manager.increment_syscall_counter("syscall1", 1);

        assert_eq!(
            execution_resources_manager.get_syscall_counter("syscall1"),
            Some(1)
        );
    }

    /// Verifies that converting a `StateDiff` back to a `CachedState` results in an equivalent `CachedState` to the original.
    #[test]
    fn state_diff_to_cached_state_should_return_correct_cached_state() {
        let mut state_reader = InMemoryStateReader::default();

        let contract_address = Address(32123.into());
        let class_hash = ClassHash([9; 32]);
        let nonce = Felt252::from(42);

        state_reader
            .address_to_class_hash
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address.clone(), nonce);

        let cached_state_original = CachedState::new(
            Arc::new(state_reader.clone()),
            Arc::new(PermanentContractClassCache::default()),
        );

        let diff = StateDiff::from_cached_state(cached_state_original.cache()).unwrap();

        let cached_state = diff
            .to_cached_state::<_, PermanentContractClassCache>(
                Arc::new(state_reader),
                Arc::new(PermanentContractClassCache::default()),
            )
            .unwrap();

        assert_eq!(
            (&*cached_state_original.contract_class_cache().clone())
                .into_iter()
                .collect::<HashMap<_, _>>(),
            (&*cached_state.contract_class_cache().clone())
                .into_iter()
                .collect::<HashMap<_, _>>()
        );
        assert_eq!(
            cached_state_original
                .get_nonce_at(&contract_address)
                .unwrap(),
            cached_state.get_nonce_at(&contract_address).unwrap()
        );
    }

    /// Ensures that squashing a StateDiff with itself results in an equivalent StateDiff.
    #[test]
    fn state_diff_squash_with_itself_should_return_same_diff() {
        let mut state_reader = InMemoryStateReader::default();

        let contract_address = Address(32123.into());
        let class_hash = ClassHash([9; 32]);
        let nonce = Felt252::from(42);

        state_reader
            .address_to_class_hash
            .insert(contract_address.clone(), class_hash);
        state_reader
            .address_to_nonce
            .insert(contract_address, nonce);

        let entry: StorageEntry = (Address(555.into()), [0; 32]);
        let mut storage_writes = HashMap::new();
        storage_writes.insert(entry, Felt252::from(666));
        let cache = StateCache::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            storage_writes,
            HashMap::new(),
        );
        let cached_state = CachedState::new_for_testing(
            Arc::new(state_reader),
            cache,
            Arc::new(PermanentContractClassCache::default()),
        );

        let mut diff = StateDiff::from_cached_state(cached_state.cache()).unwrap();

        let diff_squashed = diff.squash(diff.clone());

        assert_eq!(diff, diff_squashed);
    }
}
