#![cfg(all(feature = "cairo-native", not(feature = "cairo_1_tests")))]

use crate::CallType::Call;
use cairo_lang_starknet::casm_contract_class::CasmContractEntryPoints;
use cairo_lang_starknet::contract_class::ContractClass;
use cairo_lang_starknet::contract_class::ContractEntryPoints;
use cairo_vm::felt::Felt252;
use num_bigint::BigUint;
use num_traits::{Num, One, Zero};
use pretty_assertions_sorted::{assert_eq, assert_eq_sorted};
#[cfg(feature = "cairo-native")]
use starknet_api::hash::StarkHash;
use starknet_in_rust::definitions::block_context::BlockContext;
use starknet_in_rust::execution::{Event, OrderedEvent};
use starknet_in_rust::hash_utils::calculate_contract_address;
use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
use starknet_in_rust::state::state_api::State;
use starknet_in_rust::CasmContractClass;
use starknet_in_rust::EntryPointType::{self, External};
use starknet_in_rust::{
    definitions::constants::TRANSACTION_VERSION,
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, TransactionExecutionContext,
    },
    state::cached_state::CachedState,
    state::{in_memory_state_reader::InMemoryStateReader, ExecutionResourcesManager},
    utils::{Address, ClassHash},
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

fn insert_sierra_class_into_cache(
    contract_class_cache: &mut HashMap<ClassHash, CompiledClass>,
    class_hash: ClassHash,
    sierra_class: ContractClass,
) {
    let sierra_program = sierra_class.extract_sierra_program().unwrap();
    let entry_points = sierra_class.entry_points_by_type;
    contract_class_cache.insert(
        class_hash,
        CompiledClass::Sierra(Arc::new((sierra_program, entry_points))),
    );
}

#[test]
#[cfg(feature = "cairo-native")]
fn get_block_hash_test() {
    use starknet_in_rust::utils::felt_to_hash;

    let sierra_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/get_block_hash_basic.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    let casm_data = include_bytes!("../starknet_programs/cairo2/get_block_hash_basic.casm");
    let casm_contract_class: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

    let native_entrypoints = sierra_contract_class.clone().entry_points_by_type;
    let native_external_selector = &native_entrypoints.external.get(0).unwrap().selector;

    let casm_entrypoints = casm_contract_class.clone().entry_points_by_type;
    let casm_external_selector = &casm_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    let native_class_hash: ClassHash = [1; 32];
    let casm_class_hash: ClassHash = [2; 32];
    let caller_address = Address(1.into());

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        native_class_hash,
        sierra_contract_class,
    );

    contract_class_cache.insert(
        casm_class_hash,
        CompiledClass::Casm(Arc::new(casm_contract_class)),
    );

    let mut state_reader = InMemoryStateReader::default();
    let nonce = Felt252::zero();

    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), casm_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let state_reader = Arc::new(state_reader);
    let mut state_vm = CachedState::new(state_reader.clone(), contract_class_cache.clone());

    state_vm.cache_mut().storage_initial_values_mut().insert(
        (Address(1.into()), felt_to_hash(&Felt252::from(10))),
        Felt252::from_bytes_be(StarkHash::new([5; 32]).unwrap().bytes()),
    );
    let mut state_native = CachedState::new(state_reader, contract_class_cache);
    state_native
        .cache_mut()
        .storage_initial_values_mut()
        .insert(
            (Address(1.into()), felt_to_hash(&Felt252::from(10))),
            Felt252::from_bytes_be(StarkHash::new([5; 32]).unwrap().bytes()),
        );

    // block number
    let calldata = [10.into()].to_vec();

    let native_result = execute(
        &mut state_native,
        &caller_address,
        &caller_address,
        native_external_selector,
        &calldata,
        EntryPointType::External,
        &native_class_hash,
    );

    let vm_result = execute(
        &mut state_vm,
        &caller_address,
        &caller_address,
        casm_external_selector,
        &calldata,
        EntryPointType::External,
        &casm_class_hash,
    );

    assert_eq!(vm_result.caller_address, caller_address);
    assert_eq!(vm_result.call_type, Some(CallType::Delegate));
    assert_eq!(vm_result.contract_address, caller_address);
    assert_eq!(
        vm_result.entry_point_selector,
        Some(Felt252::new(casm_external_selector))
    );
    assert_eq!(vm_result.entry_point_type, Some(EntryPointType::External));
    assert_eq!(vm_result.calldata, calldata);
    assert!(!vm_result.failure_flag);
    assert_eq!(
        vm_result.retdata,
        [Felt252::from_bytes_be(
            StarkHash::new([5; 32]).unwrap().bytes()
        )]
        .to_vec()
    );
    assert_eq!(vm_result.class_hash, Some(casm_class_hash));

    assert_eq!(native_result.caller_address, caller_address);
    assert_eq!(native_result.call_type, Some(CallType::Delegate));
    assert_eq!(native_result.contract_address, caller_address);
    assert_eq!(
        native_result.entry_point_selector,
        Some(Felt252::new(native_external_selector))
    );
    assert_eq!(
        native_result.entry_point_type,
        Some(EntryPointType::External)
    );
    assert_eq!(native_result.calldata, calldata);
    assert!(!native_result.failure_flag);
    assert_eq!(
        native_result.retdata,
        [Felt252::from_bytes_be(
            StarkHash::new([5; 32]).unwrap().bytes()
        )]
        .to_vec()
    );
    assert_eq!(native_result.execution_resources, None);
    assert_eq!(native_result.class_hash, Some(native_class_hash));
    assert_eq!(native_result.gas_consumed, vm_result.gas_consumed);

    assert_eq!(vm_result.events, native_result.events);
    assert_eq!(
        vm_result.accessed_storage_keys,
        native_result.accessed_storage_keys
    );
    assert_eq!(vm_result.l2_to_l1_messages, native_result.l2_to_l1_messages);
}

