#![cfg(not(feature = "cairo_1_tests"))]

use cairo_vm::felt::Felt252;
use num_bigint::BigUint;
use num_traits::Zero;
use starknet_in_rust::definitions::block_context::BlockContext;
use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
use starknet_in_rust::CasmContractClass;
use starknet_in_rust::EntryPointType;
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
use std::sync::Arc;

pub fn main() {
    let args: Vec<String> = std::env::args().collect();

    bench_erc20(args.get(1) == Some(&"native".to_string()))
}

fn bench_erc20(native: bool) {
    // Create state reader with class hash data
    let mut contract_class_cache = HashMap::new();
    static CASM_CLASS_HASH: ClassHash = [2; 32];

    let (contract_class, constructor_selector) = match native {
        true => {
            let sierra_data = include_bytes!("../starknet_programs/cairo2/erc20.sierra");
            let sierra_contract_class: cairo_lang_starknet::contract_class::ContractClass =
                serde_json::from_slice(sierra_data).unwrap();

            let entrypoints = sierra_contract_class.clone().entry_points_by_type;
            let constructor_selector = entrypoints.constructor.get(0).unwrap().selector.clone();

            (CompiledClass::Sierra(Arc::new(sierra_contract_class)), constructor_selector)
        },
        false => {
            let casm_data = include_bytes!("../starknet_programs/cairo2/erc20.casm");
            let casm_contract_class: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

            let entrypoints = casm_contract_class.clone().entry_points_by_type;
            let constructor_selector = entrypoints.constructor.get(0).unwrap().selector.clone();

            (CompiledClass::Casm(Arc::new(casm_contract_class)), constructor_selector)
        }
    };

    let caller_address = Address(123456789.into());

    contract_class_cache.insert(
        CASM_CLASS_HASH,
        contract_class,
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
    let mut state = CachedState::new(state_reader, contract_class_cache);

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

    let result = execute(
        &mut state.clone(),
        &caller_address,
        &caller_address,
        &constructor_selector.clone(),
        &calldata.clone(),
        EntryPointType::Constructor,
        &CASM_CLASS_HASH,
    );

    dbg!(result);
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
        u64::MAX.into(), // gas is u64 in cairo-native and sierra
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
