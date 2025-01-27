#![deny(warnings)]

use cairo_vm::{vm::runners::cairo_runner::ExecutionResources, Felt252};
use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
use starknet_in_rust::transaction::ClassHash;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::deprecated_contract_class::ContractClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        state_cache::StorageEntry,
        ExecutionResourcesManager,
    },
    transaction::Address,
    utils::calculate_sn_keccak,
};
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[test]
fn hello_starknet_increase_balance() {
    // ---------------------------------------------------------
    //  Create program and entry point types for contract class
    // ---------------------------------------------------------

    let path = PathBuf::from("starknet_programs/increase_balance.json");
    let contract_class = ContractClass::from_path(path).unwrap();
    let entry_points_by_type = contract_class.entry_points_by_type().clone();

    // External entry point, increase_balance function increase_balance.cairo:L13
    let increase_balance_selector = *entry_points_by_type
        .get(&EntryPointType::External)
        .unwrap()
        .first()
        .unwrap()
        .selector();

    //* --------------------------------------------
    //*    Create state reader with class hash data
    //* --------------------------------------------

    let contract_class_cache = PermanentContractClassCache::default();

    //  ------------ contract data --------------------

    let address = Address(1111.into());
    let class_hash: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;
    let storage_entry: StorageEntry = (address.clone(), [1; 32]);
    let storage = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash,
        CompiledClass::Deprecated(Arc::new(contract_class)),
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);
    state_reader
        .address_to_storage_mut()
        .insert(storage_entry, storage);

    //* ---------------------------------------
    //*    Create state with previous data
    //* ---------------------------------------

    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    //* ------------------------------------
    //*    Create execution entry point
    //* ------------------------------------

    let calldata = [1.into()].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata.clone(),
        increase_balance_selector,
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        0,
    );

    //* --------------------
    //*   Execute contract
    //* ---------------------
    let block_context = BlockContext::default();
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::ZERO,
        Vec::new(),
        Default::default(),
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        *TRANSACTION_VERSION,
    );
    let mut resources_manager = ExecutionResourcesManager::default();
    let expected_key_bytes = calculate_sn_keccak("balance".as_bytes());
    let expected_key: ClassHash = ClassHash(expected_key_bytes);
    let mut expected_accessed_storage_keys = HashSet::new();
    expected_accessed_storage_keys.insert(expected_key);
    let expected_storage_read_values = vec![Felt252::ZERO, Felt252::ZERO];

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: Address(1111.into()),
        entry_point_selector: Some(increase_balance_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata,
        retdata: [].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 65,
            ..Default::default()
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys: expected_accessed_storage_keys,
        storage_read_values: expected_storage_read_values,
        ..Default::default()
    };

    assert_eq!(
        exec_entry_point
            .execute(
                &mut state,
                &block_context,
                &mut resources_manager,
                &mut tx_execution_context,
                false,
                block_context.invoke_tx_max_n_steps(),
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap()
            .call_info
            .unwrap(),
        expected_call_info
    );
}