#[test]
fn integration_test_erc20() {
    let sierra_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/erc20.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    let casm_data = include_bytes!("../starknet_programs/cairo2/erc20.casm");
    let casm_contract_class: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

    let native_entrypoints = sierra_contract_class.clone().entry_points_by_type;
    let native_constructor_selector = &native_entrypoints.constructor.get(0).unwrap().selector;

    let casm_entrypoints = casm_contract_class.clone().entry_points_by_type;
    let casm_constructor_selector = &casm_entrypoints.constructor.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    static NATIVE_CLASS_HASH: ClassHash = [1; 32];
    static CASM_CLASS_HASH: ClassHash = [2; 32];

    let caller_address = Address(123456789.into());

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        NATIVE_CLASS_HASH,
        sierra_contract_class,
    );
    contract_class_cache.insert(
        CASM_CLASS_HASH,
        CompiledClass::Casm(Arc::new(casm_contract_class)),
    );
    let mut state_reader = InMemoryStateReader::default();
    let nonce = Felt252::zero();

    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), CASM_CLASS_HASH);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let state_reader = Arc::new(state_reader);
    let mut state_vm = CachedState::new(state_reader.clone(), contract_class_cache.clone());
    let mut state_native = CachedState::new(state_reader, contract_class_cache);

    /*
        1 recipient
        2 name
        3 decimals
        4 initial_supply
        5 symbol
    */
    let calldata = [
        caller_address.0.clone(),
        2.into(),
        3.into(),
        4.into(),
        5.into(),
    ]
    .to_vec();

    let vm_result = execute(
        &mut state_vm,
        &caller_address,
        &caller_address,
        casm_constructor_selector,
        &calldata,
        EntryPointType::Constructor,
        &CASM_CLASS_HASH,
    );

    let native_result = execute(
        &mut state_native,
        &caller_address,
        &caller_address,
        native_constructor_selector,
        &calldata,
        EntryPointType::Constructor,
        &NATIVE_CLASS_HASH,
    );

    assert_eq!(vm_result.caller_address, caller_address);
    assert_eq!(vm_result.call_type, Some(CallType::Delegate));
    assert_eq!(vm_result.contract_address, caller_address);
    assert_eq!(
        vm_result.entry_point_selector,
        Some(Felt252::new(casm_constructor_selector))
    );
    assert_eq!(
        vm_result.entry_point_type,
        Some(EntryPointType::Constructor)
    );
    assert_eq!(vm_result.calldata, calldata);
    assert!(!vm_result.failure_flag);
    assert_eq!(vm_result.retdata, [].to_vec());
    assert_eq!(vm_result.class_hash, Some(CASM_CLASS_HASH));

    assert_eq!(native_result.caller_address, caller_address);
    assert_eq!(native_result.call_type, Some(CallType::Delegate));
    assert_eq!(native_result.contract_address, caller_address);
    assert_eq!(
        native_result.entry_point_selector,
        Some(Felt252::new(native_constructor_selector))
    );
    assert_eq!(
        native_result.entry_point_type,
        Some(EntryPointType::Constructor)
    );
    assert_eq!(native_result.calldata, calldata);
    assert!(!native_result.failure_flag);
    assert_eq!(native_result.retdata, [].to_vec());
    assert_eq!(native_result.execution_resources, None);
    assert_eq!(native_result.class_hash, Some(NATIVE_CLASS_HASH));

    assert_eq!(vm_result.events, native_result.events);
    assert_eq!(
        vm_result.accessed_storage_keys,
        native_result.accessed_storage_keys
    );
    assert_eq!(vm_result.l2_to_l1_messages, native_result.l2_to_l1_messages);
    assert_eq!(vm_result.gas_consumed, native_result.gas_consumed);

    #[allow(clippy::too_many_arguments)]
    fn compare_results(
        state_vm: &mut CachedState<InMemoryStateReader>,
        state_native: &mut CachedState<InMemoryStateReader>,
        selector_idx: usize,
        native_entrypoints: &ContractEntryPoints,
        casm_entrypoints: &CasmContractEntryPoints,
        calldata: &[Felt252],
        caller_address: &Address,
        debug_name: &str,
    ) {
        let native_selector = &native_entrypoints
            .external
            .get(selector_idx)
            .unwrap()
            .selector;
        let casm_selector = &casm_entrypoints
            .external
            .get(selector_idx)
            .unwrap()
            .selector;

        let vm_result = execute(
            state_vm,
            caller_address,
            caller_address,
            casm_selector,
            calldata,
            EntryPointType::External,
            &CASM_CLASS_HASH,
        );

        let native_result = execute(
            state_native,
            caller_address,
            caller_address,
            native_selector,
            calldata,
            EntryPointType::External,
            &NATIVE_CLASS_HASH,
        );

        assert_eq!(vm_result.failure_flag, native_result.failure_flag);
        assert_eq!(vm_result.retdata, native_result.retdata);
        assert_eq!(vm_result.events, native_result.events);
        assert_eq!(
            vm_result.accessed_storage_keys,
            native_result.accessed_storage_keys
        );
        assert_eq!(vm_result.l2_to_l1_messages, native_result.l2_to_l1_messages);

        assert_eq!(
            vm_result.gas_consumed, native_result.gas_consumed,
            "gas consumed mismatch for {debug_name}",
        );
    }

    // --------------- GET TOTAL SUPPLY -----------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        5,
        &native_entrypoints,
        &casm_entrypoints,
        &[],
        &caller_address,
        "get total supply 1",
    );

    // ---------------- GET DECIMALS ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        1,
        &native_entrypoints,
        &casm_entrypoints,
        &[],
        &caller_address,
        "get decimals 1",
    );

    // ---------------- GET NAME ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        6,
        &native_entrypoints,
        &casm_entrypoints,
        &[],
        &caller_address,
        "get name",
    );

    // // ---------------- GET SYMBOL ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        7,
        &native_entrypoints,
        &casm_entrypoints,
        &[],
        &caller_address,
        "get symbol",
    );

    // ---------------- GET BALANCE OF CALLER ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        8,
        &native_entrypoints,
        &casm_entrypoints,
        &[caller_address.0.clone()],
        &caller_address,
        "get balance of caller",
    );

    // // ---------------- ALLOWANCE OF ADDRESS 1 ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        3,
        &native_entrypoints,
        &casm_entrypoints,
        &[caller_address.0.clone(), 1.into()],
        &caller_address,
        "get allowance of address 1",
    );

    // // ---------------- INCREASE ALLOWANCE OF ADDRESS 1 by 10_000 ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        2,
        &native_entrypoints,
        &casm_entrypoints,
        &[1.into(), 10_000.into()],
        &caller_address,
        "increase allowance of address 1 by 10000",
    );

    // ---------------- ALLOWANCE OF ADDRESS 1 ----------------------

    // Checking again because allowance changed with previous call.
    compare_results(
        &mut state_vm,
        &mut state_native,
        3,
        &native_entrypoints,
        &casm_entrypoints,
        &[caller_address.0.clone(), 1.into()],
        &caller_address,
        "allowance of address 1 part 2",
    );

    // ---------------- APPROVE ADDRESS 1 TO MAKE TRANSFERS ON BEHALF OF THE CALLER ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        4,
        &native_entrypoints,
        &casm_entrypoints,
        &[1.into(), 5000.into()],
        &caller_address,
        "approve address 1 to make transfers",
    );

    // ---------------- TRANSFER 3 TOKENS FROM CALLER TO ADDRESS 2 ---------

    compare_results(
        &mut state_vm,
        &mut state_native,
        0,
        &native_entrypoints,
        &casm_entrypoints,
        &[2.into(), 3.into()],
        &caller_address,
        "transfer 3 tokens",
    );

    // // ---------------- GET BALANCE OF CALLER ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        8,
        &native_entrypoints,
        &casm_entrypoints,
        &[caller_address.0.clone()],
        &caller_address,
        "GET BALANCE OF CALLER",
    );

    // // ---------------- GET BALANCE OF ADDRESS 2 ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        8,
        &native_entrypoints,
        &casm_entrypoints,
        &[2.into()],
        &caller_address,
        "GET BALANCE OF ADDRESS 2",
    );

    // // ---------------- TRANSFER 1 TOKEN FROM CALLER TO ADDRESS 2, CALLED FROM ADDRESS 1 ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        9,
        &native_entrypoints,
        &casm_entrypoints,
        &[1.into(), 2.into(), 1.into()],
        &caller_address,
        "TRANSFER 1 TOKEN FROM CALLER TO ADDRESS 2, CALLED FROM ADDRESS 1",
    );

    // // ---------------- GET BALANCE OF ADDRESS 2 ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        8,
        &native_entrypoints,
        &casm_entrypoints,
        &[2.into()],
        &caller_address,
        "GET BALANCE OF ADDRESS 2 part 2",
    );

    // // ---------------- GET BALANCE OF CALLER ----------------------

    compare_results(
        &mut state_vm,
        &mut state_native,
        8,
        &native_entrypoints,
        &casm_entrypoints,
        &[caller_address.0.clone()],
        &caller_address,
        "GET BALANCE OF CALLER last",
    );
}

