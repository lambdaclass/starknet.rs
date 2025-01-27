use cairo_lang_starknet::casm_contract_class::CasmContractClass;
use cairo_vm::{
    vm::runners::{builtin_runner::RANGE_CHECK_BUILTIN_NAME, cairo_runner::ExecutionResources},
    Felt252,
};
use num_bigint::BigUint;

use pretty_assertions_sorted::{assert_eq, assert_eq_sorted};
use starknet_in_rust::utils::calculate_sn_keccak;
use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, L2toL1MessageInfo,
        OrderedEvent, OrderedL2ToL1Message, TransactionExecutionContext,
    },
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
    },
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        state_api::StateReader,
        ExecutionResourcesManager,
    },
    transaction::{Address, ClassHash},
    EntryPointType,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

fn create_execute_extrypoint(
    address: Address,
    class_hash: ClassHash,
    selector: &BigUint,
    calldata: Vec<Felt252>,
    entry_point_type: EntryPointType,
) -> ExecutionEntryPoint {
    ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(selector),
        Address(0000.into()),
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        100000000,
    )
}

#[test]
fn storage_write_read() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/simple_wallet.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let constructor_entrypoint_selector = &entrypoints.constructor.first().unwrap().selector;
    let get_balance_entrypoint_selector = &entrypoints.external.get(1).unwrap().selector;
    let increase_balance_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // RUN CONSTRUCTOR
    // Create an execution entry point
    let calldata = [25.into()].to_vec();
    let constructor_exec_entry_point = create_execute_extrypoint(
        address.clone(),
        class_hash,
        constructor_entrypoint_selector,
        calldata,
        EntryPointType::Constructor,
    );

    // Run constructor entrypoint
    constructor_exec_entry_point
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
        .unwrap();

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        address.clone(),
        class_hash,
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [25.into()]);

    // RUN INCREASE_BALANCE
    // Create an execution entry point
    let calldata = [100.into()].to_vec();
    let increase_balance_entry_point = create_execute_extrypoint(
        address.clone(),
        class_hash,
        increase_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run increase_balance entrypoint
    increase_balance_entry_point
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
        .unwrap();

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        address,
        class_hash,
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [125.into()])
}

