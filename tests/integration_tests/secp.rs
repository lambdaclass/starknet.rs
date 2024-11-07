// #![deny(warnings)]

use cairo_lang_starknet::casm_contract_class::CasmContractClass;
use cairo_vm::{
    vm::runners::{builtin_runner::RANGE_CHECK_BUILTIN_NAME, cairo_runner::ExecutionResources},
    Felt252,
};

use pretty_assertions_sorted::assert_eq;
use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        ExecutionResourcesManager,
    },
    transaction::{Address, ClassHash},
    EntryPointType,
};
use std::{collections::HashMap, sync::Arc};

#[test]
fn secp_syscall_test() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/secp.casm");

    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let secp_entrypoint_selector = &entrypoints.external.get(0).unwrap().selector;

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(1111.into());
    let class_hash: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash,
        CompiledClass::Casm {
            casm: Arc::new(contract_class),
            sierra: None,
        },
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata: Vec<Felt252> = [0.into(), Felt252::from_hex("0x6d921cc3a0edd").unwrap()].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata.clone(),
        Felt252::from(secp_entrypoint_selector),
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
        Felt252::ZERO,
        Vec::new(),
        Default::default(),
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        *TRANSACTION_VERSION,
    );
    let mut resources_manager = ExecutionResourcesManager::default();

    // expected results
    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: Address(1111.into()),
        entry_point_selector: Some(Felt252::from(secp_entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        calldata,
        retdata: [144.into()].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 301,
            n_memory_holes: 0,
            builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 15)]),
        }),
        class_hash: Some(class_hash),
        gas_consumed: 23020,
        ..Default::default()
    };
    let callinfo = exec_entry_point
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
        .unwrap();

    let retdata: Vec<String> = callinfo
        .retdata
        .clone()
        .into_iter()
        .map(|e| format!("{e}"))
        .collect();
    dbg!(format!("{:?}", retdata));

    assert_eq!(callinfo, expected_call_info);
}
