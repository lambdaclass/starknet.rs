use crate::integration_tests::complex_contracts::utils::*;
use cairo_vm::{
    types::builtin_name::BuiltinName, vm::runners::cairo_runner::ExecutionResources, Felt252,
};

use starknet_crypto::FieldElement;
use starknet_in_rust::{
    definitions::block_context::BlockContext,
    execution::{CallInfo, CallType},
    services::api::contract_classes::deprecated_contract_class::ContractClass,
    state::{
        cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, state_api::StateReader,
        ExecutionResourcesManager,
    },
    transaction::Address,
    utils::calculate_sn_keccak,
    EntryPointType,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[test]
fn amm_proxy_init_pool_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, contract_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    // Deploy proxy
    let (proxy_address, proxy_class_hash_bytes) = deploy(
        &mut state,
        "starknet_programs/amm_proxy.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let proxy_class_hash = proxy_class_hash_bytes;
    let proxy_entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&proxy_class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let calldata = [contract_address.0, 555.into(), 666.into()].to_vec();
    let caller_address = Address(1000000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &proxy_address,
        class_hash: &proxy_class_hash,
        entry_points_by_type: &proxy_entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    let result = execute_entry_point("proxy_init_pool", &calldata, &mut call_config).unwrap();
    let amm_proxy_entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"proxy_init_pool"));
    let amm_entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"init_pool"));

    let accessed_storage_keys =
        get_accessed_keys("pool_balance", vec![vec![1_u8.into()], vec![2_u8.into()]]);

    let internal_calls = vec![CallInfo {
        caller_address: proxy_address.clone(),
        call_type: Some(CallType::Call),
        contract_address,
        entry_point_selector: Some(amm_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone()[1..].to_vec(),
        retdata: [].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 232,
            n_memory_holes: 20,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 2),
                (BuiltinName::range_check, 14),
            ]),
        }),
        class_hash: Some(contract_class_hash),
        accessed_storage_keys,
        storage_read_values: vec![Felt252::ZERO, Felt252::ZERO],
        ..Default::default()
    }];

    let expected_call_info = CallInfo {
        caller_address,
        call_type: Some(CallType::Delegate),
        contract_address: proxy_address,
        entry_point_selector: Some(amm_proxy_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: [].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 280,
            n_memory_holes: 20,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 2),
                (BuiltinName::range_check, 14),
            ]),
        }),
        class_hash: Some(proxy_class_hash),
        internal_calls,
        ..Default::default()
    };

    assert_eq!(result, expected_call_info);
}

#[test]
fn amm_proxy_get_pool_token_balance_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, contract_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    // Deploy proxy
    let (proxy_address, proxy_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm_proxy.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let proxy_entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&proxy_class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let calldata = [contract_address.0, 555.into(), 666.into()].to_vec();
    let caller_address = Address(1000000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &proxy_address,
        class_hash: &proxy_class_hash,
        entry_points_by_type: &proxy_entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // Add pool balance
    execute_entry_point("proxy_init_pool", &calldata, &mut call_config).unwrap();

    let calldata = [contract_address.0, 1.into()].to_vec();
    let result =
        execute_entry_point("proxy_get_pool_token_balance", &calldata, &mut call_config).unwrap();

    let amm_proxy_entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"proxy_get_pool_token_balance"));
    let amm_entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"get_pool_token_balance"));

    let accessed_storage_keys = get_accessed_keys("pool_balance", vec![vec![1_u8.into()]]);

    let internal_calls = vec![CallInfo {
        caller_address: proxy_address.clone(),
        call_type: Some(CallType::Call),
        contract_address,
        entry_point_selector: Some(amm_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone()[1..].to_vec(),
        retdata: [555.into()].to_vec(),
        storage_read_values: [555.into()].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 84,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 1),
                (BuiltinName::range_check, 3),
            ]),
        }),
        class_hash: Some(contract_class_hash),
        accessed_storage_keys,
        ..Default::default()
    }];

    let expected_call_info = CallInfo {
        caller_address,
        call_type: Some(CallType::Delegate),
        contract_address: proxy_address,
        entry_point_selector: Some(amm_proxy_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: [555.into()].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 140,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 1),
                (BuiltinName::range_check, 3),
            ]),
        }),
        class_hash: Some(proxy_class_hash),
        internal_calls,
        ..Default::default()
    };

    assert_eq!(result, expected_call_info);
}