#[test]
fn call_contract_test() {
    // Caller contract
    let caller_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/caller.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Callee contract
    let callee_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/callee.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Caller contract entrypoints
    let caller_entrypoints = caller_contract_class.clone().entry_points_by_type;
    let call_contract_selector = &caller_entrypoints.external.get(0).unwrap().selector;

    // Callee contract entrypoints
    let callee_entrypoints = callee_contract_class.clone().entry_points_by_type;
    let fn_selector = &callee_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    // Caller contract data
    let caller_address = Address(1111.into());
    let caller_class_hash: ClassHash = [1; 32];
    let caller_nonce = Felt252::zero();

    // Callee contract data
    let callee_address = Address(1112.into());
    let callee_class_hash: ClassHash = [2; 32];
    let callee_nonce = Felt252::zero();

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        caller_class_hash,
        caller_contract_class,
    );

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        callee_class_hash,
        callee_contract_class,
    );

    let mut state_reader = InMemoryStateReader::default();

    // Insert caller contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), caller_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), caller_nonce);

    // Insert callee contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(callee_address.clone(), callee_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(callee_address.clone(), callee_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let calldata = [fn_selector.into()].to_vec();
    let result = execute(
        &mut state,
        &caller_address,
        &callee_address,
        call_contract_selector,
        &calldata,
        EntryPointType::External,
        &caller_class_hash,
    );

    assert_eq!(result.retdata, [Felt252::new(44)]);
}