#[test]
fn library_call() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/square_root.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // Add lib contract to the state

    let lib_program_data = include_bytes!("../../starknet_programs/cairo2/math_lib.casm");
    let lib_contract_class: CasmContractClass = serde_json::from_slice(lib_program_data).unwrap();

    let lib_address = Address(1112.into());
    let lib_class_hash: ClassHash = ClassHash([2; 32]);
    let lib_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        lib_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(lib_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(lib_address.clone(), lib_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(lib_address, lib_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [25.into(), Felt252::from_bytes_be(&lib_class_hash.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata.clone(),
        Felt252::from(entrypoint_selector),
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
    let expected_execution_resources = ExecutionResources {
        n_steps: 238,
        n_memory_holes: 8,
        builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 12)]),
    };
    let expected_execution_resources_internal_call = ExecutionResources {
        n_steps: 78,
        n_memory_holes: 5,
        builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 7)]),
    };

    // expected results
    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: Address(1111.into()),
        entry_point_selector: Some(Felt252::from(entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        calldata,
        retdata: [5.into()].to_vec(),
        execution_resources: Some(expected_execution_resources),
        class_hash: Some(class_hash),
        internal_calls: vec![CallInfo {
            caller_address: Address(0.into()),
            call_type: Some(CallType::Delegate),
            contract_address: Address(1111.into()),
            entry_point_selector: Some(
                Felt252::from_dec_str(
                    "544923964202674311881044083303061611121949089655923191939299897061511784662",
                )
                .unwrap(),
            ),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![25.into()],
            retdata: [5.into()].to_vec(),
            execution_resources: Some(expected_execution_resources_internal_call),
            class_hash: Some(lib_class_hash),
            gas_consumed: 0,
            ..Default::default()
        }],
        code_address: None,
        events: vec![],
        l2_to_l1_messages: vec![],
        storage_read_values: vec![],
        accessed_storage_keys: HashSet::new(),
        gas_consumed: 77550,
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

#[test]
fn call_contract_storage_write_read() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/wallet_wrapper.casm");

    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let get_balance_entrypoint_selector =
        &BigUint::from_bytes_be(&calculate_sn_keccak("get_balance".as_bytes()));
    let increase_balance_entrypoint_selector =
        &BigUint::from_bytes_be(&calculate_sn_keccak("increase_balance".as_bytes()));

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

    // Add simple_wallet contract to the state
    let simple_wallet_program_data =
        include_bytes!("../../starknet_programs/cairo2/simple_wallet.casm");

    let simple_wallet_contract_class: CasmContractClass =
        serde_json::from_slice(simple_wallet_program_data).unwrap();
    let simple_wallet_constructor_entrypoint_selector = simple_wallet_contract_class
        .entry_points_by_type
        .constructor
        .first()
        .unwrap()
        .selector
        .clone();

    let simple_wallet_address = Address(1112.into());
    let simple_wallet_class_hash: ClassHash = ClassHash([2; 32]);
    let simple_wallet_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        simple_wallet_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(simple_wallet_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(simple_wallet_address.clone(), simple_wallet_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(simple_wallet_address.clone(), simple_wallet_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

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

    let create_execute_extrypoint = |selector: &BigUint,
                                     calldata: Vec<Felt252>,
                                     entry_point_type: EntryPointType,
                                     class_hash: [u8; 32],
                                     address: Address|
     -> ExecutionEntryPoint {
        ExecutionEntryPoint::new(
            address,
            calldata,
            Felt252::from(selector),
            Address(0000.into()),
            entry_point_type,
            Some(CallType::Delegate),
            Some(ClassHash(class_hash)),
            u64::MAX.into(),
        )
    };

    // RUN SIMPLE_WALLET CONSTRUCTOR
    // Create an execution entry point
    let calldata = [25.into()].to_vec();
    let constructor_exec_entry_point = create_execute_extrypoint(
        &simple_wallet_constructor_entrypoint_selector,
        calldata,
        EntryPointType::Constructor,
        simple_wallet_class_hash.0,
        simple_wallet_address.clone(),
    );

    // Run constructor entrypoint
    constructor_exec_entry_point
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
        .unwrap();

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [simple_wallet_address.0].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address.clone(),
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [25.into()]);

    // RUN INCREASE_BALANCE
    // Create an execution entry point
    let calldata = [100.into(), simple_wallet_address.0].to_vec();
    let increase_balance_entry_point = create_execute_extrypoint(
        increase_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address.clone(),
    );

    // Run increase_balance entrypoint
    increase_balance_entry_point
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
        .unwrap();

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [simple_wallet_address.0].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address,
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [125.into()])
}

#[test]
fn emit_event() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/emit_event.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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
    let calldata = [].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
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
    let call_info = exec_entry_point
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
        .unwrap();
    assert_eq!(
        call_info.call_info.unwrap().events,
        vec![
            OrderedEvent {
                order: 0,
                keys: vec![Felt252::from_dec_str(
                    "1533133552972353850845856330693290141476612241335297758062928121906575244541"
                )
                .unwrap()],
                data: vec![1.into()]
            },
            OrderedEvent {
                order: 1,
                keys: vec![Felt252::from_dec_str(
                    "1533133552972353850845856330693290141476612241335297758062928121906575244541"
                )
                .unwrap()],
                data: vec![2.into()]
            },
            OrderedEvent {
                order: 2,
                keys: vec![Felt252::from_dec_str(
                    "1533133552972353850845856330693290141476612241335297758062928121906575244541"
                )
                .unwrap()],
                data: vec![3.into()]
            }
        ]
    )
}

#[test]
fn deploy_cairo1_from_cairo1() {
    // data to deploy
    let test_class_hash_bytes: [u8; 32] = [2; 32];
    let test_class_hash = ClassHash(test_class_hash_bytes);
    let test_felt_hash = Felt252::from_bytes_be(&test_class_hash_bytes);
    let salt = Felt252::ZERO;
    let test_data = include_bytes!("../../starknet_programs/cairo2/contract_a.casm");
    let test_contract_class: CasmContractClass = serde_json::from_slice(test_data).unwrap();

    // Create the deploy contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/deploy.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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
    contract_class_cache.set_contract_class(
        test_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(test_contract_class.clone()),
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

    // arguments of deploy contract
    let calldata: Vec<_> = [test_felt_hash, salt].to_vec();

    // set up remaining structures

    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        100_000_000,
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

    let call_info = exec_entry_point.execute(
        &mut state,
        &block_context,
        &mut resources_manager,
        &mut tx_execution_context,
        false,
        block_context.invoke_tx_max_n_steps(),
        #[cfg(feature = "cairo-native")]
        None,
    );

    assert!(call_info.is_ok());

    let ret_address = Address(
        Felt252::from_dec_str(
            "619464431559909356793718633071398796109800070568878623926447195121629120356",
        )
        .unwrap(),
    );

    let ret_class_hash = state.get_class_hash_at(&ret_address).unwrap();
    let ret_casm_class = match state.get_contract_class(&ret_class_hash).unwrap() {
        CompiledClass::Casm { casm: class, .. } => class.as_ref().clone(),
        CompiledClass::Deprecated(_) => unreachable!(),
    };

    assert_eq!(ret_casm_class, test_contract_class);
}

#[test]
fn deploy_cairo0_from_cairo1_without_constructor() {
    // data to deploy
    let test_class_hash: ClassHash = ClassHash([2; 32]);
    let test_felt_hash = Felt252::from_bytes_be(&test_class_hash.0);
    let salt = Felt252::ZERO;
    let contract_path = "starknet_programs/fibonacci.json";
    let test_contract_class: ContractClass = ContractClass::from_path(contract_path).unwrap();

    // Create the deploy contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/deploy_without_constructor.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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
    contract_class_cache.set_contract_class(
        test_class_hash,
        CompiledClass::Deprecated(Arc::new(test_contract_class.clone())),
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

    // arguments of deploy contract
    let calldata: Vec<_> = [test_felt_hash, salt].to_vec();

    // set up remaining structures

    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        100_000_000,
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

    let call_info = exec_entry_point.execute(
        &mut state,
        &block_context,
        &mut resources_manager,
        &mut tx_execution_context,
        false,
        block_context.invoke_tx_max_n_steps(),
        #[cfg(feature = "cairo-native")]
        None,
    );

    assert!(call_info.is_ok());

    let ret_address = Address(
        Felt252::from_dec_str(
            "3326516449409112130211257005742850249535379011750934837578774621442000311202",
        )
        .unwrap(),
    );

    let ret_class_hash = state.get_class_hash_at(&ret_address).unwrap();
    let ret_casm_class = match state.get_contract_class(&ret_class_hash).unwrap() {
        CompiledClass::Deprecated(class) => class.as_ref().clone(),
        CompiledClass::Casm { .. } => unreachable!(),
    };

    assert_eq!(ret_casm_class, test_contract_class);
}

#[test]
fn deploy_cairo0_from_cairo1_with_constructor() {
    // data to deploy
    let test_class_hash: ClassHash = ClassHash([2; 32]);
    let test_felt_hash = Felt252::from_bytes_be(&test_class_hash.0);
    let salt = Felt252::ZERO;
    let contract_path = "starknet_programs/test_contract.json";
    let test_contract_class: ContractClass = ContractClass::from_path(contract_path).unwrap();

    // Create the deploy contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/deploy_with_constructor.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(1111.into());
    let class_hash: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

    // simulate contract declare
    contract_class_cache.set_contract_class(
        class_hash,
        CompiledClass::Casm {
            casm: Arc::new(contract_class),
            sierra: None,
        },
    );
    contract_class_cache.set_contract_class(
        test_class_hash,
        CompiledClass::Deprecated(Arc::new(test_contract_class.clone())),
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

    // arguments of deploy contract
    let calldata: Vec<_> = [test_felt_hash, salt, address.0, Felt252::ZERO].to_vec();

    // set up remaining structures

    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        100_000_000,
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

    let call_info = exec_entry_point.execute(
        &mut state,
        &block_context,
        &mut resources_manager,
        &mut tx_execution_context,
        false,
        block_context.invoke_tx_max_n_steps(),
        #[cfg(feature = "cairo-native")]
        None,
    );

    assert!(call_info.is_ok());

    let ret_address = Address(
        Felt252::from_dec_str(
            "2981367321579044137695643605491580626686793431687828656373743652416610344312",
        )
        .unwrap(),
    );

    let ret_class_hash = state.get_class_hash_at(&ret_address).unwrap();
    let ret_casm_class = match state.get_contract_class(&ret_class_hash).unwrap() {
        CompiledClass::Deprecated(class) => class.as_ref().clone(),
        CompiledClass::Casm { .. } => unreachable!(),
    };

    assert_eq!(ret_casm_class, test_contract_class);
}

#[test]
fn deploy_cairo0_and_invoke() {
    // data to deploy
    let test_class_hash: ClassHash = ClassHash([2; 32]);
    let test_felt_hash = Felt252::from_bytes_be(&test_class_hash.0);
    let salt = Felt252::ZERO;
    let contract_path = "starknet_programs/factorial.json";
    let test_contract_class: ContractClass = ContractClass::from_path(contract_path).unwrap();

    // Create the deploy contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/deploy_without_constructor.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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
    contract_class_cache.set_contract_class(
        test_class_hash,
        CompiledClass::Deprecated(Arc::new(test_contract_class.clone())),
    );

    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let mut state: CachedState<_, _> =
        CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // arguments of deploy contract
    let calldata: Vec<_> = [test_felt_hash, salt].to_vec();

    // set up remaining structures

    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address.clone(),
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        100_000_000,
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

    let call_info = exec_entry_point.execute(
        &mut state,
        &block_context,
        &mut resources_manager,
        &mut tx_execution_context,
        false,
        block_context.invoke_tx_max_n_steps(),
        #[cfg(feature = "cairo-native")]
        None,
    );

    assert!(call_info.is_ok());

    let ret_address = Address(
        Felt252::from_dec_str(
            "3326516449409112130211257005742850249535379011750934837578774621442000311202",
        )
        .unwrap(),
    );

    let ret_class_hash = state.get_class_hash_at(&ret_address).unwrap();
    let ret_casm_class = match state.get_contract_class(&ret_class_hash).unwrap() {
        CompiledClass::Deprecated(class) => class.as_ref().clone(),
        CompiledClass::Casm { .. } => unreachable!(),
    };

    assert_eq!(ret_casm_class, test_contract_class);

    // invoke factorial

    let calldata = [3.into()].to_vec();
    let selector = Felt252::from_dec_str(
        "1554360238305724106620514039016755337737024783182305317707426109255385571750",
    )
    .unwrap();

    let exec_entry_point = ExecutionEntryPoint::new(
        ret_address,
        calldata,
        selector,
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(test_class_hash),
        100_000_000,
    );

    let call_info = exec_entry_point
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
        .unwrap();

    let retdata = call_info.call_info.unwrap().retdata;

    // expected result 3! = 6
    assert_eq!(retdata, [6.into()].to_vec());
}

#[test]
fn test_send_message_to_l1_syscall() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/send_message_to_l1.casm");

    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let external_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // RUN SEND_MSG
    // Create an execution entry point
    let send_message_exec_entry_point = create_execute_extrypoint(
        address.clone(),
        class_hash,
        external_entrypoint_selector,
        vec![],
        EntryPointType::External,
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

    // Run send_msg entrypoint
    let call_info = send_message_exec_entry_point
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
        .unwrap();

    let l2_to_l1_messages = vec![OrderedL2ToL1Message {
        order: 0,
        to_address: Address(444.into()),
        payload: vec![555.into(), 666.into()],
    }];

    let expected_n_steps = 69;
    let expected_gas_consumed = 12040;
    let expected_n_memory_holes = 1;

    let expected_execution_resources = ExecutionResources {
        n_steps: expected_n_steps,
        n_memory_holes: expected_n_memory_holes,
        builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 2)]),
    };

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: address,
        class_hash: Some(class_hash),
        entry_point_selector: Some(Felt252::from(external_entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        l2_to_l1_messages,
        execution_resources: Some(expected_execution_resources),
        gas_consumed: expected_gas_consumed,
        ..Default::default()
    };

    assert_eq!(call_info.call_info.unwrap(), expected_call_info);
}

#[test]
fn test_get_execution_info() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_execution_info.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let external_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    let block_context = BlockContext::default();
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::ZERO,
        vec![22.into(), 33.into()],
        Default::default(),
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        *TRANSACTION_VERSION,
    );

    let mut resources_manager = ExecutionResourcesManager::default();

    // RUN GET_INFO
    // Create an execution entry point
    let get_info_exec_entry_point = create_execute_extrypoint(
        address.clone(),
        class_hash,
        external_entrypoint_selector,
        vec![],
        EntryPointType::External,
    );

    // Run send_msg entrypoint
    let call_info = get_info_exec_entry_point
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
        .unwrap();

    let expected_ret_data = vec![
        block_context.block_info().sequencer_address.0,
        0.into(),
        0.into(),
        address.0,
    ];

    let expected_n_steps = 202;
    let expected_gas_consumed = 21880;

    let expected_execution_resources = ExecutionResources {
        n_steps: expected_n_steps,
        n_memory_holes: 4,
        builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 4)]),
    };

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: address,
        class_hash: Some(class_hash),
        entry_point_selector: Some(Felt252::from(external_entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        retdata: expected_ret_data,
        execution_resources: Some(expected_execution_resources),
        gas_consumed: expected_gas_consumed,
        ..Default::default()
    };

    assert_eq!(call_info.call_info.unwrap(), expected_call_info);
}

