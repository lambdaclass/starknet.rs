#![deny(warnings)]

use cairo_vm::Felt252;
use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
use starknet_in_rust::transaction::ClassHash;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::deprecated_contract_class::ContractClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        ExecutionResourcesManager,
    },
    transaction::Address,
};
use std::{path::PathBuf, sync::Arc};

#[test]
fn delegate_call() {
    //* --------------------------------------------
    //*    Create state reader with class hash data
    //* --------------------------------------------

    let contract_class_cache = PermanentContractClassCache::default();
    let nonce = Felt252::ZERO;

    // Add get_number.cairo contract to the state

    let path = PathBuf::from("starknet_programs/get_number.json");
    let contract_class = ContractClass::from_path(path).unwrap();

    let address = Address(Felt252::ONE); // const CONTRACT_ADDRESS = 1;
    let class_hash = ClassHash([2; 32]);

    contract_class_cache.set_contract_class(
        class_hash,
        CompiledClass::Deprecated(Arc::new(contract_class)),
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader.address_to_nonce_mut().insert(address, nonce);

    // ---------------------------------------------------------
    //  Create program and entry point types for contract class
    // ---------------------------------------------------------

    let path = PathBuf::from("starknet_programs/delegate_call.json");
    let contract_class = ContractClass::from_path(path).unwrap();
    let entry_points_by_type = contract_class.entry_points_by_type().clone();

    // External entry point, delegate_call function delegate.cairo:L13
    let test_delegate_call_selector = *entry_points_by_type
        .get(&EntryPointType::External)
        .unwrap()
        .first()
        .unwrap()
        .selector();

    //  ------------ contract data --------------------

    let address = Address(1111.into());
    let class_hash = ClassHash([1; 32]);

    contract_class_cache.set_contract_class(
        class_hash,
        CompiledClass::Deprecated(Arc::new(contract_class)),
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

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
        address,
        calldata,
        test_delegate_call_selector,
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

    assert!(exec_entry_point
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
        .is_ok());
}