#[test]
fn call_echo_contract_test() {
    // Caller contract
    let caller_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/echo_caller.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Callee contract
    let callee_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/echo.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Caller contract entrypoints
    let caller_entrypoints = caller_contract_class.clone().entry_points_by_type;
    let call_contract_selector = &caller_entrypoints.external.get(0).unwrap().selector;

    // Callee contract entrypoints
    let callee_entrypoints = callee_contract_class.clone().entry_points_by_type;
    let fn_selector = &callee_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    // Caller contract data
    let caller_address = Address(1111.into());
    let caller_class_hash: ClassHash = [1; 32];
    let caller_nonce = Felt252::zero();

    // Callee contract data
    let callee_address = Address(1112.into());
    let callee_class_hash: ClassHash = [2; 32];
    let callee_nonce = Felt252::zero();

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        caller_class_hash,
        caller_contract_class,
    );

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        callee_class_hash,
        callee_contract_class,
    );

    let mut state_reader = InMemoryStateReader::default();

    // Insert caller contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), caller_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), caller_nonce);

    // Insert callee contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(callee_address.clone(), callee_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(callee_address.clone(), callee_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let calldata = [fn_selector.into(), 99999999.into()].to_vec();
    let result = execute(
        &mut state,
        &caller_address,
        &callee_address,
        call_contract_selector,
        &calldata,
        EntryPointType::External,
        &caller_class_hash,
    );

    assert_eq!(result.retdata, [Felt252::new(99999999)]);
    assert_eq!(result.gas_consumed, 89110);
}

#[test]
#[cfg(feature = "cairo-native")]
fn call_events_contract_test() {
    // Caller contract
    let caller_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/caller.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Callee contract
    let callee_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/event_emitter.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Caller contract entrypoints
    let caller_entrypoints = caller_contract_class.clone().entry_points_by_type;
    let call_contract_selector = &caller_entrypoints.external.get(0).unwrap().selector;

    // Event emmitter contract entrypoints
    let callee_entrypoints = callee_contract_class.clone().entry_points_by_type;
    let fn_selector = &callee_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    // Caller contract data
    let caller_address = Address(1111.into());
    let caller_class_hash: ClassHash = [1; 32];
    let caller_nonce = Felt252::zero();

    // Callee contract data
    let callee_address = Address(1112.into());
    let callee_class_hash: ClassHash = [2; 32];
    let callee_nonce = Felt252::zero();

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        caller_class_hash,
        caller_contract_class,
    );

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        callee_class_hash,
        callee_contract_class,
    );

    let mut state_reader = InMemoryStateReader::default();

    // Insert caller contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), caller_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), caller_nonce);

    // Insert callee contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(callee_address.clone(), callee_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(callee_address.clone(), callee_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let calldata: Vec<Felt252> = [fn_selector.into()].to_vec();
    let result = execute(
        &mut state,
        &caller_address,
        &callee_address,
        call_contract_selector,
        &calldata,
        EntryPointType::External,
        &caller_class_hash,
    );

    let internal_call = CallInfo {
        caller_address: Address(1111.into()),
        call_type: Some(Call),
        contract_address: Address(1112.into()),
        code_address: None,
        class_hash: Some([
            2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2,
        ]),
        entry_point_selector: Some(fn_selector.into()),
        entry_point_type: Some(External),
        calldata: Vec::new(),
        retdata: vec![1234.into()],
        execution_resources: None,
        events: vec![OrderedEvent {
            order: 0,
            keys: vec![110.into()],
            data: vec![1.into()],
        }],
        l2_to_l1_messages: Vec::new(),
        storage_read_values: Vec::new(),
        accessed_storage_keys: HashSet::new(),
        internal_calls: Vec::new(),
        gas_consumed: 9640,
        failure_flag: false,
    };

    let event = Event {
        from_address: Address(1112.into()),
        keys: vec![110.into()],
        data: vec![1.into()],
    };

    assert_eq!(result.retdata, [1234.into()]);
    assert_eq!(result.events, []);
    assert_eq_sorted!(result.internal_calls, [internal_call]);

    let sorted_events = result.get_sorted_events().unwrap();
    assert_eq!(sorted_events, vec![event]);
}

