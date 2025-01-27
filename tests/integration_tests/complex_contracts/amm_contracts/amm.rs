use crate::integration_tests::complex_contracts::utils::get_accessed_keys;
use crate::integration_tests::complex_contracts::utils::*;
use cairo_vm::{
    types::builtin_name::BuiltinName, vm::runners::cairo_runner::ExecutionResources, Felt252,
};

use starknet_in_rust::{
    definitions::block_context::BlockContext,
    execution::{CallInfo, CallType},
    services::api::contract_classes::deprecated_contract_class::ContractClass,
    state::{
        cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, state_api::StateReader,
        ExecutionResourcesManager,
    },
    transaction::{error::TransactionError, Address},
    utils::calculate_sn_keccak,
    EntryPointType,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn init_pool(
    calldata: &[Felt252],
    call_config: &mut CallConfig,
) -> Result<CallInfo, TransactionError> {
    execute_entry_point("init_pool", calldata, call_config)
}

fn get_pool_token_balance(
    calldata: &[Felt252],
    call_config: &mut CallConfig,
) -> Result<CallInfo, TransactionError> {
    execute_entry_point("get_pool_token_balance", calldata, call_config)
}

fn get_account_token_balance(
    calldata: &[Felt252],
    call_config: &mut CallConfig,
) -> Result<CallInfo, TransactionError> {
    execute_entry_point("get_account_token_balance", calldata, call_config)
}

fn add_demo_token(
    calldata: &[Felt252],
    call_config: &mut CallConfig,
) -> Result<CallInfo, TransactionError> {
    execute_entry_point("add_demo_token", calldata, call_config)
}

// Swap function to execute swap between two tokens
fn swap(calldata: &[Felt252], call_config: &mut CallConfig) -> Result<CallInfo, TransactionError> {
    execute_entry_point("swap", calldata, call_config)
}

#[test]
fn amm_init_pool_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let calldata = [10000.into(), 10000.into()].to_vec();
    let caller_address = Address(0000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let amm_entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"init_pool"));
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let accessed_storage_keys =
        get_accessed_keys("pool_balance", vec![vec![1_u8.into()], vec![2_u8.into()]]);

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(amm_entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: [].to_vec(),
        execution_resources: Some(ExecutionResources {
            n_steps: 232,
            n_memory_holes: 20,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 14),
                (BuiltinName::keccak, 2),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: vec![Felt252::ZERO, Felt252::ZERO],
        ..Default::default()
    };

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    assert_eq!(
        init_pool(&calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn amm_add_demo_tokens_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let calldata = [10000.into(), 10000.into()].to_vec();
    let caller_address = Address(0000.into());
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    init_pool(&calldata, &mut call_config).unwrap();

    let calldata_add_demo_token = [100.into(), 100.into()].to_vec();

    let add_demo_token_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"add_demo_token"));

    let accessed_storage_keys_add_demo_token = get_accessed_keys(
        "account_balance",
        vec![
            vec![0_u8.into(), 1_u8.into()],
            vec![0_u8.into(), 2_u8.into()],
        ],
    );

    let expected_call_info_add_demo_token = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(add_demo_token_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata_add_demo_token.clone(),
        execution_resources: Some(ExecutionResources {
            n_steps: 393,
            n_memory_holes: 44,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 20),
                (BuiltinName::keccak, 8),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys: accessed_storage_keys_add_demo_token,
        storage_read_values: vec![Felt252::ZERO, Felt252::ZERO, Felt252::ZERO, Felt252::ZERO],
        ..Default::default()
    };

    assert_eq!(
        add_demo_token(&calldata_add_demo_token, &mut call_config).unwrap(),
        expected_call_info_add_demo_token
    );
}

#[test]
fn amm_get_pool_token_balance() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();

    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    let calldata = [10000.into(), 10000.into()].to_vec();
    let caller_address = Address(0000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    init_pool(&calldata, &mut call_config).unwrap();

    let calldata_get_pool_token_balance = [1.into()].to_vec();

    let get_pool_balance_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"get_pool_token_balance"));

    let accessed_storage_keys_get_pool_token_balance =
        get_accessed_keys("pool_balance", vec![vec![1_u8.into()]]);

    let expected_call_info_get_pool_token_balance = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(get_pool_balance_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata_get_pool_token_balance.clone(),
        execution_resources: Some(ExecutionResources {
            n_steps: 84,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 3),
                (BuiltinName::keccak, 1),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys: accessed_storage_keys_get_pool_token_balance,
        storage_read_values: vec![10000.into()],
        retdata: [10000.into()].to_vec(),
        ..Default::default()
    };

    assert_eq!(
        get_pool_token_balance(&calldata_get_pool_token_balance, &mut call_config).unwrap(),
        expected_call_info_get_pool_token_balance
    );
}

