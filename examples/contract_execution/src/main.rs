#![deny(warnings)]

//! A simple example of starknet-rs use.
//!
//! In [`test_contract`] we have all the interaction with the crate's API.
//! In [`main`] we use it to run a compiled contract's entrypoint and print
//! the returned data.
//!
//! It also includes some small tests that assert the data returned by
//! running some pre-compiled contracts is as expected.

use cairo_vm::Felt252;
use starknet_in_rust::{
    core::contract_address::{compute_casm_class_hash, compute_deprecated_class_hash},
    definitions::block_context::BlockContext,
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
    },
    state::{
        cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, state_api::State,
    },
    transaction::{Address, Declare, DeployAccount, InvokeFunction},
    utils::{calculate_sn_keccak, felt_to_hash},
    CasmContractClass, SierraContractClass,
};
use std::{fs::File, io::BufReader, path::Path, str::FromStr, sync::Arc};

fn main() {
    // replace this with the path to your compiled contract
    let contract_path = "../../starknet_programs/cairo2/fibonacci.sierra";

    // replace this with the name of your entrypoint
    let entry_point: &str = "fib";

    // replace this with the arguments for the entrypoint
    let calldata: Vec<Felt252> = [1.into(), 1.into(), 10.into()].to_vec();

    let retdata = test_contract(contract_path, entry_point, calldata);

    let result_strs: Vec<String> = retdata.iter().map(Felt252::to_string).collect();
    let joined_str = result_strs.join(", ");

    println!("The returned values were: {joined_str}");
}

