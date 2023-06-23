#![deny(warnings)]

use cairo_vm::felt::{felt_str, Felt252};
use lazy_static::lazy_static;
use num_traits::Zero;
use starknet_contract_class::EntryPointType;
use starknet_rs::{
    core::contract_address::compute_deprecated_class_hash,
    definitions::{
        block_context::StarknetChainId, constants::CONSTRUCTOR_ENTRY_POINT_SELECTOR,
        transaction_type::TransactionType,
    },
    execution::{CallInfo, CallType, TransactionExecutionInfo},
    services::api::contract_classes::deprecated_contract_class::ContractClass,
    state::in_memory_state_reader::InMemoryStateReader,
    state::{cached_state::CachedState, state_api::State},
    transaction::DeployAccount,
    utils::{felt_to_hash, Address},
    CasmContractClass,
};
use std::path::PathBuf;

lazy_static! {
    static ref TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH: Felt252 = felt_str!("1");
}

#[test]
fn internal_deploy_account() {
    let state_reader = InMemoryStateReader::default();
    let mut state = CachedState::new(state_reader, None, None);

    state.set_contract_classes(Default::default()).unwrap();

    let contract_class = ContractClass::try_from(PathBuf::from(
        "starknet_programs/account_without_validation.json",
    ))
    .unwrap();

    let class_hash = felt_to_hash(&compute_deprecated_class_hash(&contract_class).unwrap());

    state
        .set_contract_class(&class_hash, &contract_class)
        .unwrap();

    let internal_deploy_account = DeployAccount::new(
        class_hash,
        0,
        0.into(),
        Felt252::zero(),
        vec![],
        vec![
            felt_str!(
                "3233776396904427614006684968846859029149676045084089832563834729503047027074"
            ),
            felt_str!(
                "707039245213420890976709143988743108543645298941971188668773816813012281203"
            ),
        ],
        Address(felt_str!(
            "2669425616857739096022668060305620640217901643963991674344872184515580705509"
        )),
        StarknetChainId::TestNet.to_felt(),
        None,
    )
    .unwrap();

    let tx_info = internal_deploy_account
        .execute(&mut state, &Default::default())
        .unwrap();

    assert_eq!(
        tx_info,
        TransactionExecutionInfo::new(
            None,
            Some(CallInfo {
                call_type: Some(CallType::Call),
                contract_address: Address(felt_str!(
                    "3577223136242220508961486249701638158054969090851914040041358274796489907314"
                )),
                class_hash: Some(class_hash),
                entry_point_selector: Some(CONSTRUCTOR_ENTRY_POINT_SELECTOR.clone()),
                entry_point_type: Some(EntryPointType::Constructor),
                ..Default::default()
            }),
            None,
            0,
            [
                ("pedersen_builtin", 23),
                ("range_check_builtin", 74),
                ("l1_gas_usage", 1224)
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            Some(TransactionType::DeployAccount),
        ),
    );
}

#[test]
fn internal_deploy_account_cairo1() {
    let state_reader = InMemoryStateReader::default();
    let mut state = CachedState::new(state_reader, None, Some(Default::default()));

    state.set_contract_classes(Default::default()).unwrap();

    let program_data = include_bytes!("../starknet_programs/cairo1/hello_world_account.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    state
        .set_compiled_class(
            &TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH.clone(),
            contract_class,
        )
        .unwrap();

    let contract_address_salt =
        felt_str!("2669425616857739096022668060305620640217901643963991674344872184515580705509");

    let internal_deploy_account = DeployAccount::new(
        TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH
            .clone()
            .to_be_bytes(),
        0,
        1.into(),
        Felt252::zero(),
        vec![2.into()],
        vec![
            felt_str!(
                "3233776396904427614006684968846859029149676045084089832563834729503047027074"
            ),
            felt_str!(
                "707039245213420890976709143988743108543645298941971188668773816813012281203"
            ),
        ],
        Address(contract_address_salt),
        StarknetChainId::TestNet.to_felt(),
        None,
    )
    .unwrap();

    let tx_info = internal_deploy_account
        .execute(&mut state, &Default::default())
        .unwrap();
    let bytes = felt_str!("24944740430830204917365432020251520094789").to_be_bytes();
    let ret = std::str::from_utf8(&bytes).unwrap();
    let s = String::from(ret);
    dbg!(s);

    assert_eq!(
        tx_info,
        TransactionExecutionInfo::new(
            None,
            Some(CallInfo {
                call_type: Some(CallType::Call),
                contract_address: Address(felt_str!(
                    "3577223136242220508961486249701638158054969090851914040041358274796489907314"
                )),
                class_hash: Some(
                    TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH
                        .clone()
                        .to_be_bytes()
                ),
                entry_point_selector: Some(CONSTRUCTOR_ENTRY_POINT_SELECTOR.clone()),
                entry_point_type: Some(EntryPointType::Constructor),
                ..Default::default()
            }),
            None,
            1000000,
            [
                ("pedersen_builtin", 23),
                ("range_check_builtin", 74),
                ("l1_gas_usage", 1224)
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            Some(TransactionType::DeployAccount),
        ),
    );
}
