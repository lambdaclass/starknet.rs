use std::sync::Arc;

use cairo_vm::Felt252;
use starknet_in_rust::{
    core::contract_address::compute_casm_class_hash,
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
    },
    transaction::{Address, ClassHash, InvokeFunction, Transaction},
    utils::calculate_sn_keccak,
    CasmContractClass,
};

#[test]
fn account_panic() {
    let account_data = include_bytes!("../../starknet_programs/cairo2/account_panic.casm");
    let contract_data = include_bytes!("../../starknet_programs/cairo2/contract_a.casm");

    let account_contract_class: CasmContractClass = serde_json::from_slice(account_data).unwrap();
    let account_class_hash = ClassHash(
        compute_casm_class_hash(&account_contract_class)
            .unwrap()
            .to_bytes_be(),
    );

    let contract_class: CasmContractClass = serde_json::from_slice(contract_data).unwrap();
    let contract_class_hash_felt = compute_casm_class_hash(&contract_class).unwrap();
    let contract_class_hash = ClassHash::from(contract_class_hash_felt);

    let account_address = Address(1111.into());
    let contract_address = Address(0000.into());
    let nonce = 0.into();

    let block_context = BlockContext::default();

    let contract_class_cache = PermanentContractClassCache::default();

    contract_class_cache.set_contract_class(
        account_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(account_contract_class),
            sierra: None,
        },
    );
    contract_class_cache.set_contract_class(
        contract_class_hash,
        CompiledClass::Casm {
            casm: Arc::new(contract_class.clone()),
            sierra: None,
        },
    );

    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(account_address.clone(), account_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(account_address.clone(), nonce);
    state_reader
        .address_to_class_hash_mut()
        .insert(contract_address.clone(), contract_class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(contract_address, 1.into());
    let mut state = CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

    let selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"__execute__"));

    // arguments of contract_a contract
    // calldata is a Vec of Call, which is
    /*
       #[derive(Drop, Serde)]
       struct Call {
           to: ContractAddress,
           selector: felt252,
           calldata: Array<felt252>
       }
    */
    let selector_contract = &contract_class
        .entry_points_by_type
        .external
        .first()
        .unwrap()
        .selector;
    // calldata of contract_a is 1 value.
    let calldata: Vec<_> = [
        1.into(),
        contract_class_hash_felt,
        Felt252::from(selector_contract),
        1.into(),
        2.into(),
    ]
    .to_vec();

    // set up remaining structures

    let invoke = InvokeFunction::new(
        account_address,
        selector,
        Default::default(),
        *TRANSACTION_VERSION,
        calldata,
        vec![],
        *block_context.starknet_os_config().chain_id(),
        Some(0.into()),
    )
    .unwrap();

    let tx = Transaction::InvokeFunction(invoke);
    let exec_info = tx
        .execute(
            &mut state,
            &block_context,
            u128::MAX,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .expect("failed to invoke");
    let call_info = exec_info.call_info.as_ref().unwrap();

    assert_eq!(exec_info.revert_error, None);

    // 482670963043u128 == 'panic'
    assert_eq!(call_info.retdata[0], 482670963043u128.into());
    assert!(call_info.failure_flag);
}
