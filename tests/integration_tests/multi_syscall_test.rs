use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;
use cairo_vm::Felt252;

use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, OrderedEvent,
        OrderedL2ToL1Message, TransactionExecutionContext,
    },
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        ExecutionResourcesManager,
    },
    transaction::{Address, ClassHash},
    utils::calculate_sn_keccak,
    EntryPointType,
};
use std::{sync::Arc, vec};

#[test]
fn test_multiple_syscall() {
    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/multi_syscall_test.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();

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

    // Block for get_caller_address.
    {
        let call_info = test_syscall(
            "caller_address",
            address.clone(),
            vec![],
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(call_info.retdata, vec![caller_address.clone().0])
    }

    // Block for get_contract_address.
    {
        let call_info = test_syscall(
            "contract_address",
            address.clone(),
            vec![],
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(call_info.retdata, vec![address.clone().0])
    }
    // Block for get_execution_info_syscall.
    {
        let call_info = test_syscall(
            "execution_info_syscall",
            address.clone(),
            calldata.clone(),
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(call_info.retdata, vec![0.into(), 1111.into()]);
    }

    // Block for library_call_syscall
    {
        let entrypoint_selector =
            Felt252::from_bytes_be(&calculate_sn_keccak("get_number".as_bytes()));
        let new_call_data = vec![
            Felt252::from_bytes_be(&class_hash.0),
            entrypoint_selector,
            Felt252::from(25),
        ];
        let call_info = test_syscall(
            "test_library_call_syscall",
            address.clone(),
            new_call_data,
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(call_info.retdata, vec![25.into()])
    }

    // Block for call_contract_syscall
    {
        let entrypoint_selector =
            Felt252::from_bytes_be(&calculate_sn_keccak("get_number".as_bytes()));
        let new_call_data = vec![entrypoint_selector, Felt252::from(25)];
        let call_info = test_syscall(
            "test_call_contract_syscall",
            address.clone(),
            new_call_data,
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(call_info.retdata, vec![25.into()])
    }

    // Block for send_message_to_l1_syscall
    {
        let new_call_data = vec![2222.into(), Felt252::from(25), Felt252::from(30)];
        let call_info = test_syscall(
            "test_send_message_to_l1",
            address.clone(),
            new_call_data,
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(
            call_info.l2_to_l1_messages,
            vec![OrderedL2ToL1Message {
                order: 0,
                to_address: Address(2222.into()),
                payload: vec![Felt252::from(25), Felt252::from(30)],
            },]
        )
    }

    // Block for read write
    {
        let call_info = test_syscall(
            "read",
            address.clone(),
            calldata.clone(),
            caller_address.clone(),
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(
            call_info.retdata,
            vec![Felt252::from_dec_str("310939249775").unwrap()]
        )
    }

    // Block for emit
    {
        let call_info = test_syscall(
            "trigger_events",
            address,
            calldata,
            caller_address,
            entry_point_type,
            class_hash,
            &mut state,
        );
        assert_eq!(
            call_info.events,
            vec![
                OrderedEvent {
                    order: 0,
                    keys: vec![Felt252::from_dec_str(
                        "826422450673657747090149602083997624297692992347360843320687877601002682120"
                    )
                    .unwrap()],
                    data: vec![1.into()]
                },
                OrderedEvent {
                    order: 1,
                    keys: vec![Felt252::from_dec_str(
                        "826422450673657747090149602083997624297692992347360843320687877601002682120"
                    )
                    .unwrap()],
                    data: vec![2.into()]
                },
                OrderedEvent {
                    order: 2,
                    keys: vec![Felt252::from_dec_str(
                        "826422450673657747090149602083997624297692992347360843320687877601002682120"
                    )
                    .unwrap()],
                    data: vec![3.into()]
                }
            ]
        )
    }
}

fn test_syscall(
    entrypoint_selector: &str,
    address: Address,
    calldata: Vec<Felt252>,
    caller_address: Address,
    entry_point_type: EntryPointType,
    class_hash: ClassHash,
    state: &mut CachedState<InMemoryStateReader, PermanentContractClassCache>,
) -> CallInfo {
    let entrypoint_selector =
        Felt252::from_bytes_be(&calculate_sn_keccak(entrypoint_selector.as_bytes()));
    let exec_entry_point = ExecutionEntryPoint::new(
        address,
        calldata,
        entrypoint_selector,
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
    exec_entry_point
        .execute(
            state,
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
        .unwrap()
}
