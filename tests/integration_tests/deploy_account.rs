use cairo_vm::types::builtin_name::BuiltinName;
use cairo_vm::{vm::runners::cairo_runner::ExecutionResources, Felt252};
use lazy_static::lazy_static;
use pretty_assertions_sorted::assert_eq;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    core::contract_address::compute_deprecated_class_hash,
    definitions::{
        block_context::StarknetChainId,
        constants::{CONSTRUCTOR_ENTRY_POINT_SELECTOR, VALIDATE_DEPLOY_ENTRY_POINT_SELECTOR},
        transaction_type::TransactionType,
    },
    execution::{CallInfo, CallType, TransactionExecutionInfo},
    hash_utils::calculate_contract_address,
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
    },
    state::{
        cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, state_api::State,
    },
    transaction::{Address, ClassHash, DeployAccount},
    CasmContractClass,
};
use std::{collections::HashSet, sync::Arc};

lazy_static! {
    static ref TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH: Felt252 =
        Felt252::from_dec_str("1").unwrap();
}

#[test]
fn internal_deploy_account() {
    let state_reader = Arc::new(InMemoryStateReader::default());
    let mut state = CachedState::new(
        state_reader,
        Arc::new(PermanentContractClassCache::default()),
    );

    let contract_class =
        ContractClass::from_path("starknet_programs/account_without_validation.json").unwrap();

    let class_hash_felt = compute_deprecated_class_hash(&contract_class).unwrap();
    let class_hash = ClassHash::from(class_hash_felt);

    state
        .set_contract_class(
            &class_hash,
            &CompiledClass::Deprecated(Arc::new(contract_class)),
        )
        .unwrap();

    let contract_address_salt = Felt252::from_dec_str(
        "2669425616857739096022668060305620640217901643963991674344872184515580705509",
    )
    .unwrap();

    let internal_deploy_account = DeployAccount::new(
        class_hash,
        Default::default(),
        1.into(),
        Felt252::ZERO,
        vec![],
        vec![
            Felt252::from_dec_str(
                "3233776396904427614006684968846859029149676045084089832563834729503047027074",
            )
            .unwrap(),
            Felt252::from_dec_str(
                "707039245213420890976709143988743108543645298941971188668773816813012281203",
            )
            .unwrap(),
        ],
        contract_address_salt,
        StarknetChainId::TestNet.to_felt(),
    )
    .unwrap();

    let tx_info = internal_deploy_account
        .execute(
            &mut state,
            &Default::default(),
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    let contract_address = calculate_contract_address(
        &contract_address_salt,
        &class_hash_felt,
        &[],
        Address(Felt252::ZERO),
    )
    .unwrap();

    assert_eq!(
        tx_info,
        TransactionExecutionInfo::new(
            Some(CallInfo {
                call_type: Some(CallType::Call),
                contract_address: Address(contract_address),
                class_hash: Some(class_hash),
                entry_point_selector: Some(*VALIDATE_DEPLOY_ENTRY_POINT_SELECTOR),
                entry_point_type: Some(EntryPointType::External),
                calldata: vec![Felt252::from_bytes_be(&class_hash.0), contract_address_salt],
                execution_resources: Some(ExecutionResources {
                    n_steps: 13,
                    n_memory_holes: 0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            Some(CallInfo {
                call_type: Some(CallType::Call),
                contract_address: Address(contract_address),
                class_hash: Some(class_hash),
                entry_point_selector: Some(*CONSTRUCTOR_ENTRY_POINT_SELECTOR),
                entry_point_type: Some(EntryPointType::Constructor),
                ..Default::default()
            }),
            None,
            None,
            0,
            [
                ("n_steps", 3893),
                ("pedersen_builtin", 23),
                ("range_check_builtin", 83),
                ("l1_gas_usage", 2203)
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
    let state_reader = Arc::new(InMemoryStateReader::default());
    let mut state = CachedState::new(
        state_reader,
        Arc::new(PermanentContractClassCache::default()),
    );
    let program_data = include_bytes!("../../starknet_programs/cairo2/hello_world_account.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();

    state
        .set_contract_class(
            &ClassHash(TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH.to_bytes_be()),
            &CompiledClass::Casm {
                casm: Arc::new(contract_class),
                sierra: None,
            },
        )
        .unwrap();
    state
        .set_compiled_class_hash(
            &TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH,
            &TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH,
        )
        .unwrap();

    let contract_address_salt = Felt252::from_dec_str(
        "2669425616857739096022668060305620640217901643963991674344872184515580705509",
    )
    .unwrap();

    let internal_deploy_account = DeployAccount::new(
        ClassHash(TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH.to_bytes_be()),
        Default::default(),
        1.into(),
        Felt252::ZERO,
        vec![2.into()],
        vec![
            Felt252::from_dec_str(
                "3233776396904427614006684968846859029149676045084089832563834729503047027074",
            )
            .unwrap(),
            Felt252::from_dec_str(
                "707039245213420890976709143988743108543645298941971188668773816813012281203",
            )
            .unwrap(),
        ],
        contract_address_salt,
        StarknetChainId::TestNet.to_felt(),
    )
    .unwrap();

    let tx_info = internal_deploy_account
        .execute(
            &mut state,
            &Default::default(),
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    let accessed_keys: ClassHash = ClassHash([
        3, 178, 128, 25, 204, 253, 189, 48, 255, 198, 89, 81, 217, 75, 184, 92, 158, 43, 132, 52,
        17, 26, 0, 11, 90, 253, 83, 60, 230, 95, 87, 164,
    ]);
    let keys: HashSet<ClassHash> = [accessed_keys].iter().copied().collect();

    let n_steps = 4262;

    assert_eq!(
        tx_info,
        TransactionExecutionInfo::new(
            Some(CallInfo {
                caller_address: Address(0.into()),
                call_type: Some(CallType::Call),
                contract_address: Address(Felt252::from_dec_str("397149464972449753182583229366244826403270781177748543857889179957856017275").unwrap()),
                code_address: None,
                gas_consumed: 15340,
                class_hash: Some(ClassHash([
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 1
                ])),
                entry_point_selector: Some(Felt252::from_dec_str(
                    "1554466106298962091002569854891683800203193677547440645928814916929210362005"
                ).unwrap()),
                entry_point_type: Some(EntryPointType::External),
                calldata: vec![
                    1.into(),
                   Felt252::from_dec_str("2669425616857739096022668060305620640217901643963991674344872184515580705509").unwrap(),
                    2.into()
                ],
                retdata: vec![Felt252::from_dec_str("370462705988").unwrap()],
                execution_resources: Some(ExecutionResources {
                    n_steps: 142,
                    n_memory_holes: 2,
                    builtin_instance_counter:
                    [
                    (BuiltinName::range_check, 2),
                    ]
                .into_iter()
                .collect(),
            }),

                ..Default::default() }),

            Some(CallInfo {
                call_type: Some(CallType::Call),
                contract_address: Address(Felt252::from_dec_str("397149464972449753182583229366244826403270781177748543857889179957856017275").unwrap()),
                class_hash: Some(
                    ClassHash(TEST_ACCOUNT_COMPILED_CONTRACT_CLASS_HASH.to_bytes_be()),

                ),
                entry_point_selector: Some(Felt252::from_dec_str("1159040026212278395030414237414753050475174923702621880048416706425641521556").unwrap()),
                entry_point_type: Some(EntryPointType::Constructor),
                gas_consumed: 13740,
                calldata: vec![2.into()],
                accessed_storage_keys: keys,
                execution_resources: Some(ExecutionResources {
                    n_steps: 87,
                    n_memory_holes: 0,
                    builtin_instance_counter:
                    [
                        (BuiltinName::range_check, 2),
                    ]
                .into_iter()
                .collect(),
            }),
                ..Default::default()
            }),
            None,
            None,
            0,
            [
                ("n_steps", n_steps),
                ("pedersen_builtin", 23),
                ("range_check_builtin", 89),
                ("l1_gas_usage", 4407)
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            Some(TransactionType::DeployAccount),
        ),
    );
}