#[test]
fn replace_class_test() {
    //  Create program and entry point types for contract class
    let contract_class_a: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/get_number_a.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();
    let casm_data = include_bytes!("../starknet_programs/cairo2/get_number_a.casm");
    let casm_contract_class: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

    let entrypoints_a = contract_class_a.clone().entry_points_by_type;
    let replace_selector = &entrypoints_a.external.get(0).unwrap().selector;

    let casm_entrypoints = casm_contract_class.clone().entry_points_by_type;
    let casm_replace_selector = &casm_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    let address = Address(1111.into());
    let casm_address = Address(2222.into());

    static CLASS_HASH_A: ClassHash = [1; 32];
    static CASM_CLASS_HASH_A: ClassHash = [2; 32];

    let nonce = Felt252::zero();

    insert_sierra_class_into_cache(&mut contract_class_cache, CLASS_HASH_A, contract_class_a);

    contract_class_cache.insert(
        CASM_CLASS_HASH_A,
        CompiledClass::Casm(Arc::new(casm_contract_class)),
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), CLASS_HASH_A);
    state_reader
        .address_to_class_hash_mut()
        .insert(casm_address.clone(), CASM_CLASS_HASH_A);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Add get_number_b contract to the state (only its contract_class)
    let contract_class_b: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/get_number_b.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();
    let casm_data = include_bytes!("../starknet_programs/cairo2/get_number_b.casm");
    let casm_contract_class_b: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

    static CLASS_HASH_B: ClassHash = [3; 32];
    static CASM_CLASS_HASH_B: ClassHash = [4; 32];

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        CLASS_HASH_B,
        contract_class_b.clone(),
    );

    contract_class_cache.insert(
        CASM_CLASS_HASH_B,
        CompiledClass::Casm(Arc::new(casm_contract_class_b.clone())),
    );

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader.clone()), contract_class_cache.clone());
    let mut vm_state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    // Run upgrade entrypoint and check that the storage was updated with the new contract class
    // Create an execution entry point
    let calldata = [Felt252::from_bytes_be(&CLASS_HASH_B)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;
    let native_result = execute(
        &mut state,
        &caller_address,
        &address,
        replace_selector,
        &calldata,
        entry_point_type,
        &CLASS_HASH_A,
    );
    let calldata = [Felt252::from_bytes_be(&CASM_CLASS_HASH_B)].to_vec();
    let vm_result = execute(
        &mut vm_state,
        &caller_address,
        &casm_address,
        casm_replace_selector,
        &calldata,
        entry_point_type,
        &CASM_CLASS_HASH_A,
    );

    // Check that the class was indeed replaced in storage
    assert_eq!(state.get_class_hash_at(&address).unwrap(), CLASS_HASH_B);
    // Check that the class_hash_b leads to contract_class_b for soundness
    let sierra_program = contract_class_b.extract_sierra_program().unwrap();
    let entry_points = contract_class_b.entry_points_by_type;
    assert_eq!(
        state.get_contract_class(&CLASS_HASH_B).unwrap(),
        CompiledClass::Sierra(Arc::new((sierra_program, entry_points))),
    );

    // Check that the class was indeed replaced in storage
    assert_eq!(
        vm_state.get_class_hash_at(&casm_address).unwrap(),
        CASM_CLASS_HASH_B
    );
    // Check that the class_hash_b leads to contract_class_b for soundness
    assert_eq!(
        vm_state.get_contract_class(&CASM_CLASS_HASH_B).unwrap(),
        CompiledClass::Casm(Arc::new(casm_contract_class_b))
    );

    assert_eq!(native_result.retdata, vm_result.retdata);
    assert_eq!(native_result.events, vm_result.events);
    assert_eq!(
        native_result.accessed_storage_keys,
        vm_result.accessed_storage_keys
    );
    assert_eq!(native_result.l2_to_l1_messages, vm_result.l2_to_l1_messages);
    assert_eq!(native_result.gas_consumed, vm_result.gas_consumed);
    assert_eq!(native_result.failure_flag, vm_result.failure_flag);
    assert_eq_sorted!(native_result.internal_calls, vm_result.internal_calls);
    assert_eq!(native_result.class_hash.unwrap(), CLASS_HASH_A);
    assert_eq!(vm_result.class_hash.unwrap(), CASM_CLASS_HASH_A);
    assert_eq!(native_result.caller_address, caller_address);
    assert_eq!(vm_result.caller_address, caller_address);
    assert_eq!(native_result.call_type, vm_result.call_type);
    assert_eq!(native_result.contract_address, address);
    assert_eq!(vm_result.contract_address, casm_address);
    assert_eq!(native_result.code_address, vm_result.code_address);
    assert_eq!(
        native_result.entry_point_selector,
        vm_result.entry_point_selector
    );
    assert_eq!(native_result.entry_point_type, vm_result.entry_point_type);
}

