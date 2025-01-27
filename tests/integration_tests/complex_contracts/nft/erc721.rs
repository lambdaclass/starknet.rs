use crate::integration_tests::complex_contracts::utils::*;
use assert_matches::assert_matches;
use cairo_vm::{
    types::builtin_name::BuiltinName, vm::runners::cairo_runner::ExecutionResources, Felt252,
};

use starknet_crypto::FieldElement;
use starknet_in_rust::definitions::block_context::BlockContext;
use starknet_in_rust::services::api::contract_classes::deprecated_contract_class::ContractClass;
use starknet_in_rust::state::cached_state::CachedState;
use starknet_in_rust::transaction::error::TransactionError;
use starknet_in_rust::transaction::ClassHash;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    execution::{CallInfo, CallType, OrderedEvent},
    state::{
        contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, state_api::StateReader,
        ExecutionResourcesManager,
    },
    transaction::Address,
    utils::calculate_sn_keccak,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[test]
fn erc721_constructor_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice(b"some-nft");
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = vec![collection_name, collection_symbol, to];

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let entry_points_by_type =
        TryInto::<ContractClass>::try_into(state.get_contract_class(&class_hash).unwrap())
            .unwrap()
            .entry_points_by_type()
            .clone();
    let entry_point_type = EntryPointType::External;
    let mut resources_manager = ExecutionResourcesManager::default();
    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &Address(111.into()),
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    let result_get_name = execute_entry_point("name", &[], &mut call_config).unwrap();
    assert_eq!(result_get_name.retdata, vec![calldata[0]]);
    let result_get_symbol = execute_entry_point("symbol", &[], &mut call_config).unwrap();
    assert_eq!(result_get_symbol.retdata, vec![calldata[1]]);
}