#[test]
fn replace_class_internal() {
    // This test only checks that the contract is updated in the storage, see `replace_class_contract_call`
    //  Create program and entry point types for contract class
    let program_data_a = include_bytes!("../../starknet_programs/cairo2/get_number_a.casm");
    let contract_class_a: CasmContractClass = serde_json::from_slice(program_data_a).unwrap();
    let entrypoints_a = contract_class_a.clone().entry_points_by_type;
    let upgrade_selector = &entrypoints_a.external.first().unwrap().selector;

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(1111.into());
    let class_hash_a: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash_a,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_a),
            sierra: None,
        },
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_a);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Add get_number_b contract to the state (only its contract_class)
    let program_data_b = include_bytes!("../../starknet_programs/cairo2/get_number_b.casm");
    let contract_class_b: CasmContractClass = serde_json::from_slice(program_data_b).unwrap();

    let class_hash_b: ClassHash = ClassHash([2; 32]);

    contract_class_cache.set_contract_class(
        class_hash_b,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_b.clone()),
            sierra: None,
        },
    );

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Run upgrade entrypoint and check that the storage was updated with the new contract class
    // Create an execution entry point
    let calldata = [Felt252::from_bytes_be(&class_hash_b.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata,
        Felt252::from(upgrade_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash_a),
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
        .unwrap();
    // Check that the class was indeed replaced in storage
    assert_eq!(state.get_class_hash_at(&address).unwrap(), class_hash_b);
    // Check that the class_hash_b leads to contract_class_b for soundness
    assert_eq!(
        state.get_contract_class(&class_hash_b).unwrap(),
        CompiledClass::Casm {
            casm: Arc::new(contract_class_b),
            sierra: None
        }
    );
}

#[test]
fn replace_class_contract_call() {
    /* Test Outline:
       - Add `get_number_a.cairo` contract at address 2 and `get_number_b.cairo` contract without an address
       - Call `get_number` function of `get_number_wrapper.cairo` and expect to get an answer from `get_number_a` (25)
       - Call `upgrade` function of `get_number_wrapper.cairo` with `get_number_b.cairo`'s class_hash
       - Call `get_number` function of `get_number_wrapper.cairo` and expect to get an answer from `get_number_b` (17)
    */

    // SET GET_NUMBER_A
    // Add get_number_a.cairo to storage
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_a.casm");
    let contract_class_a: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(Felt252::ONE);
    let class_hash_a: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash_a,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_a),
            sierra: None,
        },
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_a);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // SET GET_NUMBER_B

    // Add get_number_b contract to the state (only its contract_class)

    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_b.casm");
    let contract_class_b: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let class_hash_b: ClassHash = ClassHash([2; 32]);

    contract_class_cache.set_contract_class(
        class_hash_b,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_b),
            sierra: None,
        },
    );

    // SET GET_NUMBER_WRAPPER

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_wrapper.casm");
    let wrapper_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = wrapper_contract_class.clone().entry_points_by_type;
    let get_number_entrypoint_selector = &entrypoints.external.get(1).unwrap().selector;
    let upgrade_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

    let wrapper_address = Address(Felt252::from(2));
    let wrapper_class_hash: ClassHash = ClassHash([3; 32]);

    contract_class_cache.set_contract_class(
        wrapper_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(wrapper_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address.clone(), wrapper_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(wrapper_address, nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // INITIALIZE STARKNET CONFIG
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

    // CALL GET_NUMBER BEFORE REPLACE_CLASS

    let calldata = [].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata,
        Felt252::from(get_number_entrypoint_selector),
        caller_address.clone(),
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        100000,
    );

    let result = exec_entry_point
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
        .unwrap();
    assert_eq!(result.call_info.unwrap().retdata, vec![25.into()]);

    // REPLACE_CLASS

    let calldata = [Felt252::from_bytes_be(&class_hash_b.0)].to_vec();

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata,
        Felt252::from(upgrade_entrypoint_selector),
        caller_address.clone(),
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        100000,
    );

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
        .unwrap();

    // CALL GET_NUMBER AFTER REPLACE_CLASS

    let calldata = [].to_vec();

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(get_number_entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        100000,
    );

    let result = exec_entry_point
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
        .unwrap();
    assert_eq!(result.call_info.clone().unwrap().retdata, vec![17.into()]);
    assert_eq!(result.call_info.unwrap().failure_flag, false);
}