#[test]
fn replace_class_contract_call() {
    fn compare_results(native_result: CallInfo, vm_result: CallInfo) {
        assert_eq!(vm_result.retdata, native_result.retdata);
        assert_eq!(vm_result.events, native_result.events);
        assert_eq!(
            vm_result.accessed_storage_keys,
            native_result.accessed_storage_keys
        );
        assert_eq!(vm_result.l2_to_l1_messages, native_result.l2_to_l1_messages);
        assert_eq!(vm_result.gas_consumed, native_result.gas_consumed);
        assert_eq!(vm_result.failure_flag, false);
        assert_eq!(native_result.failure_flag, false);
        assert_eq_sorted!(vm_result.internal_calls, native_result.internal_calls);
        assert_eq!(
            vm_result.accessed_storage_keys,
            native_result.accessed_storage_keys
        );
        assert_eq!(
            vm_result.storage_read_values,
            native_result.storage_read_values
        );
        assert_eq!(vm_result.class_hash, native_result.class_hash);
    }
    // Same execution than cairo_1_syscalls.rs test but comparing results to native execution.

    // SET GET_NUMBER_A
    // Add get_number_a.cairo to storage
    let program_data = include_bytes!("../starknet_programs/cairo2/get_number_a.casm");
    let casm_contract_class_a: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let sierra_class_a: cairo_lang_starknet::contract_class::ContractClass = serde_json::from_str(
        std::fs::read_to_string("starknet_programs/cairo2/get_number_a.sierra")
            .unwrap()
            .as_str(),
    )
    .unwrap();

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();
    let mut native_contract_class_cache = HashMap::new();

    let address = Address(Felt252::one());
    let class_hash_a: ClassHash = [1; 32];
    let nonce = Felt252::zero();

    contract_class_cache.insert(
        class_hash_a,
        CompiledClass::Casm(Arc::new(casm_contract_class_a)),
    );
    insert_sierra_class_into_cache(
        &mut native_contract_class_cache,
        class_hash_a,
        sierra_class_a,
    );

    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_a);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce.clone());

    let mut native_state_reader = InMemoryStateReader::default();
    native_state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_a);

    // SET GET_NUMBER_B

    // Add get_number_b contract to the state (only its contract_class)

    let program_data = include_bytes!("../starknet_programs/cairo2/get_number_b.casm");
    let contract_class_b: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let sierra_class_b: cairo_lang_starknet::contract_class::ContractClass = serde_json::from_str(
        std::fs::read_to_string("starknet_programs/cairo2/get_number_b.sierra")
            .unwrap()
            .as_str(),
    )
    .unwrap();
    let class_hash_b: ClassHash = [2; 32];

    contract_class_cache.insert(
        class_hash_b,
        CompiledClass::Casm(Arc::new(contract_class_b)),
    );
    insert_sierra_class_into_cache(
        &mut native_contract_class_cache,
        class_hash_b,
        sierra_class_b,
    );

    // SET GET_NUMBER_WRAPPER

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../starknet_programs/cairo2/get_number_wrapper.casm");
    let wrapper_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = wrapper_contract_class.clone().entry_points_by_type;
    let get_number_entrypoint_selector = &entrypoints.external.get(1).unwrap().selector;
    let upgrade_entrypoint_selector: &BigUint = &entrypoints.external.get(0).unwrap().selector;

    let wrapper_sierra_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/get_number_wrapper.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();
    let native_entrypoints = wrapper_sierra_class.clone().entry_points_by_type;

    let native_get_number_entrypoint_selector =
        &native_entrypoints.external.get(1).unwrap().selector;
    let native_upgrade_entrypoint_selector: &BigUint =
        &native_entrypoints.external.get(0).unwrap().selector;

    let wrapper_address = Address(Felt252::from(2));
    let wrapper_class_hash: ClassHash = [3; 32];

    contract_class_cache.insert(
        wrapper_class_hash,
        CompiledClass::Casm(Arc::new(wrapper_contract_class)),
    );
    insert_sierra_class_into_cache(
        &mut native_contract_class_cache,
        wrapper_class_hash,
        wrapper_sierra_class,
    );

    state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address.clone(), wrapper_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(wrapper_address.clone(), nonce);

    native_state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address, wrapper_class_hash);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader.clone()), contract_class_cache.clone());
    let mut native_state = CachedState::new(Arc::new(state_reader), contract_class_cache);
    // CALL GET_NUMBER BEFORE REPLACE_CLASS

    let calldata = [].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let vm_result = execute(
        &mut state,
        &caller_address,
        &address,
        get_number_entrypoint_selector,
        &calldata,
        entry_point_type,
        &wrapper_class_hash,
    );

    let native_result = execute(
        &mut native_state,
        &caller_address,
        &address,
        native_get_number_entrypoint_selector,
        &calldata,
        entry_point_type,
        &wrapper_class_hash,
    );
    compare_results(native_result, vm_result);

    // REPLACE_CLASS

    let calldata = [Felt252::from_bytes_be(&class_hash_b)].to_vec();

    let vm_result = execute(
        &mut state,
        &caller_address,
        &address,
        upgrade_entrypoint_selector,
        &calldata,
        entry_point_type,
        &wrapper_class_hash,
    );

    let native_result = execute(
        &mut native_state,
        &caller_address,
        &address,
        native_upgrade_entrypoint_selector,
        &calldata,
        entry_point_type,
        &wrapper_class_hash,
    );
    compare_results(native_result, vm_result);
    // CALL GET_NUMBER AFTER REPLACE_CLASS

    let calldata = [].to_vec();

    let vm_result = execute(
        &mut state,
        &caller_address,
        &address,
        get_number_entrypoint_selector,
        &calldata,
        entry_point_type,
        &wrapper_class_hash,
    );

    let native_result = execute(
        &mut native_state,
        &caller_address,
        &address,
        native_get_number_entrypoint_selector,
        &calldata,
        entry_point_type,
        &wrapper_class_hash,
    );
    compare_results(native_result, vm_result);
}

#[test]
#[cfg(feature = "cairo-native")]
fn keccak_syscall_test() {
    let sierra_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/test_cairo_keccak.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    let native_entrypoints = sierra_contract_class.clone().entry_points_by_type;
    let native_entrypoint_selector = &native_entrypoints.external.get(0).unwrap().selector;

    let native_class_hash: ClassHash = [1; 32];

    let caller_address = Address(123456789.into());
    let mut contract_class_cache = HashMap::new();

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        native_class_hash,
        sierra_contract_class,
    );

    let mut state_reader = InMemoryStateReader::default();
    let nonce = Felt252::zero();

    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let native_result = execute(
        &mut state,
        &caller_address,
        &caller_address,
        native_entrypoint_selector,
        &[],
        EntryPointType::External,
        &native_class_hash,
    );

    assert!(!native_result.failure_flag);
    assert_eq!(native_result.gas_consumed, 545370);
}
fn execute(
    state: &mut CachedState<InMemoryStateReader>,
    caller_address: &Address,
    callee_address: &Address,
    selector: &BigUint,
    calldata: &[Felt252],
    entrypoint_type: EntryPointType,
    class_hash: &ClassHash,
) -> CallInfo {
    let exec_entry_point = ExecutionEntryPoint::new(
        (*callee_address).clone(),
        calldata.to_vec(),
        Felt252::new(selector),
        (*caller_address).clone(),
        entrypoint_type,
        Some(CallType::Delegate),
        Some(*class_hash),
        u64::MAX.into(),
    );

    // Execute the entrypoint
    // Set up the current block number
    let mut block_context = BlockContext::default();
    block_context.block_info_mut().block_number = 30;

    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::zero(),
        Vec::new(),
        0,
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        TRANSACTION_VERSION.clone(),
    );
    let mut resources_manager = ExecutionResourcesManager::default();

    exec_entry_point
        .execute(
            state,
            &block_context,
            &mut resources_manager,
            &mut tx_execution_context,
            false,
            block_context.invoke_tx_max_n_steps(),
        )
        .unwrap()
        .call_info
        .unwrap()
}

