#![deny(warnings)]

use felt::Felt;
use starknet_rs::{
    business_logic::{
        execution::{
            execution_entry_point::ExecutionEntryPoint,
            objects::{CallInfo, CallType, TransactionExecutionContext},
        },
        fact_state::{
            contract_state::ContractState, in_memory_state_reader::InMemoryStateReader,
            state::ExecutionResourcesManager,
        },
        state::cached_state::CachedState,
    },
    definitions::{
        constants::TRANSACTION_VERSION,
        general_config::{StarknetChainId, StarknetGeneralConfig},
    },
    services::api::contract_class::{ContractClass, EntryPointType},
    starknet_storage::dict_storage::DictStorage,
    utils::{calculate_sn_keccak, Address},
};
use std::path::Path;

#[allow(clippy::too_many_arguments)]
fn test_contract(
    contract_path: impl AsRef<Path>,
    entry_point: &str,
    class_hash: [u8; 32],
    contract_address: Address,
    caller_address: Address,
    general_config: StarknetGeneralConfig,
    tx_context: Option<TransactionExecutionContext>,
    return_data: impl Into<Vec<Felt>>,
) {
    let contract_class = ContractClass::try_from(contract_path.as_ref().to_path_buf())
        .expect("Could not load contract from JSON");

    let tx_execution_context = tx_context.unwrap_or_else(|| {
        TransactionExecutionContext::create_for_testing(
            Address(0.into()),
            10,
            0.into(),
            general_config.invoke_tx_max_n_steps(),
            TRANSACTION_VERSION,
        )
    });

    let contract_state = ContractState::new(
        class_hash,
        tx_execution_context.nonce().clone(),
        Default::default(),
    );
    let mut state_reader = InMemoryStateReader::new(DictStorage::new(), DictStorage::new());
    state_reader
        .contract_states_mut()
        .insert(contract_address.clone(), contract_state);
    let mut state = CachedState::new(
        state_reader,
        Some([(class_hash, contract_class)].into_iter().collect()),
    );

    let entry_point_selector = Felt::from_bytes_be(&calculate_sn_keccak(entry_point.as_bytes()));
    let entry_point = ExecutionEntryPoint::new(
        contract_address.clone(),
        vec![],
        entry_point_selector.clone(),
        caller_address.clone(),
        EntryPointType::External,
        CallType::Delegate.into(),
        class_hash.into(),
    );

    let mut resources_manager = ExecutionResourcesManager::default();

    assert_eq!(
        entry_point
            .execute(
                &mut state,
                &general_config,
                &mut resources_manager,
                &tx_execution_context,
            )
            .expect("Could not execute contract"),
        CallInfo {
            contract_address,
            caller_address,
            entry_point_type: EntryPointType::External.into(),
            call_type: CallType::Delegate.into(),
            class_hash: class_hash.into(),
            entry_point_selector: Some(entry_point_selector),
            retdata: return_data.into(),
            ..Default::default()
        },
    );
}

#[test]
fn get_block_number_syscall() {
    let run = |block_number| {
        let mut general_config = StarknetGeneralConfig::default();
        general_config.block_info_mut().block_number = block_number;

        test_contract(
            "tests/syscalls.json",
            "test_get_block_number",
            [1; 32],
            Address(1111.into()),
            Address(0.into()),
            general_config,
            None,
            [block_number.into()],
        );
    };

    run(0);
    run(5);
    run(1000);
}

#[test]
fn get_block_timestamp_syscall() {
    let run = |block_timestamp| {
        let mut general_config = StarknetGeneralConfig::default();
        general_config.block_info_mut().block_timestamp = block_timestamp;

        test_contract(
            "tests/syscalls.json",
            "test_get_block_timestamp",
            [1; 32],
            Address(1111.into()),
            Address(0.into()),
            general_config,
            None,
            [block_timestamp.into()],
        );
    };

    run(0);
    run(5);
    run(1000);
}

#[test]
fn get_caller_address_syscall() {
    let run = |caller_address: Felt| {
        test_contract(
            "tests/syscalls.json",
            "test_get_caller_address",
            [1; 32],
            Address(1111.into()),
            Address(caller_address.clone()),
            StarknetGeneralConfig::default(),
            None,
            [caller_address],
        );
    };

    run(0.into());
    run(5.into());
    run(1000.into());
}