/// This function:
///  - deploys an account
///  - declares a new contract class
///  - deploys a new contract
///  - executes the given entry point in the deployed contract
fn test_contract(
    contract_path: impl AsRef<Path>,
    entry_point: &str,
    call_data: Vec<Felt252>,
) -> Vec<Felt252> {
    //* --------------------------------------------
    //*             Initialize needed variables
    //* --------------------------------------------
    let block_context = BlockContext::default();
    // Values hardcoded to pass signature validation
    let signature = vec![
        Felt252::from_dec_str(
            "3086480810278599376317923499561306189851900463386393948998357832163236918254",
        )
        .unwrap(),
        Felt252::from_dec_str(
            "598673427589502599949712887611119751108407514580626464031881322743364689811",
        )
        .unwrap(),
    ];

    //* --------------------------------------------
    //*             Initialize state
    //* --------------------------------------------
    let state_reader = Arc::new(InMemoryStateReader::default());
    let mut state = CachedState::new(
        state_reader,
        Arc::new(PermanentContractClassCache::default()),
    );

    //* --------------------------------------------
    //*             Deploy deployer contract
    //* --------------------------------------------
    let deployer_contract =
        ContractClass::from_str(include_str!("../../../starknet_programs/deployer.json")).unwrap();
    let deployer_contract_address = Address(Felt252::from(17));
    let deployer_contract_class_hash =
        felt_to_hash(&compute_deprecated_class_hash(&deployer_contract).unwrap());
    state
        .set_contract_class(
            &deployer_contract_class_hash,
            &CompiledClass::Deprecated(Arc::new(deployer_contract)),
        )
        .unwrap();
    state
        .deploy_contract(
            deployer_contract_address.clone(),
            deployer_contract_class_hash,
        )
        .expect("Failed to deploy deployer contract");

    //* --------------------------------------------
    //*             Deploy Account contract
    //* --------------------------------------------
    let account_contract =
        ContractClass::from_str(include_str!("../../../starknet_programs/Account.json")).unwrap();
    let account_contract_class_hash = felt_to_hash(&Felt252::from(1));
    state
        .set_contract_class(
            &account_contract_class_hash,
            &CompiledClass::Deprecated(Arc::new(account_contract)),
        )
        .unwrap();

    let internal_deploy = DeployAccount::new_with_tx_hash(
        account_contract_class_hash,
        Default::default(),
        1.into(),
        0.into(),
        // Values hardcoded to pass signature validation
        vec![Felt252::from_dec_str(
            "1735102664668487605176656616876767369909409133946409161569774794110049207117",
        )
        .unwrap()],
        signature.clone(),
        Felt252::from_dec_str(
            "2669425616857739096022668060305620640217901643963991674344872184515580705509",
        )
        .unwrap(),
        2718.into(),
    )
    .unwrap();

    let account_contract_address = internal_deploy
        .execute(
            &mut state,
            &block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .expect("Account Deploy Failed")
        .call_info
        .unwrap()
        .contract_address
        .clone();

    //* --------------------------------------------
    //*          Read contract from file
    //* --------------------------------------------
    let file = File::open(contract_path).unwrap();
    let reader = BufReader::new(file);
    let sierra_contract_class: SierraContractClass =
        serde_json::from_reader(reader).expect("Could not load contract from JSON");
    let casm_class =
        CasmContractClass::from_contract_class(sierra_contract_class.clone(), false, usize::MAX)
            .unwrap();
    let compiled_class_hash =
        compute_casm_class_hash(&casm_class).expect("Error computing sierra class hash");
    //* --------------------------------------------
    //*        Declare new contract class
    //* --------------------------------------------
    let declare_tx = Declare::new_with_tx_hash(
        &sierra_contract_class,
        Some(casm_class),
        compiled_class_hash,
        account_contract_address.clone(),
        Default::default(), // max fee
        2.into(),
        signature.clone(),
        1.into(), // nonce
        // Value hardcoded to pass signature validation
        2718.into(),
    )
    .expect("couldn't create declare transaction");

    declare_tx
        .execute(
            &mut state,
            &block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .expect("could not declare the contract class");

    //* ----------------------------------------------------------
    //*     Deploy new contract class instance through the deployer
    //* -----------------------------------------------------------

    let deploy = InvokeFunction::new(
        deployer_contract_address,
        Felt252::from_bytes_be(&calculate_sn_keccak("deploy_contract".as_bytes())),
        Default::default(),
        0.into(),
        vec![compiled_class_hash, 3.into(), 0.into()], // call data
        signature.clone(),
        *block_context.starknet_os_config().chain_id(),
        None,
    )
    .unwrap();

    let contract_address = deploy
        .execute(
            &mut state,
            &block_context,
            0,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .expect("could not deploy contract")
        .call_info
        .unwrap()
        .retdata[0];

    //* ---------------------------------------------------------
    //*        Execute contract entrypoint through the account
    //* ---------------------------------------------------------
    let entry_point_selector = Felt252::from_bytes_be(&calculate_sn_keccak(entry_point.as_bytes()));
    let mut account_execute_calldata = vec![
        // call_array_len: felt
        1.into(),
        // call_array: CallArray*
        // struct CallArray {
        //     to: felt,
        contract_address,
        //     selector: felt,
        entry_point_selector,
        //     data_offset: felt,
        0.into(),
        //     data_len: felt,
        call_data.len().into(),
        // }
        // calldata_len: felt
        call_data.len().into(),
    ];
    // calldata: felt*
    account_execute_calldata.extend(call_data);
    let invoke_tx = InvokeFunction::new_with_tx_hash(
        account_contract_address,
        Felt252::from_bytes_be(&calculate_sn_keccak("__execute__".as_bytes())),
        Default::default(),
        1.into(),
        account_execute_calldata,
        signature,
        Some(2.into()),
        // Value hardcoded to pass signature validation
        2718.into(),
    )
    .unwrap();

    let tx_exec_info = invoke_tx
        .execute(
            &mut state,
            &block_context,
            0,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    //* --------------------------------------------
    //*          Extract return values
    //* --------------------------------------------
    tx_exec_info
        .call_info
        .expect("call info should exist")
        .retdata
}

#[cfg(test)]
mod tests {
    use crate::test_contract;

    #[test]
    fn test_example_contract() {
        let retdata = test_contract(
            "../../starknet_programs/cairo2/example_contract.sierra",
            "get_balance",
            [].to_vec(),
        );
        assert_eq!(retdata, vec![0.into()]);
    }

    #[test]
    fn test_fibonacci() {
        let retdata = test_contract(
            "../../starknet_programs/cairo2/fibonacci.sierra",
            "fib",
            [1.into(), 1.into(), 10.into()].to_vec(),
        );
        assert_eq!(retdata, vec![89.into()]);
    }

    #[test]
    fn test_factorial() {
        let retdata = test_contract(
            "../../starknet_programs/cairo2/factorial.sierra",
            "factorial",
            [10.into()].to_vec(),
        );
        assert_eq!(retdata, vec![3628800.into()]);
    }
}