#[test]
fn erc721_balance_of_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

    let entry_point_type_constructor = EntryPointType::Constructor;
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
        entry_point_type: &entry_point_type_constructor,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    //owner to check balance
    let calldata = [Felt252::from(666)].to_vec();

    call_config.entry_point_type = &entry_point_type;

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"balanceOf"));

    //expected result should be 1,0 in uint256. So in Felt252 it should be [1,0]
    let expected_read_result = vec![Felt252::from(1_u8), Felt252::from(0_u8)];

    let mut accessed_storage_keys = HashSet::new();
    let mut balance = get_accessed_keys("ERC721_balances", vec![vec![666_u32.into()]])
        .drain()
        .collect::<Vec<ClassHash>>()[0];
    accessed_storage_keys.insert(balance);
    balance.0[31] += 1;
    accessed_storage_keys.insert(balance);

    let expected_call_info = CallInfo {
        caller_address: Address(666.into()),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result.clone(),
        execution_resources: Some(ExecutionResources {
            n_steps: 105,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 3),
                (BuiltinName::keccak, 1),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: expected_read_result,
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("balanceOf", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_test_owner_of() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    //tokenId to ask for owner
    let calldata = [Felt252::from(1), Felt252::from(0)].to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"ownerOf"));

    let expected_read_result = vec![Felt252::from(666)];

    let accessed_storage_keys =
        get_accessed_keys("ERC721_owners", vec![vec![1_u32.into(), 0_u32.into()]]);

    let expected_call_info = CallInfo {
        caller_address: Address(666.into()),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result.clone(),
        execution_resources: Some(ExecutionResources {
            n_steps: 116,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 5),
                (BuiltinName::keccak, 2),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: expected_read_result,
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("ownerOf", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_test_get_approved() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // The address given approval to transfer the token
    let to = Felt252::from(777);
    let calldata_approve = [to, Felt252::from(1), Felt252::from(0)].to_vec();
    call_config.entry_point_type = &entry_point_type;

    execute_entry_point("approve", &calldata_approve, &mut call_config).unwrap();

    // tokenId (uint256) to check if it is approved
    let calldata = [Felt252::from(1), Felt252::from(0)].to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"getApproved"));

    // expected result is 0 because it is not approved
    let expected_read_result = vec![Felt252::from(777)];

    // First checks if the token is owned by anyone and then checks if it is approved
    let storage_read_values = vec![Felt252::from(666), Felt252::from(777)];

    let mut accessed_storage_keys = get_accessed_keys(
        "ERC721_token_approvals",
        vec![vec![1_u32.into(), 0_u32.into()]],
    );
    accessed_storage_keys.extend(get_accessed_keys(
        "ERC721_owners",
        vec![vec![1_u8.into(), 0_u8.into()]],
    ));

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result,
        execution_resources: Some(ExecutionResources {
            n_steps: 192,
            n_memory_holes: 20,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 8),
                (BuiltinName::keccak, 4),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values,
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("getApproved", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_test_is_approved_for_all() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // The address given approval to transfer the token
    let to = Felt252::from(777);
    let calldata_set_approve_all = [to, Felt252::from(1)].to_vec();

    execute_entry_point(
        "setApprovalForAll",
        &calldata_set_approve_all,
        &mut call_config,
    )
    .unwrap();

    // Owner of tokens who is approving the operator to have control of all his tokens
    let owner = Felt252::from(666);
    // The address in control for the approvals
    let operator = Felt252::from(777);
    let calldata = [owner, operator].to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"isApprovedForAll"));

    // expected result is 0 because it is not approved
    let expected_read_result = vec![Felt252::from(1)];

    // Checks only the storage variable ERC721_operator_approvals
    let storage_read_values = vec![Felt252::from(1)];

    let accessed_storage_keys = get_accessed_keys(
        "ERC721_operator_approvals",
        vec![vec![666_u32.into(), 777_u32.into()]],
    );

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result,
        execution_resources: Some(ExecutionResources {
            n_steps: 101,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 3),
                (BuiltinName::keccak, 2),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values,
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("isApprovedForAll", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_test_approve() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };
    // The address given approval to transfer the token
    let to = Felt252::from(777);
    let calldata = [to, Felt252::from(1), Felt252::from(0)].to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"approve"));

    let expected_read_result = vec![];

    // Checks only the storage variable ERC721_operator_approvals
    let storage_read_values = vec![Felt252::from(666), Felt252::ZERO, Felt252::from(666)];

    // Ask for the owner of the token
    let mut accessed_storage_keys =
        get_accessed_keys("ERC721_owners", vec![vec![1_u32.into(), 0_u32.into()]]);
    // Writes the new approval
    accessed_storage_keys.extend(get_accessed_keys(
        "ERC721_token_approvals",
        vec![vec![1_u32.into(), 0_u32.into()]],
    ));

    let event_hash = Felt252::from_bytes_be(&calculate_sn_keccak("Approval".as_bytes()));
    let expected_events = vec![OrderedEvent::new(
        0,
        vec![event_hash],
        vec![
            Felt252::from(666),
            Felt252::from(777),
            Felt252::from(1),
            Felt252::ZERO,
        ],
    )];

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result,
        execution_resources: Some(ExecutionResources {
            n_steps: 332,
            n_memory_holes: 30,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 13),
                (BuiltinName::keccak, 6),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values,
        events: expected_events,
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("approve", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_set_approval_for_all() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // The address given approval to transfer the token
    let to = Felt252::from(777);
    let calldata = [to, Felt252::from(1)].to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"setApprovalForAll"));

    // set only no return value
    let expected_read_result = vec![];

    // Only writes in operator_approvals
    let storage_read_values = vec![Felt252::ZERO];

    // Writes to the operator the new set value
    let accessed_storage_keys = get_accessed_keys(
        "ERC721_operator_approvals",
        vec![vec![666_u32.into(), 777_u32.into()]],
    );

    let event_hash = Felt252::from_bytes_be(&calculate_sn_keccak("ApprovalForAll".as_bytes()));
    let expected_events = vec![OrderedEvent::new(
        0,
        vec![event_hash],
        vec![Felt252::from(666), Felt252::from(777), Felt252::from(1)],
    )];

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result,
        execution_resources: Some(ExecutionResources {
            n_steps: 154,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 3),
                (BuiltinName::keccak, 2),
            ]),
        }),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values,
        events: expected_events,
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("setApprovalForAll", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_transfer_from_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    let calldata = [
        Felt252::from(666),
        Felt252::from(777),
        Felt252::from(1),
        Felt252::ZERO,
    ]
    .to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"transferFrom"));

    let mut accessed_storage_keys = get_accessed_keys(
        "ERC721_owners",
        vec![vec![FieldElement::from(1_u8), FieldElement::from(0_u8)]],
    );
    accessed_storage_keys.extend(get_accessed_keys(
        "ERC721_token_approvals",
        vec![vec![FieldElement::from(1_u8), FieldElement::from(0_u8)]],
    ));

    let mut balance_from = get_accessed_keys("ERC721_balances", vec![vec![666_u32.into()]])
        .drain()
        .collect::<Vec<ClassHash>>()[0];
    accessed_storage_keys.insert(balance_from);
    balance_from.0[31] += 1;
    accessed_storage_keys.insert(balance_from);

    let mut balance_to = get_accessed_keys("ERC721_balances", vec![vec![777_u32.into()]])
        .drain()
        .collect::<Vec<ClassHash>>()[0];
    accessed_storage_keys.insert(balance_to);
    balance_to.0[31] += 1;
    accessed_storage_keys.insert(balance_to);

    let expected_read_values = vec![
        666.into(),
        666.into(),
        666.into(),
        0.into(),
        666.into(),
        1.into(),
        0.into(),
        1.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        0.into(),
        666.into(),
    ];

    let approval_event_hash = Felt252::from_bytes_be(&calculate_sn_keccak("Approval".as_bytes()));
    let approval_event = OrderedEvent::new(
        0,
        vec![approval_event_hash],
        vec![
            Felt252::from(666),
            Felt252::ZERO,
            Felt252::from(1),
            Felt252::ZERO,
        ],
    );
    let transfer_event_hash = Felt252::from_bytes_be(&calculate_sn_keccak("Transfer".as_bytes()));
    let transfer_event = OrderedEvent::new(
        1,
        vec![transfer_event_hash],
        vec![
            Felt252::from(666),
            Felt252::from(777),
            Felt252::from(1),
            Felt252::ZERO,
        ],
    );
    let expected_events = vec![approval_event, transfer_event];

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: expected_read_values,
        events: expected_events,
        execution_resources: Some(ExecutionResources {
            n_steps: 1131,
            n_memory_holes: 117,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 53),
                (BuiltinName::keccak, 16),
            ]),
        }),
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("transferFrom", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_transfer_from_and_get_owner_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    let calldata = [
        Felt252::from(666),
        Felt252::from(777),
        Felt252::from(1),
        Felt252::ZERO,
    ]
    .to_vec();
    execute_entry_point("transferFrom", &calldata, &mut call_config).unwrap();

    // Now we call ownerOf
    let calldata = [Felt252::from(1), Felt252::from(0)].to_vec();

    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"ownerOf"));

    let expected_read_result = vec![Felt252::from(777)];

    let accessed_storage_keys =
        get_accessed_keys("ERC721_owners", vec![vec![1_u32.into(), 0_u32.into()]]);

    let expected_call_info = CallInfo {
        caller_address: caller_address.clone(),
        call_type: Some(CallType::Delegate),
        contract_address: contract_address.clone(),
        entry_point_selector: Some(entrypoint_selector),
        entry_point_type: Some(EntryPointType::External),
        calldata: calldata.clone(),
        retdata: expected_read_result.clone(),
        class_hash: Some(class_hash),
        accessed_storage_keys,
        storage_read_values: expected_read_result,
        execution_resources: Some(ExecutionResources {
            n_steps: 116,
            n_memory_holes: 10,
            builtin_instance_counter: HashMap::from([
                (BuiltinName::range_check, 5),
                (BuiltinName::keccak, 2),
            ]),
        }),
        ..Default::default()
    };

    assert_eq!(
        execute_entry_point("ownerOf", &calldata, &mut call_config).unwrap(),
        expected_call_info
    );
}