#[test]
fn library_call() {
    //  Create program and entry point types for contract class
    let contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_slice(include_bytes!(
            "../starknet_programs/cairo2/square_root.sierra"
        ))
        .unwrap();

    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    let address = Address(1111.into());
    let class_hash: ClassHash = [1; 32];
    let nonce = Felt252::zero();

    contract_class_cache.insert(
        class_hash,
        CompiledClass::Sierra(Arc::new((
            contract_class.extract_sierra_program().unwrap(),
            entrypoints.clone(),
        ))),
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Add lib contract to the state

    let lib_program_data = include_bytes!("../starknet_programs/cairo2/math_lib.sierra");

    let lib_contract_class: ContractClass = serde_json::from_slice(lib_program_data).unwrap();

    let lib_address = Address(1112.into());
    let lib_class_hash: ClassHash = [2; 32];
    let lib_nonce = Felt252::zero();

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        lib_class_hash,
        lib_contract_class,
    );

    state_reader
        .address_to_class_hash_mut()
        .insert(lib_address.clone(), lib_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(lib_address, lib_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    // Create an execution entry point
    let calldata = [25.into(), Felt252::from_bytes_be(&lib_class_hash)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata.clone(),
        Felt252::new(entrypoint_selector.clone()),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        100000,
    );

    // Execute the entrypoint
    let block_context = BlockContext::default();
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::zero(),
        Vec::new(),
        0,
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        TRANSACTION_VERSION.clone(),
    );
    let mut resources_manager = ExecutionResourcesManager::default();

    // expected results
    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: Address(1111.into()),
        entry_point_selector: Some(Felt252::new(entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        calldata,
        retdata: [5.into()].to_vec(),
        execution_resources: None,
        class_hash: Some(class_hash),
        internal_calls: vec![CallInfo {
            caller_address: Address(0.into()),
            call_type: Some(CallType::Delegate),
            contract_address: Address(1111.into()),
            entry_point_selector: Some(
                Felt252::from_str_radix(
                    "544923964202674311881044083303061611121949089655923191939299897061511784662",
                    10,
                )
                .unwrap(),
            ),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![25.into()],
            retdata: [5.into()].to_vec(),
            execution_resources: None,
            class_hash: Some(lib_class_hash),
            gas_consumed: 0,
            ..Default::default()
        }],
        code_address: None,
        events: vec![],
        l2_to_l1_messages: vec![],
        storage_read_values: vec![],
        accessed_storage_keys: HashSet::new(),
        gas_consumed: 78250,
        ..Default::default()
    };

    assert_eq_sorted!(
        exec_entry_point
            .execute(
                &mut state,
                &block_context,
                &mut resources_manager,
                &mut tx_execution_context,
                false,
                block_context.invoke_tx_max_n_steps()
            )
            .unwrap()
            .call_info
            .unwrap(),
        expected_call_info
    );
}

fn execute_deploy(
    state: &mut CachedState<InMemoryStateReader>,
    caller_address: &Address,
    selector: &BigUint,
    calldata: &[Felt252],
    entrypoint_type: EntryPointType,
    class_hash: &ClassHash,
) -> CallInfo {
    let exec_entry_point = ExecutionEntryPoint::new(
        (*caller_address).clone(),
        calldata.to_vec(),
        Felt252::new(selector),
        (*caller_address).clone(),
        entrypoint_type,
        Some(CallType::Delegate),
        Some(*class_hash),
        u64::MAX.into(),
    );

    // Execute the entrypoint
    let block_context = BlockContext::default();
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::zero(),
        Vec::new(),
        0,
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        TRANSACTION_VERSION.clone(),
    );
    let mut resources_manager = ExecutionResourcesManager::default();

    exec_entry_point
        .execute(
            state,
            &block_context,
            &mut resources_manager,
            &mut tx_execution_context,
            false,
            block_context.invoke_tx_max_n_steps(),
        )
        .unwrap()
        .call_info
        .unwrap()
}

#[test]
#[cfg(feature = "cairo-native")]
fn deploy_syscall_test() {
    // Deployer contract

    let deployer_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/deploy.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Deployee contract
    let deployee_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/echo.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // deployer contract entrypoints
    let deployer_entrypoints = deployer_contract_class.clone().entry_points_by_type;
    let deploy_contract_selector = &deployer_entrypoints.external.get(0).unwrap().selector;

    // Echo contract entrypoints
    let deployee_entrypoints = deployee_contract_class.clone().entry_points_by_type;
    let _fn_selector = &deployee_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    // Deployer contract data
    let deployer_address = Address(1111.into());
    let deployer_class_hash: ClassHash = [1; 32];
    let deployer_nonce = Felt252::zero();

    // Deployee contract data
    let deployee_class_hash: ClassHash = Felt252::one().to_be_bytes();
    let _deployee_nonce = Felt252::zero();

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        deployer_class_hash,
        deployer_contract_class,
    );

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        deployee_class_hash,
        deployee_contract_class,
    );

    let mut state_reader = InMemoryStateReader::default();

    // Insert deployer contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(deployer_address.clone(), deployer_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(deployer_address.clone(), deployer_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let calldata = [Felt252::from_bytes_be(&deployee_class_hash), Felt252::one()].to_vec();
    let result = execute_deploy(
        &mut state,
        &deployer_address,
        deploy_contract_selector,
        &calldata,
        EntryPointType::External,
        &deployer_class_hash,
    );
    let expected_deployed_contract_address = Address(
        calculate_contract_address(
            &Felt252::one(),
            &Felt252::from_bytes_be(&deployee_class_hash),
            &[100.into()],
            deployer_address,
        )
        .unwrap(),
    );

    assert_eq!(result.retdata, [expected_deployed_contract_address.0]);
    assert_eq!(result.events, []);
    assert_eq!(result.internal_calls.len(), 1);

    let sorted_events = result.get_sorted_events().unwrap();
    assert_eq!(sorted_events, vec![]);
    assert_eq!(result.failure_flag, false)
}

