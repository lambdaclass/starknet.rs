#![deny(warnings)]

#[macro_use]
extern crate honggfuzz;

use cairo_vm::{vm::runners::cairo_runner::ExecutionResources, Felt252};
use starknet_in_rust::execution::execution_entry_point::ExecutionResult;
use starknet_in_rust::transaction::ClassHash;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
    },
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        ExecutionResourcesManager,
    },
    transaction::Address,
    utils::calculate_sn_keccak,
};
use std::{
    collections::HashSet, fs, path::PathBuf, process::Command, sync::Arc, thread, time::Duration,
};

fn main() {
    println!("Starting fuzzer");
    let mut iteration = 0;
    fs::create_dir("fuzzer/cairo_programs").expect("Failed to create cairo_programs/ directory");
    loop {
        fuzz!(|data: &[u8]| {
            iteration += 1;

            // ---------------------------------------------------------
            //  Create the content of the .cairo file with a function that writes the random data in the counter
            // ---------------------------------------------------------
            let file_content1 = "
            %lang starknet
            from starkware.cairo.common.cairo_builtins import HashBuiltin

            @storage_var
            func _counter() -> (res: felt) {
            }

            @external
            func write_and_read{syscall_ptr: felt*, pedersen_ptr: HashBuiltin*, range_check_ptr}() -> (res:felt) {
                _counter.write('";

            let input = data_to_ascii(data);

            let file_content2 = "');
                return _counter.read();
            }
            ";
            let file_name = format!("cairo_programs/output-{iteration}.cairo");

            let file_content = file_content1.to_owned() + &input + file_content2;

            println!("{file_content:?}");
            // ---------------------------------------------------------
            //  Create the .cairo file
            // ---------------------------------------------------------

            fs::write(file_name, file_content.as_bytes())
                .expect("Failed to write generated cairo program");

            // ---------------------------------------------------------
            //  Compile the .cairo file to create the .json file
            // ---------------------------------------------------------

            let cairo_file_name = format!("cairo_programs/output-{iteration}.cairo");
            let json_file_name = format!("cairo_programs/output-{iteration}.json");

            let _output = Command::new("starknet-compile")
                .arg(cairo_file_name.clone())
                .arg("--output")
                .arg(json_file_name.clone())
                .output()
                .expect("failed to execute process");

            // ---------------------------------------------------------
            //  Create program and entry point types for contract class
            // ---------------------------------------------------------

            let path = PathBuf::from(&json_file_name);
            let contract_class = ContractClass::from_path(path).unwrap();

            let storage_entrypoint_selector = *contract_class
                .entry_points_by_type()
                .get(&EntryPointType::External)
                .unwrap()
                .first()
                .unwrap()
                .selector();

            fs::remove_file(cairo_file_name).expect("Failed to remove generated cairo source");
            fs::remove_file(json_file_name)
                .expect("Failed to remove generated cairo compiled program");

            //* --------------------------------------------
            //*    Create state reader with class hash data
            //* --------------------------------------------

            let contract_class_cache = PermanentContractClassCache::default();

            //  ------------ contract data --------------------

            let address = Address(1111.into());
            let class_hash: ClassHash = ClassHash([1; 32]);

            contract_class_cache.set_contract_class(
                class_hash,
                CompiledClass::Deprecated(Arc::new(contract_class)),
            );
            let mut state_reader = InMemoryStateReader::default();
            state_reader
                .address_to_class_hash_mut()
                .insert(address.clone(), class_hash);

            //* ---------------------------------------
            //*    Create state with previous data
            //* ---------------------------------------

            let mut state =
                CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

            //* ------------------------------------
            //*    Create execution entry point
            //* ------------------------------------

            let calldata = [].to_vec();
            let caller_address = Address(0000.into());
            let entry_point_type = EntryPointType::External;

            let exec_entry_point = ExecutionEntryPoint::new(
                address.clone(),
                calldata.clone(),
                storage_entrypoint_selector,
                caller_address,
                entry_point_type,
                Some(CallType::Delegate),
                Some(class_hash),
                0,
            );

            //* --------------------
            //*   Execute contract
            //* ---------------------
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

            let expected_key_bytes = calculate_sn_keccak("_counter".as_bytes());
            let expected_key = ClassHash(expected_key_bytes);

            let mut expected_accessed_storage_keys = HashSet::new();
            expected_accessed_storage_keys.insert(expected_key);

            let expected_call_info = CallInfo {
                caller_address: Address(0.into()),
                call_type: Some(CallType::Delegate),
                contract_address: Address(1111.into()),
                entry_point_selector: Some(storage_entrypoint_selector),
                entry_point_type: Some(EntryPointType::External),
                calldata,
                retdata: [Felt252::from_bytes_be_slice(data_to_ascii(data).as_bytes())].to_vec(),
                execution_resources: Some(ExecutionResources::default()),
                class_hash: Some(class_hash),
                storage_read_values: vec![Felt252::from_bytes_be_slice(
                    data_to_ascii(data).as_bytes(),
                )],
                accessed_storage_keys: expected_accessed_storage_keys,
                ..Default::default()
            };

            let ExecutionResult { call_info, .. } = exec_entry_point
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
            assert_eq!(call_info.unwrap(), expected_call_info);

            assert!(!state.cache().storage_writes().is_empty());
            assert_eq!(
                state
                    .cache()
                    .storage_writes()
                    .get(&(address, expected_key_bytes))
                    .cloned(),
                Some(Felt252::from_bytes_be_slice(data_to_ascii(data).as_bytes()))
            );
        });
        thread::sleep(Duration::from_secs(1));
    }
}

fn data_to_ascii(data: &[u8]) -> String {
    let data_string = String::from_utf8_lossy(data);
    let mut chars = Vec::new();
    for i in data_string.chars() {
        if !i.is_ascii() {
            chars.push('X');
        } else if (i as u32) < 40 {
            let num_string = (i as u32).to_string();
            for j in num_string.chars() {
                chars.push(j);
            }
        } else {
            chars.push(i);
        };
    }

    let mut data_ascii: String = chars.iter().collect();
    data_ascii.truncate(30);
    data_ascii
}