#[test]
fn amm_proxy_add_demo_token_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, contract_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    // Deploy proxy
    let (proxy_address, proxy_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm_proxy.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let proxy_entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&proxy_class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let calldata = [contract_address.0, 555.into(), 666.into()].to_vec();
    let caller_address = Address(1000000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &proxy_address,
        class_hash: &proxy_class_hash,
        entry_points_by_type: &proxy_entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // Add pool balance
    execute_entry_point("proxy_init_pool", &calldata, &mut call_config).unwrap();

    let calldata = [contract_address.0, 55.into(), 66.into()].to_vec();
    let amm_proxy_entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"proxy_add_demo_token"));
    let result = execute_entry_point("proxy_add_demo_token", &calldata, &mut call_config).unwrap();

    let amm_entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"add_demo_token"));

    let mut felt_slice: [u8; 32] = [0; 32];
    felt_slice[0..32].copy_from_slice(&proxy_address.0.clone().to_bytes_be());
    let proxy_addres_felt = FieldElement::from_bytes_be(&felt_slice).unwrap();

    let accessed_storage_keys = get_accessed_keys(
        "account_balance",
        vec![
            vec![proxy_addres_felt, 1_u32.into()],
            vec![proxy_addres_felt, 2_u32.into()],
        ],
    );

    let internal_calls = vec![CallInfo {
        caller_address: proxy_address.clone(),
        call_type: Some(CallType::Call),
        contract_address,
        entry_point_selector: Some(amm_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone()[1..].to_vec(),
        storage_read_values: vec![0.into(), 0.into(), 0.into(), 0.into()],
        execution_resources: Some(ExecutionResources {
            n_steps: 397,
            n_memory_holes: 42,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 8),
                (BuiltinName::range_check, 20),
            ]),
        }),
        class_hash: Some(contract_class_hash),
        accessed_storage_keys,
        ..Default::default()
    }];

    let expected_call_info = CallInfo {
        caller_address,
        call_type: Some(CallType::Delegate),
        contract_address: proxy_address,
        entry_point_selector: Some(amm_proxy_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        execution_resources: Some(ExecutionResources {
            n_steps: 445,
            n_memory_holes: 42,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 8),
                (BuiltinName::range_check, 20),
            ]),
        }),
        class_hash: Some(proxy_class_hash),
        internal_calls,
        ..Default::default()
    };

    assert_eq!(result, expected_call_info);
}

#[test]
fn amm_proxy_get_account_token_balance() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, contract_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    // Deploy proxy
    let (proxy_address, proxy_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm_proxy.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let proxy_entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&proxy_class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let calldata = [contract_address.0, 100.into(), 200.into()].to_vec();
    let caller_address = Address(1000000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &proxy_address,
        class_hash: &proxy_class_hash,
        entry_points_by_type: &proxy_entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // Add account balance for the proxy contract in the amm contract
    execute_entry_point("proxy_add_demo_token", &calldata, &mut call_config).unwrap();

    //First argument is the amm contract address
    //Second argument is the account address, in this case the proxy address
    //Third argument is the token id
    let calldata = [contract_address.0, proxy_address.0, 2.into()].to_vec();
    let amm_proxy_entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"proxy_get_account_token_balance"));
    let result = execute_entry_point(
        "proxy_get_account_token_balance",
        &calldata,
        &mut call_config,
    )
    .unwrap();

    let amm_entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"get_account_token_balance"));

    let mut felt_slice: [u8; 32] = [0; 32];
    felt_slice[0..32].copy_from_slice(proxy_address.0.clone().to_bytes_be().get(0..32).unwrap());
    let proxy_addres_felt = FieldElement::from_bytes_be(&felt_slice).unwrap();

    let accessed_storage_keys = get_accessed_keys(
        "account_balance",
        vec![vec![proxy_addres_felt, 2_u8.into()]],
    );

    let internal_calls = vec![CallInfo {
        caller_address: proxy_address.clone(),
        call_type: Some(CallType::Call),
        contract_address,
        entry_point_selector: Some(amm_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone()[1..].to_vec(),
        retdata: [200.into()].to_vec(),
        storage_read_values: [200.into()].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 92,
            n_memory_holes: 11,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 2),
                (BuiltinName::range_check, 3),
            ]),
        }),
        class_hash: Some(contract_class_hash),
        accessed_storage_keys,
        ..Default::default()
    }];

    let expected_call_info = CallInfo {
        caller_address,
        call_type: Some(CallType::Delegate),
        contract_address: proxy_address,
        entry_point_selector: Some(amm_proxy_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: [200.into()].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 151,
            n_memory_holes: 11,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 2),
                (BuiltinName::range_check, 3),
            ]),
        }),
        class_hash: Some(proxy_class_hash),
        internal_calls,
        ..Default::default()
    };

    assert_eq!(result, expected_call_info);
}