#[test]
fn replace_class_contract_call_same_transaction() {
    /* Test Outline:
       - Add `get_number_a.cairo` contract at address 2 and `get_number_b.cairo` contract without an address
       - Call `get_numbers_old_new` function of `get_number_wrapper.cairo` and expect to get both answers from `get_number_a`, and 'get_number_b' (25, 17)
    */

    // SET GET_NUMBER_A
    // Add get_number_a.cairo to storage
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_a.casm");
    let contract_class_a: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(Felt252::ONE);
    let class_hash_a: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash_a,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_a),
            sierra: None,
        },
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_a);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // SET GET_NUMBER_B

    // Add get_number_b contract to the state (only its contract_class)
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_b.casm");
    let contract_class_b: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let class_hash_b: ClassHash = ClassHash([2; 32]);

    contract_class_cache.set_contract_class(
        class_hash_b,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_b),
            sierra: None,
        },
    );

    // SET GET_NUMBER_WRAPPER

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_wrapper.casm");
    let wrapper_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = wrapper_contract_class.clone().entry_points_by_type;
    let get_numbers_entrypoint_selector = &entrypoints.external.get(2).unwrap().selector;

    let wrapper_address = Address(Felt252::from(2));
    let wrapper_class_hash: ClassHash = ClassHash([3; 32]);

    contract_class_cache.set_contract_class(
        wrapper_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(wrapper_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address.clone(), wrapper_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(wrapper_address, nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // INITIALIZE STARKNET CONFIG
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

    // CALL GET_NUMBERS_OLD_NEW

    let calldata = [Felt252::from_bytes_be(&class_hash_b.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(get_numbers_entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        u64::MAX.into(),
    );

    let result = exec_entry_point
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
        .unwrap();
    assert_eq!(
        result.call_info.unwrap().retdata,
        vec![25.into(), 17.into()]
    );
}

#[test]
fn call_contract_upgrade_cairo_0_to_cairo_1_same_transaction() {
    /* Test Outline:
       - Add `get_number_c.cairo` contract at address 2 and `get_number_b.cairo` contract without an address
       - Call `get_numbers_old_new` function of `get_number_wrapper.cairo` and expect to get both answers from `get_number_c`, and 'get_number_b' (33, 17)
    */

    // SET GET_NUMBER_C

    // Add get_number_a.cairo to storage

    let contract_class_c = ContractClass::from_path("starknet_programs/get_number_c.json").unwrap();

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(Felt252::ONE);
    let class_hash_c: ClassHash = ClassHash::from(Felt252::ONE);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash_c,
        CompiledClass::Deprecated(Arc::new(contract_class_c)),
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_c);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // SET GET_NUMBER_B

    // Add get_number_b contract to the state (only its contract_class)
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_b.casm");
    let contract_class_b: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let class_hash_b: ClassHash = ClassHash::from(Felt252::from(2));

    contract_class_cache.set_contract_class(
        class_hash_b,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_b),
            sierra: None,
        },
    );

    // SET GET_NUMBER_WRAPPER

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_wrapper.casm");
    let wrapper_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = wrapper_contract_class.clone().entry_points_by_type;
    let get_numbers_entrypoint_selector = &entrypoints.external.get(2).unwrap().selector;

    let wrapper_address = Address(Felt252::from(2));
    let wrapper_class_hash: ClassHash = ClassHash([3; 32]);

    contract_class_cache.set_contract_class(
        wrapper_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(wrapper_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address.clone(), wrapper_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(wrapper_address, nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // INITIALIZE STARKNET CONFIG
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

    // CALL GET_NUMBERS_OLD_NEW

    let calldata = [Felt252::from_bytes_be(&class_hash_b.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(get_numbers_entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        u64::MAX.into(),
    );

    let result = exec_entry_point
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
        .unwrap();
    assert_eq!(
        result.call_info.unwrap().retdata,
        vec![33.into(), 17.into()]
    );
}

#[test]
fn call_contract_downgrade_cairo_1_to_cairo_0_same_transaction() {
    /* Test Outline:
       - Add `get_number_b.cairo` contract at address 2 and `get_number_c.cairo` contract without an address
       - Call `get_numbers_old_new` function of `get_number_wrapper.cairo` and expect to get both answers from `get_number_b`, and 'get_number_c' (17, 33)
    */

    // SET GET_NUMBER_C
    // Add get_number_a.cairo to the state (only its contract_class)
    let contract_class_c = ContractClass::from_path("starknet_programs/get_number_c.json").unwrap();

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(Felt252::ONE);
    let class_hash_c: ClassHash = ClassHash::from(Felt252::ONE);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash_c,
        CompiledClass::Deprecated(Arc::new(contract_class_c)),
    );

    // SET GET_NUMBER_B

    // Add get_number_b contract to the state
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_b.casm");
    let contract_class_b: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let class_hash_b: ClassHash = ClassHash::from(Felt252::from(2));

    contract_class_cache.set_contract_class(
        class_hash_b,
        CompiledClass::Casm {
            casm: Arc::new(contract_class_b),
            sierra: None,
        },
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_b);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // SET GET_NUMBER_WRAPPER

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_wrapper.casm");
    let wrapper_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = wrapper_contract_class.clone().entry_points_by_type;
    let get_numbers_entrypoint_selector = &entrypoints.external.get(2).unwrap().selector;

    let wrapper_address = Address(Felt252::from(2));
    let wrapper_class_hash: ClassHash = ClassHash([3; 32]);

    contract_class_cache.set_contract_class(
        wrapper_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(wrapper_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address.clone(), wrapper_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(wrapper_address, nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // INITIALIZE STARKNET CONFIG
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

    // CALL GET_NUMBERS_OLD_NEW

    let calldata = [Felt252::from_bytes_be(&class_hash_c.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(get_numbers_entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        u64::MAX.into(),
    );

    let result = exec_entry_point
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
        .unwrap();
    assert_eq!(
        result.call_info.unwrap().retdata,
        vec![17.into(), 33.into()]
    );
}

#[test]
fn call_contract_replace_class_cairo_0() {
    /* Test Outline:
       - Add `get_number_d.cairo` contract at address 2 and `get_number_c.cairo` contract without an address
       - Call `get_numbers_old_new` function of `get_number_wrapper.cairo` and expect to get both answers from `get_number_d`, and 'get_number_c' (64, 33)
    */

    // SET GET_NUMBER_C
    // Add get_number_a.cairo to the state (only its contract_class)
    let contract_class_c = ContractClass::from_path("starknet_programs/get_number_c.json").unwrap();

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(Felt252::ONE);
    let class_hash_c: ClassHash = ClassHash::from(Felt252::ONE);
    let nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        class_hash_c,
        CompiledClass::Deprecated(Arc::new(contract_class_c)),
    );

    // SET GET_NUMBER_B

    // Add get_number_b contract to the state

    let contract_class_d = ContractClass::from_path("starknet_programs/get_number_d.json").unwrap();

    let class_hash_d: ClassHash = ClassHash::from(Felt252::from(2));

    contract_class_cache.set_contract_class(
        class_hash_d,
        CompiledClass::Deprecated(Arc::new(contract_class_d)),
    );
    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash_d);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // SET GET_NUMBER_WRAPPER

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_number_wrapper.casm");
    let wrapper_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = wrapper_contract_class.clone().entry_points_by_type;
    let get_numbers_entrypoint_selector = &entrypoints.external.get(2).unwrap().selector;

    let wrapper_address = Address(Felt252::from(2));
    let wrapper_class_hash: ClassHash = ClassHash([3; 32]);

    contract_class_cache.set_contract_class(
        wrapper_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(wrapper_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(wrapper_address.clone(), wrapper_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(wrapper_address, nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // INITIALIZE STARKNET CONFIG
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

    // CALL GET_NUMBERS_OLD_NEW

    let calldata = [Felt252::from_bytes_be(&class_hash_c.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(get_numbers_entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(wrapper_class_hash),
        u64::MAX.into(),
    );

    let result = exec_entry_point
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
        .unwrap();
    assert_eq!(
        result.call_info.unwrap().retdata,
        vec![64.into(), 33.into()]
    );
}

#[test]
fn test_out_of_gas_failure() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/emit_event.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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
    let calldata = [].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    // Purposefully set initial gas to 0 so that the syscall fails
    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        0,
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
    let call_info = exec_entry_point
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
        .unwrap();
    let call_info = call_info.call_info.unwrap();
    assert_eq!(
        call_info.retdata,
        vec![Felt252::from_bytes_be_slice("Out of gas".as_bytes())]
    );
    assert!(call_info.failure_flag)
}

#[test]
fn deploy_syscall_failure_uninitialized_class_hash() {
    //  Create program and entry point types for contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/deploy_contract_no_args.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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
    let calldata = [Felt252::ZERO].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
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
    let call_info = exec_entry_point
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
        .unwrap();
    assert_eq!(
        std::str::from_utf8(&call_info.call_info.unwrap().retdata[0].to_bytes_be())
            .unwrap()
            .trim_start_matches('\0'),
        "CLASS_HASH_NOT_FOUND"
    )
}

#[test]
fn deploy_syscall_failure_in_constructor() {
    //  Create program and entry point types for contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/deploy_contract_no_args.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // Add failing constructor contract
    let f_c_program_data =
        include_bytes!("../../starknet_programs/cairo2/failing_constructor.casm");
    let f_c_contract_class: CasmContractClass = serde_json::from_slice(f_c_program_data).unwrap();
    let f_c_class_hash = Felt252::ONE;
    contract_class_cache.set_contract_class(
        ClassHash::from(f_c_class_hash),
        CompiledClass::Casm {
            casm: Arc::new(f_c_contract_class),
            sierra: None,
        },
    );

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [f_c_class_hash].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
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
    let call_info = exec_entry_point
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
        .unwrap();
    // Check that we get the error from the constructor
    // assert( 1 == 0 , 'Oops');
    assert_eq!(
        std::str::from_utf8(&call_info.call_info.unwrap().retdata[0].to_bytes_be())
            .unwrap()
            .trim_start_matches('\0'),
        "Oops"
    )
}

#[test]
fn storage_read_no_value() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/simple_wallet.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let get_balance_entrypoint_selector = &entrypoints.external.get(1).unwrap().selector;

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

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        address,
        class_hash,
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    // As the value doesn't exist in storage, it's value will be 0
    assert_eq!(call_info.call_info.unwrap().retdata, [0.into()]);
}

#[test]
fn storage_read_unavailable_address_domain() {
    //  Create program and entry point types for contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/faulty_low_level_storage_read.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let read_storage_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // RUN READ_STORAGE
    // Create an execution entry point
    let calldata = [].to_vec();
    let read_storage_exec_entry_point = create_execute_extrypoint(
        address,
        class_hash,
        read_storage_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run read_storage entrypoint
    let call_info = read_storage_exec_entry_point
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
        .unwrap();

    assert_eq!(
        call_info.call_info.unwrap().retdata[0],
        Felt252::from_bytes_be_slice(b"Unsupported address domain")
    );
}

#[test]
fn storage_write_unavailable_address_domain() {
    //  Create program and entry point types for contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/faulty_low_level_storage_write.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let read_storage_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // RUN READ_STORAGE
    // Create an execution entry point
    let calldata = [].to_vec();
    let read_storage_exec_entry_point = create_execute_extrypoint(
        address,
        class_hash,
        read_storage_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run read_storage entrypoint
    let call_info = read_storage_exec_entry_point
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
        .unwrap();

    assert_eq!(
        call_info.call_info.unwrap().retdata[0],
        Felt252::from_bytes_be_slice(b"Unsupported address domain")
    );
}

#[test]
fn library_call_failure() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/square_root.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // Add lib contract to the state

    let lib_program_data = include_bytes!("../../starknet_programs/cairo2/faulty_math_lib.casm");
    let lib_contract_class: CasmContractClass = serde_json::from_slice(lib_program_data).unwrap();

    let lib_address = Address(1112.into());
    let lib_class_hash: ClassHash = ClassHash([2; 32]);
    let lib_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        lib_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(lib_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(lib_address.clone(), lib_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(lib_address, lib_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [25.into(), Felt252::from_bytes_be(&lib_class_hash.0)].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
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
    let mut expected_execution_resources = ExecutionResources::default();
    expected_execution_resources
        .builtin_instance_counter
        .insert(RANGE_CHECK_BUILTIN_NAME.to_string(), 7);
    expected_execution_resources.n_memory_holes = 6;

    let call_info = exec_entry_point
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
        .unwrap();

    let call_info = call_info.call_info.unwrap();

    assert_eq!(
        std::str::from_utf8(&call_info.retdata[0].to_bytes_be())
            .unwrap()
            .trim_start_matches('\0'),
        "Unimplemented"
    );
    assert!(call_info.failure_flag);
}

#[test]
fn send_messages_to_l1_different_contract_calls() {
    //  Create program and entry point types for contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/send_messages_contract_call.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoint_selector = &contract_class.entry_points_by_type.external[0]
        .selector
        .clone();

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

    // Add send_message_to_l1 contract to the state

    let program_data =
        include_bytes!("../../starknet_programs/cairo2/send_simple_message_to_l1.casm");
    let send_msg_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let send_msg_address = Address(1.into()); //Hardcoded in contract
    let send_msg_class_hash: ClassHash = ClassHash([2; 32]);
    let send_msg_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        send_msg_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(send_msg_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(send_msg_address.clone(), send_msg_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(send_msg_address.clone(), send_msg_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [25.into(), 50.into(), 75.into()].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        1000000,
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

    let call_info = exec_entry_point
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
        .unwrap();
    let l1_to_l2_messages = call_info
        .call_info
        .unwrap()
        .get_sorted_l2_to_l1_messages()
        .unwrap();

    assert_eq!(
        l1_to_l2_messages,
        vec![
            L2toL1MessageInfo::new(
                OrderedL2ToL1Message {
                    order: 0,
                    to_address: Address(25.into()),
                    payload: vec![50.into()]
                },
                send_msg_address.clone()
            ),
            L2toL1MessageInfo::new(
                OrderedL2ToL1Message {
                    order: 1,
                    to_address: Address(25.into()),
                    payload: vec![75.into()]
                },
                send_msg_address
            )
        ],
    )
}

#[test]
fn send_messages_to_l1_different_contract_calls_cairo1_to_cairo0() {
    //  Create program and entry point types for contract class
    let program_data =
        include_bytes!("../../starknet_programs/cairo2/send_messages_contract_call.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoint_selector = &contract_class.entry_points_by_type.external[0]
        .selector
        .clone();

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

    // Add send_message_to_l1 contract to the state

    let send_msg_contract_class =
        ContractClass::from_path("starknet_programs/send_message_to_l1.json").unwrap();

    let send_msg_address = Address(1.into()); //Hardcoded in contract
    let send_msg_class_hash: ClassHash = ClassHash([2; 32]);
    let send_msg_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        send_msg_class_hash,
        CompiledClass::Deprecated(Arc::new(send_msg_contract_class)),
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(send_msg_address.clone(), send_msg_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(send_msg_address.clone(), send_msg_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [25.into(), 50.into(), 75.into()].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        1000000,
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

    let call_info = exec_entry_point
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
        .unwrap();
    let l1_to_l2_messages = call_info
        .call_info
        .unwrap()
        .get_sorted_l2_to_l1_messages()
        .unwrap();
    assert_eq!(
        l1_to_l2_messages,
        vec![
            L2toL1MessageInfo::new(
                OrderedL2ToL1Message {
                    order: 0,
                    to_address: Address(25.into()),
                    payload: vec![50.into()]
                },
                send_msg_address.clone()
            ),
            L2toL1MessageInfo::new(
                OrderedL2ToL1Message {
                    order: 1,
                    to_address: Address(25.into()),
                    payload: vec![75.into()]
                },
                send_msg_address
            )
        ],
    )
}

#[test]
fn send_messages_to_l1_different_contract_calls_cairo0_to_cairo1() {
    //  Create program and entry point types for contract class
    let contract_class =
        ContractClass::from_path("starknet_programs/send_messages_contract_call.json").unwrap();
    let entrypoint_selector = &contract_class.entry_points_by_type()[&EntryPointType::External][0]
        .selector()
        .to_owned();

    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();

    let address = Address(1111.into());
    let class_hash: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

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

    // Add send_message_to_l1 contract to the state

    let program_data =
        include_bytes!("../../starknet_programs/cairo2/send_simple_message_to_l1.casm");
    let send_msg_contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    let send_msg_address = Address(1.into()); //Hardcoded in contract
    let send_msg_class_hash: ClassHash = ClassHash([2; 32]);
    let send_msg_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        send_msg_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(send_msg_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(send_msg_address.clone(), send_msg_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(send_msg_address.clone(), send_msg_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [25.into(), 50.into(), 75.into()].to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address.clone(),
        calldata,
        *entrypoint_selector,
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        1000000,
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

    let call_info = exec_entry_point
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
        .unwrap();
    let l1_to_l2_messages = call_info
        .call_info
        .unwrap()
        .get_sorted_l2_to_l1_messages()
        .unwrap();
    assert_eq!(
        l1_to_l2_messages,
        vec![
            L2toL1MessageInfo::new(
                OrderedL2ToL1Message {
                    order: 0,
                    to_address: Address(25.into()),
                    payload: vec![50.into()]
                },
                send_msg_address.clone()
            ),
            L2toL1MessageInfo::new(
                OrderedL2ToL1Message {
                    order: 1,
                    to_address: Address(25.into()),
                    payload: vec![75.into()]
                },
                send_msg_address
            )
        ],
    )
}

#[test]
fn keccak_syscall() {
    let program_data = include_bytes!("../../starknet_programs/cairo2/test_cairo_keccak.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let read_storage_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // RUN READ_STORAGE
    // Create an execution entry point
    let calldata = [].to_vec();
    let read_storage_exec_entry_point = create_execute_extrypoint(
        address,
        class_hash,
        read_storage_entrypoint_selector,
        calldata,
        EntryPointType::External,
    );

    // Run read_storage entrypoint
    let call_info = read_storage_exec_entry_point
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
        .unwrap();

    let call_info = call_info.call_info.unwrap();

    assert_eq!(call_info.retdata[0], Felt252::ONE);
    assert_eq!(call_info.gas_consumed, 509590);
}

#[test]
fn library_call_recursive_50_calls() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/square_root_recursive.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    // Add lib contract to the state

    let lib_program_data = include_bytes!("../../starknet_programs/cairo2/math_lib.casm");
    let lib_contract_class: CasmContractClass = serde_json::from_slice(lib_program_data).unwrap();

    let lib_address = Address(1112.into());
    let lib_class_hash: ClassHash = ClassHash([2; 32]);
    let lib_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        lib_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(lib_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(lib_address.clone(), lib_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(lib_address, lib_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    // Create an execution entry point
    let calldata = [
        Felt252::from_dec_str("1125899906842624").unwrap(),
        Felt252::from_bytes_be(&lib_class_hash.0),
        Felt252::from(50),
    ]
    .to_vec();
    let caller_address = Address(0000.into());
    let entry_point_type = EntryPointType::External;

    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        Felt252::from(entrypoint_selector),
        caller_address,
        entry_point_type,
        Some(CallType::Delegate),
        Some(class_hash),
        u128::MAX,
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
    let expected_execution_resources_internal_call = ExecutionResources {
        n_steps: 78,
        n_memory_holes: 5,
        builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 7)]),
    };

    let call_info = exec_entry_point
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

    assert_eq!(call_info.internal_calls.len(), 50);
    assert_eq!(
        call_info.internal_calls[0],
        CallInfo {
            caller_address: Address(0.into()),
            call_type: Some(CallType::Delegate),
            contract_address: Address(1111.into()),
            entry_point_selector: Some(
                Felt252::from_dec_str(
                    "544923964202674311881044083303061611121949089655923191939299897061511784662"
                )
                .unwrap(),
            ),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![Felt252::from_dec_str("1125899906842624").unwrap()],
            retdata: [Felt252::from_dec_str("33554432").unwrap()].to_vec(),
            execution_resources: Some(expected_execution_resources_internal_call),
            class_hash: Some(lib_class_hash),
            gas_consumed: 0,
            ..Default::default()
        }
    );
    assert_eq!(call_info.retdata, [1.into()].to_vec());
    assert!(!call_info.failure_flag);
}

#[test]
fn call_contract_storage_write_read_recursive_50_calls() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/wallet_wrapper.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let get_balance_entrypoint_selector =
        &BigUint::from_bytes_be(&calculate_sn_keccak("get_balance".as_bytes()));
    let increase_balance_entrypoint_selector = &BigUint::from_bytes_be(&calculate_sn_keccak(
        "increase_balance_recursive".as_bytes(),
    ));

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

    // Add simple_wallet contract to the state
    let simple_wallet_program_data =
        include_bytes!("../../starknet_programs/cairo2/simple_wallet.casm");
    let simple_wallet_contract_class: CasmContractClass =
        serde_json::from_slice(simple_wallet_program_data).unwrap();
    let simple_wallet_constructor_entrypoint_selector = simple_wallet_contract_class
        .entry_points_by_type
        .constructor
        .first()
        .unwrap()
        .selector
        .clone();

    let simple_wallet_address = Address(1112.into());
    let simple_wallet_class_hash: ClassHash = ClassHash([2; 32]);
    let simple_wallet_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        simple_wallet_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(simple_wallet_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(simple_wallet_address.clone(), simple_wallet_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(simple_wallet_address.clone(), simple_wallet_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

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

    let create_execute_extrypoint = |selector: &BigUint,
                                     calldata: Vec<Felt252>,
                                     entry_point_type: EntryPointType,
                                     class_hash: [u8; 32],
                                     address: Address|
     -> ExecutionEntryPoint {
        ExecutionEntryPoint::new(
            address,
            calldata,
            Felt252::from(selector),
            Address(0000.into()),
            entry_point_type,
            Some(CallType::Delegate),
            Some(ClassHash(class_hash)),
            u64::MAX.into(),
        )
    };

    // RUN SIMPLE_WALLET CONSTRUCTOR
    // Create an execution entry point
    let calldata = [25.into()].to_vec();
    let constructor_exec_entry_point = create_execute_extrypoint(
        &simple_wallet_constructor_entrypoint_selector,
        calldata,
        EntryPointType::Constructor,
        simple_wallet_class_hash.0,
        simple_wallet_address.clone(),
    );

    // Run constructor entrypoint
    constructor_exec_entry_point
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
        .unwrap();

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [simple_wallet_address.0].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address.clone(),
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [25.into()]);

    // RUN INCREASE_BALANCE
    // Create an execution entry point
    let calldata = [50.into(), simple_wallet_address.0].to_vec();
    let increase_balance_entry_point = create_execute_extrypoint(
        increase_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address.clone(),
    );

    // Run increase_balance entrypoint
    let call_info = increase_balance_entry_point
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
    // Check that the recursive function did in fact call the simple_wallet contract 50 times
    assert_eq!(call_info.internal_calls.len(), 50);
    assert!(!call_info.failure_flag);

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [simple_wallet_address.0].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address,
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [75.into()])
}

#[test]
fn call_contract_storage_write_read_recursive_100_calls() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/wallet_wrapper.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let get_balance_entrypoint_selector =
        &BigUint::from_bytes_be(&calculate_sn_keccak("get_balance".as_bytes()));
    let increase_balance_entrypoint_selector = &BigUint::from_bytes_be(&calculate_sn_keccak(
        "increase_balance_recursive".as_bytes(),
    ));

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

    // Add simple_wallet contract to the state
    let simple_wallet_program_data =
        include_bytes!("../../starknet_programs/cairo2/simple_wallet.casm");
    let simple_wallet_contract_class: CasmContractClass =
        serde_json::from_slice(simple_wallet_program_data).unwrap();
    let simple_wallet_constructor_entrypoint_selector = simple_wallet_contract_class
        .entry_points_by_type
        .constructor
        .first()
        .unwrap()
        .selector
        .clone();

    let simple_wallet_address = Address(1112.into());
    let simple_wallet_class_hash: ClassHash = ClassHash([2; 32]);
    let simple_wallet_nonce = Felt252::ZERO;

    contract_class_cache.set_contract_class(
        simple_wallet_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(simple_wallet_contract_class),
            sierra: None,
        },
    );
    state_reader
        .address_to_class_hash_mut()
        .insert(simple_wallet_address.clone(), simple_wallet_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(simple_wallet_address.clone(), simple_wallet_nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

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

    let create_execute_extrypoint = |selector: &BigUint,
                                     calldata: Vec<Felt252>,
                                     entry_point_type: EntryPointType,
                                     class_hash: [u8; 32],
                                     address: Address|
     -> ExecutionEntryPoint {
        ExecutionEntryPoint::new(
            address,
            calldata,
            Felt252::from(selector),
            Address(0000.into()),
            entry_point_type,
            Some(CallType::Delegate),
            Some(ClassHash(class_hash)),
            u64::MAX.into(),
        )
    };

    // RUN SIMPLE_WALLET CONSTRUCTOR
    // Create an execution entry point
    let calldata = [25.into()].to_vec();
    let constructor_exec_entry_point = create_execute_extrypoint(
        &simple_wallet_constructor_entrypoint_selector,
        calldata,
        EntryPointType::Constructor,
        simple_wallet_class_hash.0,
        simple_wallet_address.clone(),
    );

    // Run constructor entrypoint
    constructor_exec_entry_point
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
        .unwrap();

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [simple_wallet_address.0].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address.clone(),
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [25.into()]);

    // RUN INCREASE_BALANCE
    // Create an execution entry point
    let calldata = [100.into(), simple_wallet_address.0].to_vec();
    let increase_balance_entry_point = create_execute_extrypoint(
        increase_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address.clone(),
    );

    // Run increase_balance entrypoint
    let call_info = increase_balance_entry_point
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
    // Check that the recursive function did in fact call the simple_wallet contract 50 times
    assert_eq!(call_info.internal_calls.len(), 100);
    assert!(!call_info.failure_flag);

    // RUN GET_BALANCE
    // Create an execution entry point
    let calldata = [simple_wallet_address.0].to_vec();
    let get_balance_exec_entry_point = create_execute_extrypoint(
        get_balance_entrypoint_selector,
        calldata,
        EntryPointType::External,
        class_hash.0,
        address,
    );

    // Run get_balance entrypoint
    let call_info = get_balance_exec_entry_point
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
        .unwrap();
    assert_eq!(call_info.call_info.unwrap().retdata, [125.into()])
}

#[test]
fn test_get_execution_info_v2() {
    //  Create program and entry point types for contract class
    use starknet_in_rust::transaction::{
        CurrentAccountTxFields, DataAvailabilityMode, ResourceBounds,
        VersionSpecificAccountTxFields,
    };
    let program_data = include_bytes!("../../starknet_programs/cairo2/get_execution_info_v2.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let external_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

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

    let account_tx_fields = VersionSpecificAccountTxFields::Current(CurrentAccountTxFields {
        l1_resource_bounds: ResourceBounds {
            max_amount: 30,
            max_price_per_unit: 15,
        },
        l2_resource_bounds: Some(ResourceBounds {
            max_amount: 10,
            max_price_per_unit: 5,
        }),
        tip: 3,
        nonce_data_availability_mode: DataAvailabilityMode::L1,
        fee_data_availability_mode: DataAvailabilityMode::L2,
        paymaster_data: vec![6.into(), 17.into()],
        account_deployment_data: vec![7.into(), 18.into()],
    });

    let block_context = BlockContext::default();
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::ZERO,
        vec![22.into(), 33.into()],
        account_tx_fields,
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        *TRANSACTION_VERSION,
    );

    let mut resources_manager = ExecutionResourcesManager::default();

    // RUN GET_INFO
    // Create an execution entry point
    let get_info_exec_entry_point = create_execute_extrypoint(
        address.clone(),
        class_hash,
        external_entrypoint_selector,
        vec![],
        EntryPointType::External,
    );

    // Run send_msg entrypoint
    let call_info = get_info_exec_entry_point
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
        .unwrap();

    let expected_ret_data = vec![
        block_context.block_info().sequencer_address.0,
        0.into(),
        0.into(),
        address.0,
    ];

    let expected_n_steps = 438;

    let expected_gas_consumed = 48180;

    let expected_execution_resources = ExecutionResources {
        n_steps: expected_n_steps,
        n_memory_holes: 14,
        builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 14)]),
    };

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: address,
        class_hash: Some(class_hash),
        entry_point_selector: Some(Felt252::from(external_entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        retdata: expected_ret_data,
        execution_resources: Some(expected_execution_resources),
        gas_consumed: expected_gas_consumed,
        ..Default::default()
    };

    assert_eq!(call_info.call_info.unwrap(), expected_call_info);
}
