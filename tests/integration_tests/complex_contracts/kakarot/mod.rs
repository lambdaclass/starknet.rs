use std::str::FromStr;

use cairo_vm::Felt252;
use reth_primitives::{sign_message, AccessList, Bytes, Signature, TransactionSigned, B256, U256};
use starknet::core::utils::{get_contract_address, get_storage_var_address, starknet_keccak};
use starknet_api::core::L2_ADDRESS_UPPER_BOUND;
use starknet_crypto::{poseidon_hash_many, FieldElement};
use starknet_in_rust::definitions::block_context::{FeeTokenAddresses, StarknetOsConfig};
use starknet_in_rust::definitions::constants::EXECUTE_ENTRY_POINT_SELECTOR;
use starknet_in_rust::transaction::Address;
use starknet_in_rust::transaction::{
    ClassHash, InvokeFunction, Transaction, VersionSpecificAccountTxFields,
};
use starknet_in_rust::utils::{calculate_sn_keccak, felt_to_field_element, field_element_to_felt};

use crate::integration_tests::cairo_native::TestStateSetup;

#[test]
// #[ignore = "Failed to deserialize param #1: see PR `https://github.com/lambdaclass/cairo_native/pull/630`"]
fn test_kakarot_contract() {
    // Evm constants
    let private_key: B256 =
        B256::from_str("0x6ae82d865482a203603ecbf25c865e082396d7705a6bbce92c1ff1d6ab9b503c")
            .unwrap();
    let public_key: reth_primitives::Address =
        reth_primitives::Address::from_str("0x7513A12F74fFF533ee12F20EE524e4883CBd1945").unwrap();
    let contract_address: reth_primitives::Address =
        reth_primitives::Address::from_str("0x0000000000000000000000000000000000001234").unwrap();

    // Starknet constants
    let chain_id = Felt252::from_hex("0x4B4B5254").unwrap();

    let kakarot_class_hash = ClassHash([1; 32]);
    let kakarot_address = Address(1.into());
    let uninitialized_account_class_hash = ClassHash([3; 32]);

    // EOA
    let eoa_nonce = Felt252::ZERO;
    let eoa_evm_address = Felt252::from_bytes_be_slice(public_key.0.as_slice());
    let eoa_class_hash = ClassHash([2; 32]);
    let eoa_address = compute_starknet_address(
        &eoa_evm_address,
        &kakarot_address.0,
        &Felt252::from_bytes_be(&uninitialized_account_class_hash.0),
    );
    let eoa_address = Address(eoa_address);

    // Contract Account
    let contract_nonce = Felt252::ZERO;
    let contract_account_evm_address = Felt252::from_bytes_be_slice(contract_address.0.as_slice());
    let contract_account_class_hash = ClassHash([3; 32]);
    let contract_account_address = compute_starknet_address(
        &contract_account_evm_address,
        &kakarot_address.0,
        &Felt252::from_bytes_be(&uninitialized_account_class_hash.0),
    );
    let contract_account_address = Address(contract_account_address);
    // PUSH1 01 PUSH1 00 SSTORE PUSH1 02 PUSH1 00 MSTORE8 PUSH1 01 PUSH1 00 RETURN
    let bytecode = Felt252::from_bytes_be_slice(&[
        0x60, 0x01, 0x60, 0x00, 0x55, 0x60, 0x02, 0x60, 0x00, 0x53, 0x60, 0x01, 0x60, 0x00, 0xf3,
    ]);
    let bytecode_base_address = compute_storage_key("Account_bytecode", &[]);

    let bytecode_storage_key = field_element_to_felt(&poseidon_hash_many(&[
        felt_to_field_element(&bytecode_base_address).unwrap(),
        FieldElement::ZERO,
    ]));

    let fee_token_address = Address(
        Felt252::from_hex("0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7")
            .unwrap(),
    );

    let mut state = TestStateSetup::default();

    dbg!("deploy");

    // Deploy Kakarot
    state
        .load_contract_at_address(
            kakarot_class_hash,
            kakarot_address.clone(),
            "starknet_programs/kakarot/contracts_KakarotCore.cairo",
        )
        .unwrap();
    dbg!("deploy eoa");
    // Deploy EOA
    state
        .load_contract_at_address(
            eoa_class_hash,
            eoa_address.clone(),
            "starknet_programs/kakarot/contracts_AccountContract.cairo",
        )
        .unwrap();
    dbg!("deploy contract account");
    // Deploy Contract Account
    state
        .load_contract_at_address(
            contract_account_class_hash,
            contract_account_address.clone(),
            "starknet_programs/kakarot/contracts_AccountContract.cairo",
        )
        .unwrap();

        dbg!("uninit account");
    // Declare uninitialized account class hash
    state
        .load_contract(
            uninitialized_account_class_hash,
            "starknet_programs/kakarot/contracts_UninitializedAccount.cairo",
        )
        .unwrap();

        dbg!("perepare storage");
    // Prepare storage for Kakarot
    let kakarot_storage = vec![
        // Add the EOA to the address registry
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_evm_to_starknet_address", &[eoa_evm_address]),
            ),
            eoa_address.0,
        ),
        (
            // Add the contract account to the address registry
            (
                kakarot_address.clone(),
                compute_storage_key(
                    "Kakarot_evm_to_starknet_address",
                    &[contract_account_evm_address],
                ),
            ),
            contract_account_address.0,
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_uninitialized_account_class_hash", &[]),
            ),
            Felt252::from_bytes_be(&uninitialized_account_class_hash.0),
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_account_contract_class_hash", &[]),
            ),
            Felt252::from_bytes_be(&contract_account_class_hash.0),
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_native_token_address", &[]),
            ),
            fee_token_address.0,
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_coinbase", &[]),
            ),
            eoa_address.0,
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_base_fee", &[]),
            ),
            Felt252::ONE,
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_prev_randao", &[]),
            ),
            Felt252::ZERO,
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Kakarot_block_gas_limit", &[]),
            ),
            Felt252::MAX,
        ),
        (
            (
                kakarot_address.clone(),
                compute_storage_key("Ownable_owner", &[]),
            ),
            eoa_address.0,
        ),
    ];

    // Prepare storage for EOA
    let eoa_storage = vec![
        // Set the bytecode
        ((eoa_address.clone(), bytecode_storage_key), Felt252::ZERO),
        (
            (
                eoa_address.clone(),
                compute_storage_key("Account_storage", &[]),
            ),
            Felt252::ZERO,
        ),
        (
            (
                eoa_address.clone(),
                compute_storage_key("Account_is_initialized", &[]),
            ),
            Felt252::ONE,
        ),
        (
            (
                eoa_address.clone(),
                compute_storage_key("Account_nonce", &[]),
            ),
            eoa_nonce,
        ),
        (
            (
                eoa_address.clone(),
                compute_storage_key("Account_implementation", &[]),
            ),
            Felt252::from_bytes_be(&eoa_class_hash.0),
        ),
        (
            (
                eoa_address.clone(),
                compute_storage_key("Account_evm_address", &[]),
            ),
            eoa_address.0,
        ),
        (
            (
                eoa_address.clone(),
                compute_storage_key("Ownable_owner", &[]),
            ),
            kakarot_address.0,
        ),
    ];

    // Prepare storage for Contract Account
    let contract_account_storage = vec![
        // Set the bytecode
        (
            (contract_account_address.clone(), bytecode_storage_key),
            bytecode,
        ),
        (
            (
                contract_account_address.clone(),
                compute_storage_key("Account_storage", &[]),
            ),
            Felt252::ZERO,
        ),
        (
            (
                contract_account_address.clone(),
                compute_storage_key("Account_is_initialized", &[]),
            ),
            Felt252::ONE,
        ),
        (
            (
                contract_account_address.clone(),
                compute_storage_key("Account_nonce", &[]),
            ),
            contract_nonce,
        ),
        (
            (
                contract_account_address.clone(),
                compute_storage_key("Account_implementation", &[]),
            ),
            Felt252::from_bytes_be(&kakarot_class_hash.0),
        ),
        (
            (
                contract_account_address.clone(),
                compute_storage_key("Account_evm_address", &[]),
            ),
            contract_account_evm_address,
        ),
        (
            (
                contract_account_address.clone(),
                compute_storage_key("Ownable_owner", &[]),
            ),
            kakarot_address.0,
        ),
    ];

    // Set the initial storage
    let mut state = state.finalize_with_starknet_os_config(StarknetOsConfig::new(
        chain_id,
        FeeTokenAddresses::new(fee_token_address.clone(), fee_token_address),
        Default::default(),
    ));

    let storage = [kakarot_storage, eoa_storage, contract_account_storage].concat();
    for (k, v) in storage {
        state.insert_initial_storage_value((k.0, k.1.to_bytes_be()), v);
    }

    // Prepare the evm transaction
    let mut transaction = TransactionSigned {
        hash: B256::default(),
        signature: Signature::default(),
        transaction: reth_primitives::Transaction::Eip1559(reth_primitives::TxEip1559 {
            chain_id: *chain_id.to_le_digits().first().unwrap(),
            nonce: 0,
            gas_limit: 1_000_000,
            max_fee_per_gas: 0,
            max_priority_fee_per_gas: 0,
            to: reth_primitives::TransactionKind::Call(contract_address),
            value: U256::from(u8::MIN),
            access_list: AccessList::default(),
            input: Bytes::default(),
        }),
    };
    let signature = sign_message(private_key, transaction.transaction.signature_hash()).unwrap();
    transaction.signature = signature;

    // Prepare the starknet transaction
    let mut calldata: Vec<u8> = vec![];
    transaction
        .transaction
        .encode_without_signature(&mut calldata);
    let mut calldata: Vec<Felt252> = calldata.into_iter().map(Felt252::from).collect::<Vec<_>>();
    let mut execute_calldata = vec![
        Felt252::ONE,      // call array length
        kakarot_address.0, // contract address
        Felt252::from_bytes_be(&calculate_sn_keccak(b"eth_send_transaction")), // selector
        Felt252::from(calldata.len()), // calldata length
    ];
    execute_calldata.append(&mut calldata);

    let [r_low, r_high] = split_u256(signature.r);
    let [s_low, s_high] = split_u256(signature.s);
    let signature = [r_low, r_high, s_low, s_high, signature.odd_y_parity as u128]
        .into_iter()
        .map(Felt252::from)
        .collect::<Vec<_>>();

    let tx = Transaction::InvokeFunction(
        InvokeFunction::new(
            eoa_address,
            *EXECUTE_ENTRY_POINT_SELECTOR,
            VersionSpecificAccountTxFields::Deprecated(0),
            Felt252::ONE,
            execute_calldata,
            signature,
            chain_id,
            Some(Felt252::ZERO),
        )
        .unwrap(),
    );

    let tx = tx.create_for_simulation(false, false, true, true, true);

    dbg!("executing tx");
    let execution_result = state.execute_transaction(tx).unwrap();
    dbg!("after executing tx");
    assert!(execution_result.revert_error.is_none());

    // Check that the storage var was updated (contract bytecode should update storage var at key 0u256 to 1)
    let low_key = compute_poseidon_storage_base_address(
        "contract_account_storage_keys",
        &[Felt252::ZERO, Felt252::ZERO],
    );
    let (_, value) = state.storage_writes_at((contract_account_address, low_key.to_bytes_be()));
    assert!(value.is_some());
    assert_eq!(value.unwrap(), Felt252::ONE);
}

