use cairo_vm::Felt252;
use starknet_in_rust::{
    call_contract,
    definitions::block_context::{BlockContext, StarknetChainId},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, state_api::State, ExecutionResourcesManager,
    },
    transaction::{Address, ClassHash, DeployAccount},
    utils::calculate_sn_keccak,
    CasmContractClass, EntryPointType,
};
use std::sync::Arc;

#[test]
fn test_erc20_cairo2() {
    // data to deploy
    let erc20_class_hash: ClassHash = ClassHash([2; 32]);
    let test_data = include_bytes!("../../../starknet_programs/cairo2/erc20.casm");
    let test_contract_class: CasmContractClass = serde_json::from_slice(test_data).unwrap();

    // Create the deploy contract class
    let program_data = include_bytes!("../../../starknet_programs/cairo2/deploy_erc20.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let entrypoint_selector = &entrypoints.external.first().unwrap().selector;

    // Create state reader with class hash data
    let contract_class_cache = Arc::new(PermanentContractClassCache::default());

    let address = Address(1111.into());
    let class_hash: ClassHash = ClassHash([1; 32]);
    let nonce = Felt252::ZERO;

    contract_class_cache.extend([
        (
            class_hash,
            CompiledClass::Casm {
                casm: Arc::new(contract_class),
                sierra: None,
            },
        ),
        (
            erc20_class_hash,
            CompiledClass::Casm {
                casm: Arc::new(test_contract_class),
                sierra: None,
            },
        ),
    ]);

    let mut state_reader = InMemoryStateReader::default();
    state_reader
        .address_to_class_hash_mut()
        .insert(address.clone(), class_hash);
    state_reader
        .address_to_nonce_mut()
        .insert(address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let mut state = CachedState::new(Arc::new(state_reader), contract_class_cache);

    let name_ = Felt252::from_bytes_be_slice(b"some-token");
    let symbol_ = Felt252::from_bytes_be_slice(b"my-super-awesome-token");
    let decimals_ = Felt252::from(24);
    let initial_supply = Felt252::from(1000);
    let recipient = Felt252::from_dec_str(
        "397149464972449753182583229366244826403270781177748543857889179957856017275",
    )
    .unwrap();
    let erc20_salt = Felt252::from_dec_str("1234").unwrap();
    // arguments of deploy contract
    let calldata = vec![
        Felt252::from_bytes_be(&erc20_class_hash.0),
        erc20_salt,
        recipient,
        name_,
        decimals_,
        initial_supply,
        symbol_,
    ];
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
        100_000_000_000,
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
        1.into(),
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
    let erc20_address = *call_info.call_info.unwrap().retdata.first().unwrap();

    // ACCOUNT 1
    let program_data_account =
        include_bytes!("../../../starknet_programs/cairo2/hello_world_account.casm");
    let contract_class_account: CasmContractClass =
        serde_json::from_slice(program_data_account).unwrap();

    state
        .set_contract_class(
            &ClassHash::from(Felt252::ONE),
            &CompiledClass::Casm {
                casm: Arc::new(contract_class_account),
                sierra: None,
            },
        )
        .unwrap();
    state
        .set_compiled_class_hash(
            &Felt252::from_dec_str("1").unwrap(),
            &Felt252::from_bytes_be(&class_hash.0),
        )
        .unwrap();

    let contract_address_salt = Felt252::from_dec_str(
        "2669425616857739096022668060305620640217901643963991674344872184515580705509",
    )
    .unwrap();

    let internal_deploy_account = DeployAccount::new(
        ClassHash::from(Felt252::from_dec_str("1").unwrap()),
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

    let account_address_1 = internal_deploy_account
        .execute(
            &mut state,
            &Default::default(),
            #[cfg(feature = "cairo-native")]
            None,
        )
        .expect("failed to execute internal_deploy_account")
        .validate_info
        .expect("validate_info missing")
        .contract_address;

    // ACCOUNT 2
    let program_data_account =
        include_bytes!("../../../starknet_programs/cairo2/hello_world_account.casm");
    let contract_class_account: CasmContractClass =
        serde_json::from_slice(program_data_account).unwrap();

    state
        .set_contract_class(
            &ClassHash::from(Felt252::ONE),
            &CompiledClass::Casm {
                casm: Arc::new(contract_class_account),
                sierra: None,
            },
        )
        .unwrap();
    state
        .set_compiled_class_hash(
            &Felt252::from_dec_str("1").unwrap(),
            &Felt252::from_bytes_be(&class_hash.0),
        )
        .unwrap();

    let contract_address_salt = Felt252::from_dec_str("123123123123123").unwrap();

    let internal_deploy_account = DeployAccount::new(
        ClassHash::from(Felt252::from_dec_str("1").unwrap()),
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

    let account_address_2 = internal_deploy_account
        .execute(
            &mut state,
            &Default::default(),
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap()
        .validate_info
        .unwrap()
        .contract_address;

    // TRANSFER
    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"transfer"));
    let calldata = vec![account_address_2.clone().0, Felt252::from(123)];

    let retdata = call_contract(
        erc20_address,
        entrypoint_selector,
        calldata,
        &mut state,
        BlockContext::default(),
        account_address_1.clone(),
        #[cfg(feature = "cairo-native")]
        None,
    )
    .unwrap();

    assert!(retdata.is_empty());

    // GET BALANCE ACCOUNT 1
    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"balance_of"));
    let retdata = call_contract(
        erc20_address,
        entrypoint_selector,
        vec![account_address_1.clone().0],
        &mut state,
        BlockContext::default(),
        account_address_1.clone(),
        #[cfg(feature = "cairo-native")]
        None,
    )
    .unwrap();

    assert_eq!(retdata, vec![877.into()]);

    // GET BALANCE ACCOUNT 2
    let entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"balance_of"));
    let retdata = call_contract(
        erc20_address,
        entrypoint_selector,
        vec![account_address_2.0],
        &mut state,
        BlockContext::default(),
        account_address_1,
        #[cfg(feature = "cairo-native")]
        None,
    )
    .unwrap();

    assert_eq!(retdata, vec![123.into()]);
}