#[test]
fn get_contract_address_syscall() {
    let run = |contract_address: Felt| {
        test_contract(
            "tests/syscalls.json",
            "test_get_contract_address",
            [1; 32],
            Address(contract_address.clone()),
            Address(0.into()),
            StarknetGeneralConfig::default(),
            None,
            [contract_address],
        );
    };

    run(1.into());
    run(5.into());
    run(1000.into());
}

#[test]
fn get_sequencer_address_syscall() {
    let run = |sequencer_address: Felt| {
        let mut general_config = StarknetGeneralConfig::default();
        general_config.block_info_mut().sequencer_address = Address(sequencer_address.clone());

        test_contract(
            "tests/syscalls.json",
            "test_get_sequencer_address",
            [1; 32],
            Address(1111.into()),
            Address(0.into()),
            general_config,
            None,
            [sequencer_address],
        );
    };

    run(0.into());
    run(5.into());
    run(1000.into());
}

#[test]
fn get_tx_info_syscall() {
    let run = |version,
               account_contract_address: Address,
               max_fee,
               signature: Vec<Felt>,
               transaction_hash: Felt,
               chain_id| {
        let mut general_config = StarknetGeneralConfig::default();
        *general_config.starknet_os_config_mut().chain_id_mut() = chain_id;

        let n_steps = general_config.invoke_tx_max_n_steps();
        test_contract(
            "tests/syscalls.json",
            "test_get_tx_info",
            [1; 32],
            Address(1111.into()),
            Address(0.into()),
            general_config,
            Some(TransactionExecutionContext::new(
                account_contract_address.clone(),
                transaction_hash.clone(),
                signature.clone(),
                max_fee,
                3.into(),
                n_steps,
                version,
            )),
            [
                version.into(),
                account_contract_address.0,
                max_fee.into(),
                signature.len().into(),
                signature
                    .into_iter()
                    .reduce(|a, b| a + b)
                    .unwrap_or_default(),
                transaction_hash,
                chain_id.to_felt(),
            ],
        );
    };

    run(
        0,
        Address::default(),
        12,
        vec![],
        0.into(),
        StarknetChainId::TestNet,
    );
    run(
        10,
        Address::default(),
        12,
        vec![],
        0.into(),
        StarknetChainId::TestNet,
    );
    run(
        10,
        Address(1111.into()),
        12,
        vec![],
        0.into(),
        StarknetChainId::TestNet,
    );
    run(
        10,
        Address(1111.into()),
        50,
        vec![],
        0.into(),
        StarknetChainId::TestNet,
    );
    run(
        10,
        Address(1111.into()),
        50,
        [0x12, 0x34, 0x56, 0x78].map(Felt::from).to_vec(),
        0.into(),
        StarknetChainId::TestNet,
    );
    run(
        10,
        Address(1111.into()),
        50,
        [0x12, 0x34, 0x56, 0x78].map(Felt::from).to_vec(),
        12345678.into(),
        StarknetChainId::TestNet,
    );
    run(
        10,
        Address(1111.into()),
        50,
        [0x12, 0x34, 0x56, 0x78].map(Felt::from).to_vec(),
        12345678.into(),
        StarknetChainId::TestNet2,
    );
}

#[test]
fn get_tx_signature_syscall() {
    let run = |signature: Vec<Felt>| {
        let general_config = StarknetGeneralConfig::default();
        let n_steps = general_config.invoke_tx_max_n_steps();

        test_contract(
            "tests/syscalls.json",
            "test_get_tx_signature",
            [1; 32],
            Address(1111.into()),
            Address(0.into()),
            general_config,
            Some(TransactionExecutionContext::new(
                Address::default(),
                0.into(),
                signature.clone(),
                12,
                3.into(),
                n_steps,
                0,
            )),
            [
                signature.len().into(),
                signature
                    .into_iter()
                    .reduce(|a, b| a + b)
                    .unwrap_or_default(),
            ],
        );
    };

    run(vec![]);
    run([0x12, 0x34, 0x56, 0x78].map(Felt::from).to_vec());
    run([0x9A, 0xBC, 0xDE, 0xF0].map(Felt::from).to_vec());
}