fn compute_storage_key(key: &str, args: &[Felt252]) -> Felt252 {
    let args = args
        .iter()
        .map(|arg| felt_to_field_element(arg).unwrap())
        .collect::<Vec<_>>();
    field_element_to_felt(&get_storage_var_address(key, &args).unwrap())
}

fn compute_starknet_address(
    evm_address: &Felt252,
    kakarot_address: &Felt252,
    account_class_hash: &Felt252,
) -> Felt252 {
    let salt = felt_to_field_element(evm_address).unwrap();
    let class_hash = felt_to_field_element(account_class_hash).unwrap();
    let deployer_address = felt_to_field_element(kakarot_address).unwrap();

    let constructor_args = vec![deployer_address, salt];

    let starknet_address =
        get_contract_address(salt, class_hash, &constructor_args, deployer_address);

    field_element_to_felt(&starknet_address)
}

fn split_u256(value: U256) -> [u128; 2] {
    [
        (value & U256::from(u128::MAX)).try_into().unwrap(), // safe unwrap <= U128::MAX.
        (value >> U256::from(128)).try_into().unwrap(),      // safe unwrap <= U128::MAX.
    ]
}

fn compute_poseidon_storage_base_address(storage_var_name: &str, keys: &[Felt252]) -> Felt252 {
    let selector = starknet_keccak(storage_var_name.as_bytes());

    let data: Vec<_> = keys
        .iter()
        .filter_map(|d| felt_to_field_element(d).ok())
        .collect();
    let data = [vec![selector], data].concat();

    let key = poseidon_hash_many(&data);
    let key = field_element_to_felt(&key);

    key.mod_floor(
        &Felt252::from_bytes_le_slice(&L2_ADDRESS_UPPER_BOUND.to_bytes_be())
            .try_into()
            .unwrap(),
    )
}