#[test]
fn erc721_safe_transfer_from_should_fail_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    deploy(
        &mut state,
        "starknet_programs/ERC165.json",
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

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

    let mut call_config = CallConfig {
        state: &mut state,
        caller_address: &caller_address,
        address: &contract_address,
        class_hash: &class_hash,
        entry_points_by_type: &entry_points_by_type,
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    // data_len = 0 then there is no need to sent *felt for data
    let calldata = [
        Felt252::from(666),
        Felt252::from(1000000),
        Felt252::from(1),
        Felt252::ZERO,
        Felt252::ZERO,
    ]
    .to_vec();

    // The contract will fail because the receiver address is not a IERC721Receiver contract.
    assert!(execute_entry_point("safeTransferFrom", &calldata, &mut call_config).is_err());
}

#[test]
fn erc721_calling_constructor_twice_should_fail_test() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::Constructor;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    assert!(execute_entry_point("constructor", &calldata, &mut call_config).is_err());
}

//Should panic is necessary because the constructor will fail
//deploy() will try to unwrap the result of the constructor
#[test]
fn erc721_constructor_should_fail_with_to_equal_zero() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(0);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    //call deploy but assert that its a transaction error of type cairo runner
    assert_matches!(
        deploy(
            &mut state,
            "starknet_programs/ERC721.json",
            &calldata,
            &block_context,
            None
        )
        .unwrap_err(),
        TransactionError::CairoRunner(..)
    );
}

#[test]
fn erc721_transfer_fail_to_zero_address() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    let calldata = [
        Felt252::from(666),
        Felt252::ZERO,
        Felt252::from(1),
        Felt252::ZERO,
    ]
    .to_vec();
    assert!(execute_entry_point("transferFrom", &calldata, &mut call_config).is_err());
}

#[test]
fn erc721_transfer_fail_not_owner() {
    let block_context = BlockContext::default();
    let mut state = CachedState::new(
        Arc::new(InMemoryStateReader::default()),
        Arc::new(PermanentContractClassCache::default()),
    );

    let collection_name = Felt252::from_bytes_be_slice("some-nft".as_bytes());
    let collection_symbol = Felt252::from(555);
    let to = Felt252::from(666);
    let calldata = [collection_name, collection_symbol, to].to_vec();

    let (contract_address, class_hash) = deploy(
        &mut state,
        "starknet_programs/ERC721.json",
        &calldata,
        &block_context,
        None,
    )
    .unwrap();

    let caller_address = Address(666.into());
    let block_context = BlockContext::default();
    let mut resources_manager = ExecutionResourcesManager::default();
    let entry_point_type = EntryPointType::External;

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
        entry_point_type: &entry_point_type,
        block_context: &block_context,
        resources_manager: &mut resources_manager,
    };

    let calldata = [
        Felt252::from(777),
        Felt252::from(777),
        Felt252::from(1),
        Felt252::ZERO,
    ]
    .to_vec();

    assert!(execute_entry_point("transferFrom", &calldata, &mut call_config).is_err());
}