#[test]
#[cfg(feature = "cairo-native")]
fn deploy_syscall_address_unavailable_test() {
    // Deployer contract

    use starknet_in_rust::utils::felt_to_hash;
    let deployer_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/deploy.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Deployee contract
    let deployee_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/echo.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // deployer contract entrypoints
    let deployer_entrypoints = deployer_contract_class.clone().entry_points_by_type;
    let deploy_contract_selector = &deployer_entrypoints.external.get(0).unwrap().selector;

    // Echo contract entrypoints
    let deployee_entrypoints = deployee_contract_class.clone().entry_points_by_type;
    let _fn_selector = &deployee_entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    // Deployer contract data
    let deployer_address = Address(1111.into());
    let deployer_class_hash: ClassHash = [2; 32];
    let deployer_nonce = Felt252::zero();

    // Deployee contract data
    let deployee_class_hash: ClassHash = felt_to_hash(&Felt252::one());
    let deployee_nonce = Felt252::zero();
    let expected_deployed_contract_address = Address(
        calculate_contract_address(
            &Felt252::one(),
            &Felt252::from_bytes_be(&deployee_class_hash),
            &[100.into()],
            deployer_address.clone(),
        )
        .unwrap(),
    );
    // Insert contract to be deployed so that its address is taken
    let deployee_address = expected_deployed_contract_address;

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        deployer_class_hash,
        deployer_contract_class,
    );

    insert_sierra_class_into_cache(
        &mut contract_class_cache,
        deployee_class_hash,
        deployee_contract_class,
    );

    let mut state_reader = InMemoryStateReader::default();

    // Insert deployer contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(deployer_address.clone(), deployer_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(deployer_address.clone(), deployer_nonce);

    // Insert deployee contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(deployee_address.clone(), deployee_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(deployee_address.clone(), deployee_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let calldata = [Felt252::from_bytes_be(&deployee_class_hash), Felt252::one()].to_vec();
    let result = execute_deploy(
        &mut state,
        &deployer_address,
        deploy_contract_selector,
        &calldata,
        EntryPointType::External,
        &deployer_class_hash,
    );

    assert_eq!(
        std::str::from_utf8(&result.retdata[0].to_be_bytes())
            .unwrap()
            .trim_start_matches('\0'),
        "Result::unwrap failed."
    );
    assert_eq!(result.events, []);
    assert_eq!(result.failure_flag, true);
    assert!(result.internal_calls.is_empty());
}

#[test]
#[cfg(feature = "cairo-native")]
fn get_execution_info_test() {
    // Same test as test_get_execution_info in the cairo_1_syscalls.rs but in native

    let sierra_contract_class: cairo_lang_starknet::contract_class::ContractClass =
        serde_json::from_str(
            std::fs::read_to_string("starknet_programs/cairo2/get_execution_info.sierra")
                .unwrap()
                .as_str(),
        )
        .unwrap();

    // Contract entrypoints
    let entrypoints = sierra_contract_class.clone().entry_points_by_type;
    let selector = &entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();

    // Contract data
    let address = Address(1111.into());
    let class_hash: ClassHash = [1; 32];
    let nonce = Felt252::zero();

    insert_sierra_class_into_cache(&mut contract_class_cache, class_hash, sierra_contract_class);

    let mut state_reader = InMemoryStateReader::default();

    // Insert caller contract info into state reader
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let calldata = [].to_vec();

    // Create the entrypoint
    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata.to_vec(),
        Felt252::new(selector),
        Address(0.into()),
        EntryPointType::External,
        Some(CallType::Delegate),
        Some(class_hash),
        u128::MAX,
    );

    // Create default BlockContext
    let block_context = BlockContext::default();

    // Create TransactionExecutionContext
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::zero(),
        vec![22.into(), 33.into()],
        0,
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        TRANSACTION_VERSION.clone(),
    );
    let mut resources_manager = ExecutionResourcesManager::default();

    // Execute the entrypoint
    let call_info = exec_entry_point
        .execute(
            &mut state,
            &block_context,
            &mut resources_manager,
            &mut tx_execution_context,
            false,
            block_context.invoke_tx_max_n_steps(),
        )
        .unwrap()
        .call_info
        .unwrap();

    let expected_ret_data = vec![
        block_context.block_info().sequencer_address.0.clone(),
        0.into(),
        0.into(),
        address.0.clone(),
    ];

    let expected_gas_consumed = 22980;

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: address,
        class_hash: Some(class_hash),
        entry_point_selector: Some(selector.into()),
        entry_point_type: Some(EntryPointType::External),
        retdata: expected_ret_data,
        execution_resources: None,
        gas_consumed: expected_gas_consumed,
        ..Default::default()
    };

    assert_eq!(call_info, expected_call_info);
}
