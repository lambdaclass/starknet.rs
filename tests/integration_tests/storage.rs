use cairo_vm::vm::runners::cairo_runner::ExecutionResources;
use cairo_vm::Felt252;

use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
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
        ExecutionResourcesManager,
    },
    transaction::Address,
    utils::calculate_sn_keccak,
};
use starknet_in_rust::{transaction::ClassHash, EntryPointType};
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[test]
fn integration_storage_test() {
    // ---------------------------------------------------------
    //  Create program and entry point types for contract class
    // ---------------------------------------------------------

    let path = PathBuf::from("starknet_programs/storage.json");
    let contract_class = ContractClass::from_path(path).unwrap();
    let entry_points_by_type = contract_class.entry_points_by_type().clone();

    let storage_entrypoint_selector = *entry_points_by_type
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
    let class_hash = ClassHash([1; 32]);
    let nonce = Felt252::from(88);
    let storage_entry = (address.clone(), [90; 32]);
    let storage_value = Felt252::from(10902);

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
        .insert(storage_entry, storage_value);

    //* ---------------------------------------
    //*    Create state with previous data
    //* ---------------------------------------

    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    //* ------------------------------------
    //*    Create execution entry point
    //* ------------------------------------

    let calldata = [].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata.clone(),
        storage_entrypoint_selector,
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

    let expected_key_bytes = calculate_sn_keccak("_counter".as_bytes());
    let expected_key = ClassHash(expected_key_bytes);
    let mut expected_accessed_storage_keys = HashSet::new();
    expected_accessed_storage_keys.insert(expected_key);

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: Address(1111.into()),
        entry_point_selector: Some(storage_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata,
        retdata: [42.into()].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 68,
            ..Default::default()
        }),
        class_hash: Some(class_hash),
        storage_read_values: vec![0.into(), 42.into()],
        accessed_storage_keys: expected_accessed_storage_keys,
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

    assert!(!state.cache().storage_writes().is_empty());
    assert_eq!(
        state
            .cache()
            .storage_writes()
            .get(&(address, expected_key.0))
            .cloned(),
        Some(Felt252::from(42))
    );
}