#[test]
fn amm_swap_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();

    let calldata = [10000.into(), 10000.into()].to_vec();
    let caller_address = Address(0000.into());
    let mut resources_manager = ExecutionResourcesManager::default();

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    init_pool(&calldata, &mut call_config).unwrap();

    //add tokens to user
    let calldata_add_demo_token = [100.into(), 100.into()].to_vec();
    add_demo_token(&calldata_add_demo_token, &mut call_config).unwrap();

    let calldata_swap = [1.into(), 10.into()].to_vec();

    let expected_return = [9.into()].to_vec();

    //access keys are all keys in pool balance and only this users balance but thats checked in account
    let accessed_storage_keys_pool_balance =
        get_accessed_keys("pool_balance", vec![vec![1_u8.into()], vec![2_u8.into()]]);
    //access keys balance of user. In account balance we ask for users address as key
    let accessed_storage_keys_user_balance = get_accessed_keys(
        "account_balance",
        vec![
            vec![0_u8.into(), 1_u8.into()],
            vec![0_u8.into(), 2_u8.into()],
        ],
    );

    //make the two hashsets as one
    let mut accessed_storage_keys = HashSet::new();
    accessed_storage_keys.extend(accessed_storage_keys_pool_balance);
    accessed_storage_keys.extend(accessed_storage_keys_user_balance);

    let swap_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"swap"));

    let expected_call_infoswap = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(swap_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata_swap.clone(),
        retdata: expected_return,
        execution_resources: Some(ExecutionResources {
            n_steps: 820,
            n_memory_holes: 95,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 41),
                (BuiltinName::keccak, 14),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: [
            100.into(),
            10000.into(),
            10000.into(),
            100.into(),
            100.into(),
            10000.into(),
            100.into(),
            100.into(),
            10000.into(),
        ]
        .to_vec(),
        ..Default::default()
    };

    assert_eq!(
        swap(&calldata_swap, &mut call_config).unwrap(),
        expected_call_infoswap
    );
}

#[test]
fn amm_init_pool_should_fail_with_amount_out_of_bounds() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    let calldata = [Felt252::from(2_u32.pow(30)), Felt252::from(2_u32.pow(30))].to_vec();
    let caller_address = Address(0000.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    assert!(init_pool(&calldata, &mut call_config).is_err());
}

#[test]
fn amm_swap_should_fail_with_unexistent_token() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    let calldata = [Felt252::ZERO, Felt252::from(10)].to_vec();
    let caller_address = Address(0000.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    assert!(swap(&calldata, &mut call_config).is_err());
}

#[test]
fn amm_swap_should_fail_with_amount_out_of_bounds() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    let calldata = [Felt252::ONE, Felt252::from(2_u32.pow(30))].to_vec();
    let caller_address = Address(0000.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    assert!(swap(&calldata, &mut call_config).is_err());
}

#[test]
fn amm_swap_should_fail_when_user_does_not_have_enough_funds() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    let calldata = [Felt252::ONE, Felt252::from(100)].to_vec();
    let caller_address = Address(0000.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    init_pool(
        &[Felt252::from(1000), Felt252::from(1000)],
        &mut call_config,
    )
    .unwrap();
    add_demo_token(&[Felt252::from(10), Felt252::from(10)], &mut call_config).unwrap();

    assert!(swap(&calldata, &mut call_config).is_err());
}

#[test]
fn amm_get_account_token_balance_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );
    // Deploy contract
    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/amm.json",
        &[],
        &block_context,
        None,
    )
    .unwrap();
    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    //add 10 tokens of token type 1
    let caller_address = Address(0000.into());
    let calldata = [10.into(), 0.into()].to_vec();

    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &EntryPointType::External,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    add_demo_token(&calldata, &mut call_config).unwrap();

    let calldata_get_balance = [0000.into(), 1.into()].to_vec();
    let result = get_account_token_balance(&calldata_get_balance, &mut call_config);

    //expected return value 10
    let expected_return = [10.into()].to_vec();

    let accessed_storage_keys =
        get_accessed_keys("account_balance", vec![vec![0_u8.into(), 1_u8.into()]]);

    let get_account_token_balance_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(b"get_account_token_balance"));

    let expected_call_info_get_account_token_balance = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address,
        entry_point_selector: Some(get_account_token_balance_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata_get_balance,
        retdata: expected_return,
        execution_resources: Some(ExecutionResources {
            n_steps: 92,
            n_memory_holes: 11,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 3),
                (BuiltinName::keccak, 2),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: [10.into()].to_vec(),
        ..Default::default()
    };

    assert_eq!(
        result.unwrap(),
        expected_call_info_get_account_token_balance
    );
}