#[test]
fn amm_proxy_swap() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, contract_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    // Deploy proxy
    let (proxy_address, proxy_class_hash) = deploy(
        &mut state,
        "starknet_programs/amm_proxy.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let proxy_entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&proxy_class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let calldata = [contract_address.0, 100.into(), 200.into()].to_vec();
    let caller_address = Address(1000000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &proxy_address,
        class_hash: &proxy_class_hash,
        entry_points_by_type: &proxy_entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // Add account balance for the proxy contract in the amm contract
    execute_entry_point("proxy_add_demo_token", &calldata, &mut call_config).unwrap();

    //Init pool to have 1000 tokens of each type
    let calldata = [contract_address.0, 1000.into(), 1000.into()].to_vec();
    execute_entry_point("proxy_init_pool", &calldata, &mut call_config).unwrap();

    //Swap 100 tokens of type 1 for type 2
    //First argument is the amm contract address
    //Second argunet is the token to swap (type 1)
    //Third argument is the amount of tokens to swap (100)
    let calldata = [contract_address.0, 1.into(), 100.into()].to_vec();
    let expected_result = [90.into()].to_vec();
    let result = execute_entry_point("proxy_swap", &calldata, &mut call_config).unwrap();

    let amm_proxy_entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"proxy_swap"));

    let amm_entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"swap"));
    //checked for amm contract both tokens balances
    let accessed_storage_keys_pool_balance =
        get_accessed_keys("pool_balance", vec![vec![1_u8.into()], vec![2_u8.into()]]);

    let mut felt_slice: [u8; 32] = [0; 32];
    felt_slice[0..32].copy_from_slice(proxy_address.0.clone().to_bytes_be().get(0..32).unwrap());
    let proxy_addres_felt = FieldElement::from_bytes_be(&felt_slice).unwrap();

    //checked for proxy account both tokens balances
    let accessed_storage_keys_user_balance = get_accessed_keys(
        "account_balance",
        vec![
            vec![proxy_addres_felt, 1_u8.into()],
            vec![proxy_addres_felt, 2_u8.into()],
        ],
    );

    let mut accessed_storage_keys = HashSet::new();
    accessed_storage_keys.extend(accessed_storage_keys_pool_balance);
    accessed_storage_keys.extend(accessed_storage_keys_user_balance);

    let internal_calls = vec![CallInfo {
        caller_address: proxy_address.clone(),
        call_type: Some(CallType::Call),
        contract_address,
        entry_point_selector: Some(amm_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone()[1..].to_vec(),
        retdata: [90.into()].to_vec(),
        storage_read_values: [
            100.into(),
            1000.into(),
            1000.into(),
            100.into(),
            100.into(),
            1000.into(),
            200.into(),
            200.into(),
            1000.into(),
        ]
        .to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 826,
            n_memory_holes: 92,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 14),
                (BuiltinName::range_check, 41),
            ]),
        }),
        class_hash: Some(contract_class_hash),
        accessed_storage_keys,
        ..Default::default()
    }];

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: proxy_address.clone(),
        entry_point_selector: Some(amm_proxy_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_result,
        execution_resources: Some(ExecutionResources {
            n_steps: 885,
            n_memory_holes: 92,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::pedersen, 14),
                (BuiltinName::range_check, 41),
            ]),
        }),
        class_hash: Some(proxy_class_hash),
        internal_calls,
        ..Default::default()
    };

    assert_eq!(result, expected_call_info);
}
