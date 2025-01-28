// This module tests our code against the blockifier to ensure they work in the same way.
use assert_matches::assert_matches;
use cairo_lang_starknet::contract_class::ContractClass as SierraContractClass;
use cairo_vm::{
    vm::runners::builtin_runner::{HASH_BUILTIN_NAME, RANGE_CHECK_BUILTIN_NAME},
    vm::{
        errors::{
            cairo_run_errors::CairoRunError, vm_errors::VirtualMachineError,
            vm_exception::VmException,
        },
        runners::cairo_runner::ExecutionResources,
    },
    Felt252,
};
use lazy_static::lazy_static;
use num_bigint::BigUint;
use num_traits::Zero;
use pretty_assertions_sorted::{assert_eq, assert_eq_sorted};
use starknet_in_rust::execution::execution_entry_point::ExecutionEntryPoint;
use starknet_in_rust::execution::TransactionExecutionContext;
use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
use starknet_in_rust::services::api::contract_classes::deprecated_contract_class::ContractClass;
use starknet_in_rust::state::ExecutionResourcesManager;
use starknet_in_rust::transaction::fee::calculate_tx_fee;
use starknet_in_rust::transaction::CompiledClassHash;
use starknet_in_rust::transaction::{Declare, Deploy};
use starknet_in_rust::CasmContractClass;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    core::contract_address::{compute_casm_class_hash, compute_sierra_class_hash},
    definitions::constants::{
        CONSTRUCTOR_ENTRY_POINT_SELECTOR, EXECUTE_ENTRY_POINT_SELECTOR, TRANSACTION_VERSION,
        TRANSFER_ENTRY_POINT_SELECTOR, TRANSFER_EVENT_SELECTOR,
        VALIDATE_DECLARE_ENTRY_POINT_SELECTOR, VALIDATE_DEPLOY_ENTRY_POINT_SELECTOR,
    },
};
use starknet_in_rust::{
    core::errors::state_errors::StateError,
    definitions::block_context::{FeeTokenAddresses, FeeType, GasPrices},
};
use starknet_in_rust::{
    definitions::constants::{DEFAULT_CAIRO_RESOURCE_FEE_WEIGHTS, VALIDATE_ENTRY_POINT_SELECTOR},
    transaction::VersionSpecificAccountTxFields,
};
use starknet_in_rust::{
    definitions::{
        block_context::{BlockContext, StarknetChainId, StarknetOsConfig},
        transaction_type::TransactionType,
    },
    execution::{CallInfo, CallType, OrderedEvent, TransactionExecutionInfo},
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        state_api::{State, StateReader},
        state_cache::{StateCache, StorageEntry},
        BlockInfo,
    },
    transaction::{
        error::TransactionError, invoke_function::InvokeFunction, Address, ClassHash,
        DeclareDeprecated, DeployAccount,
    },
    utils::{calculate_sn_keccak, felt_to_hash},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

const ACCOUNT_CONTRACT_PATH: &str = "starknet_programs/account_without_validation.json";
const ERC20_CONTRACT_PATH: &str = "starknet_programs/ERC20.json";
const TEST_CONTRACT_PATH: &str = "starknet_programs/test_contract.json";
const TEST_EMPTY_CONTRACT_PATH: &str = "starknet_programs/empty_contract.json";

lazy_static! {
    // Addresses.
    static ref TEST_ACCOUNT_CONTRACT_ADDRESS: Address = Address(Felt252::from_dec_str("257").unwrap());
    static ref TEST_CONTRACT_ADDRESS: Address = Address(Felt252::from_dec_str("256").unwrap());
    static ref TEST_FIB_CONTRACT_ADDRESS: Address = Address(Felt252::from_dec_str("27728").unwrap());
    pub static ref TEST_SEQUENCER_ADDRESS: Address =
    Address(Felt252::from_dec_str("4096").unwrap());
    pub static ref TEST_ERC20_CONTRACT_ADDRESS: Address =
    Address(Felt252::from_dec_str("4097").unwrap());
    pub(crate) static ref TEST_STRK_CONTRACT_ADDRESS: Address =
    Address(Felt252::from_dec_str("4097").unwrap());
    pub(crate) static ref TEST_FEE_TOKEN_ADDRESSES : FeeTokenAddresses = FeeTokenAddresses::new(TEST_ERC20_CONTRACT_ADDRESS.clone(), TEST_STRK_CONTRACT_ADDRESS.clone());



    // Class hashes.
    static ref TEST_ACCOUNT_CONTRACT_CLASS_HASH: ClassHash = ClassHash::from(Felt252::from_dec_str("273").unwrap());
    static ref TEST_CLASS_HASH: ClassHash = ClassHash::from(Felt252::from_dec_str("272").unwrap());
    static ref TEST_EMPTY_CONTRACT_CLASS_HASH: ClassHash = ClassHash::from(Felt252::from_dec_str("274").unwrap());
    static ref TEST_ERC20_CONTRACT_CLASS_HASH: ClassHash = ClassHash::from(Felt252::from_dec_str("4112").unwrap());
    static ref TEST_FIB_COMPILED_CONTRACT_CLASS_HASH: ClassHash = ClassHash::from(Felt252::from_dec_str("2889767417435368609058888822622483550637539736178264636938129582300971548553").unwrap());

    // Storage keys.
    // NOTE: this key corresponds to the lower 128 bits of an U256
    static ref TEST_ERC20_ACCOUNT_BALANCE_KEY: Felt252 =
        Felt252::from_dec_str("1192211877881866289306604115402199097887041303917861778777990838480655617515").unwrap();
    static ref TEST_ERC20_SEQUENCER_BALANCE_KEY: Felt252 =
        Felt252::from_dec_str("3229073099929281304021185011369329892856197542079132996799046100564060768274").unwrap();
    static ref TEST_ERC20_BALANCE_KEY_1: Felt252 =
        Felt252::from_dec_str("1192211877881866289306604115402199097887041303917861778777990838480655617516").unwrap();
    static ref TEST_ERC20_BALANCE_KEY_2: Felt252 =
        Felt252::from_dec_str("3229073099929281304021185011369329892856197542079132996799046100564060768275").unwrap();

    static ref TEST_ERC20_DEPLOYED_ACCOUNT_BALANCE_KEY: Felt252 =
        Felt252::from_dec_str("2542253978940891427830343982984992363331567580652119103860970381451088310289").unwrap();

    // Others.
    static ref INITIAL_BALANCE: Felt252 = Felt252::from(u128::MAX);
    static ref GAS_PRICES: GasPrices = GasPrices::new(1, 1);
}

pub fn new_starknet_block_context_for_testing() -> BlockContext {
    BlockContext::new(
        StarknetOsConfig::new(
            StarknetChainId::TestNet.to_felt(),
            TEST_FEE_TOKEN_ADDRESSES.clone(),
        ),
        0,
        0,
        DEFAULT_CAIRO_RESOURCE_FEE_WEIGHTS.clone(),
        1_000_000,
        0,
        BlockInfo::empty(TEST_SEQUENCER_ADDRESS.clone()),
        HashMap::default(),
        true,
    )
}

fn create_account_tx_test_state() -> Result<
    (
        BlockContext,
        CachedState<InMemoryStateReader, PermanentContractClassCache>,
    ),
    Box<dyn std::error::Error>,
> {
    let block_context = new_starknet_block_context_for_testing();

    let test_contract_class_hash = *TEST_CLASS_HASH;
    let test_account_contract_class_hash = *TEST_ACCOUNT_CONTRACT_CLASS_HASH;
    let test_erc20_class_hash = *TEST_ERC20_CONTRACT_CLASS_HASH;
    let class_hash_to_class = HashMap::from([
        (
            test_account_contract_class_hash,
            ContractClass::from_path(ACCOUNT_CONTRACT_PATH)?,
        ),
        (
            test_contract_class_hash,
            ContractClass::from_path(TEST_CONTRACT_PATH)?,
        ),
        (
            test_erc20_class_hash,
            ContractClass::from_path(ERC20_CONTRACT_PATH)?,
        ),
    ]);

    let test_contract_address = TEST_CONTRACT_ADDRESS.clone();
    let test_account_contract_address = TEST_ACCOUNT_CONTRACT_ADDRESS.clone();
    let test_erc20_address = block_context
        .starknet_os_config()
        .fee_token_address()
        .eth_fee_token_address
        .clone();
    let address_to_class_hash = HashMap::from([
        (test_contract_address, test_contract_class_hash),
        (
            test_account_contract_address,
            test_account_contract_class_hash,
        ),
        (test_erc20_address.clone(), test_erc20_class_hash),
    ]);

    let test_erc20_account_balance_key = *TEST_ERC20_ACCOUNT_BALANCE_KEY;

    let storage_view = HashMap::from([(
        (test_erc20_address, test_erc20_account_balance_key),
        *INITIAL_BALANCE,
    )]);

    let cached_state = CachedState::new(
        {
            let mut state_reader = InMemoryStateReader::default();
            for (contract_address, class_hash) in address_to_class_hash {
                let storage_keys: HashMap<StorageEntry, Felt252> = storage_view
                    .iter()
                    .filter_map(|((address, storage_key), storage_value)| {
                        (address == &contract_address).then_some((
                            (address.clone(), storage_key.to_bytes_be()),
                            *storage_value,
                        ))
                    })
                    .collect();

                state_reader
                    .address_to_class_hash_mut()
                    .insert(contract_address.clone(), class_hash);

                state_reader
                    .address_to_nonce_mut()
                    .insert(contract_address.clone(), Felt252::ZERO);
                state_reader.address_to_storage_mut().extend(storage_keys);
            }
            for (class_hash, contract_class) in class_hash_to_class {
                state_reader.class_hash_to_compiled_class_mut().insert(
                    class_hash,
                    CompiledClass::Deprecated(Arc::new(contract_class)),
                );
            }
            Arc::new(state_reader)
        },
        Arc::new(PermanentContractClassCache::default()),
    );

    Ok((block_context, cached_state))
}

fn expected_state_before_tx() -> CachedState<InMemoryStateReader, PermanentContractClassCache> {
    let in_memory_state_reader = initial_in_memory_state_reader();

    CachedState::new(
        Arc::new(in_memory_state_reader),
        Arc::new(PermanentContractClassCache::default()),
    )
}

fn expected_state_after_tx(
    fee: u128,
) -> CachedState<InMemoryStateReader, PermanentContractClassCache> {
    let in_memory_state_reader = initial_in_memory_state_reader();

    let contract_classes_cache = PermanentContractClassCache::default();
    contract_classes_cache.set_contract_class(
        *TEST_CLASS_HASH,
        CompiledClass::Deprecated(Arc::new(
            ContractClass::from_path(TEST_CONTRACT_PATH).unwrap(),
        )),
    );
    contract_classes_cache.set_contract_class(
        *TEST_ACCOUNT_CONTRACT_CLASS_HASH,
        CompiledClass::Deprecated(Arc::new(
            ContractClass::from_path(ACCOUNT_CONTRACT_PATH).unwrap(),
        )),
    );
    contract_classes_cache.set_contract_class(
        *TEST_ERC20_CONTRACT_CLASS_HASH,
        CompiledClass::Deprecated(Arc::new(
            ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap(),
        )),
    );

    CachedState::new_for_testing(
        Arc::new(in_memory_state_reader),
        state_cache_after_invoke_tx(fee),
        Arc::new(contract_classes_cache),
    )
}

fn state_cache_after_invoke_tx(fee: u128) -> StateCache {
    let class_hash_initial_values = HashMap::from([(
        TEST_ERC20_CONTRACT_ADDRESS.clone(),
        *TEST_ERC20_CONTRACT_CLASS_HASH,
    )]);

    let nonce_initial_values =
        HashMap::from([(TEST_ACCOUNT_CONTRACT_ADDRESS.clone(), Felt252::ZERO)]);

    let storage_initial_values = HashMap::from([
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_SEQUENCER_BALANCE_KEY.clone().to_bytes_be(),
            ),
            Felt252::ZERO,
        ),
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be(),
            ),
            *INITIAL_BALANCE,
        ),
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_BALANCE_KEY_1.clone().to_bytes_be(),
            ),
            Felt252::ZERO,
        ),
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_BALANCE_KEY_2.clone().to_bytes_be(),
            ),
            Felt252::ZERO,
        ),
    ]);

    let class_hash_writes = HashMap::new();

    let nonce_writes = HashMap::from([(TEST_ACCOUNT_CONTRACT_ADDRESS.clone(), Felt252::from(1))]);

    let storage_writes = HashMap::from([
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_SEQUENCER_BALANCE_KEY.clone().to_bytes_be(),
            ),
            Felt252::from(fee),
        ),
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be(),
            ),
            *INITIAL_BALANCE - Felt252::from(fee),
        ),
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_BALANCE_KEY_1.clone().to_bytes_be(),
            ),
            Felt252::from(0),
        ),
        (
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_BALANCE_KEY_2.clone().to_bytes_be(),
            ),
            Felt252::from(0),
        ),
    ]);

    let compiled_class_hash_initial_values = HashMap::new();
    let compiled_class_hash_writes: HashMap<ClassHash, CompiledClassHash> = HashMap::new();
    let compiled_class_hash: HashMap<ClassHash, CompiledClassHash> = HashMap::new();

    StateCache::new_for_testing(
        class_hash_initial_values,
        compiled_class_hash_initial_values,
        nonce_initial_values,
        storage_initial_values,
        class_hash_writes,
        compiled_class_hash_writes,
        nonce_writes,
        storage_writes,
        compiled_class_hash,
    )
}

fn initial_in_memory_state_reader() -> InMemoryStateReader {
    InMemoryStateReader::new(
        HashMap::from([
            (TEST_CONTRACT_ADDRESS.clone(), *TEST_CLASS_HASH),
            (
                TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
                *TEST_ACCOUNT_CONTRACT_CLASS_HASH,
            ),
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                *TEST_ERC20_CONTRACT_CLASS_HASH,
            ),
        ]),
        HashMap::from([
            (TEST_CONTRACT_ADDRESS.clone(), Felt252::ZERO),
            (TEST_ACCOUNT_CONTRACT_ADDRESS.clone(), Felt252::ZERO),
            (TEST_ERC20_CONTRACT_ADDRESS.clone(), Felt252::ZERO),
        ]),
        HashMap::from([(
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be(),
            ),
            *INITIAL_BALANCE,
        )]),
        HashMap::from([
            (
                *TEST_ERC20_CONTRACT_CLASS_HASH,
                CompiledClass::Deprecated(Arc::new(
                    ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap(),
                )),
            ),
            (
                *TEST_ACCOUNT_CONTRACT_CLASS_HASH,
                CompiledClass::Deprecated(Arc::new(
                    ContractClass::from_path(ACCOUNT_CONTRACT_PATH).unwrap(),
                )),
            ),
            (
                *TEST_CLASS_HASH,
                CompiledClass::Deprecated(Arc::new(
                    ContractClass::from_path(TEST_CONTRACT_PATH).unwrap(),
                )),
            ),
        ]),
        HashMap::new(),
    )
}

fn expected_validate_call_info(
    entry_point_selector: Felt252,
    calldata: Vec<Felt252>,
    storage_address: Address,
) -> CallInfo {
    CallInfo {
        entry_point_type: EntryPointType::External.into(),
        entry_point_selector: entry_point_selector.into(),
        calldata,
        contract_address: storage_address,

        // Entries **not** in blockifier.
        class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
        call_type: Some(CallType::Call),
        execution_resources: Some(ExecutionResources {
            n_steps: 13,
            ..Default::default()
        }),

        ..Default::default()
    }
}

fn expected_fee_transfer_call_info(
    block_context: &BlockContext,
    account_address: &Address,
    actual_fee: u128,
) -> CallInfo {
    CallInfo {
        entry_point_type: EntryPointType::External.into(),
        entry_point_selector: (*TRANSFER_ENTRY_POINT_SELECTOR).into(),
        calldata: vec![
            block_context.block_info().sequencer_address.0,
            actual_fee.into(),
            Felt252::ZERO,
        ],
        contract_address: block_context
            .starknet_os_config()
            .fee_token_address()
            .eth_fee_token_address
            .clone(),
        caller_address: account_address.clone(),
        retdata: vec![Felt252::ONE],
        events: vec![OrderedEvent {
            order: 0,
            keys: vec![*TRANSFER_EVENT_SELECTOR],
            data: vec![
                account_address.0,
                block_context.block_info().sequencer_address.0,
                actual_fee.into(),
                Felt252::ZERO,
            ],
        }],

        // Entries **not** in blockifier.
        class_hash: Some(*TEST_ERC20_CONTRACT_CLASS_HASH),
        call_type: Some(CallType::Call),
        accessed_storage_keys: HashSet::from([
            ClassHash(
                Felt252::from_hex(
                    "0x59edd60f3f5ec74e9044489e795cf85179665185dd4317e31668390760f3012",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658813",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658812",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x59edd60f3f5ec74e9044489e795cf85179665185dd4317e31668390760f3011",
                )
                .unwrap()
                .to_bytes_be(),
            ),
        ]),
        storage_read_values: vec![
            *INITIAL_BALANCE,
            Felt252::ZERO,
            *INITIAL_BALANCE,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
        ],
        execution_resources: Some(ExecutionResources {
            n_steps: 529,
            n_memory_holes: 57,
            builtin_instance_counter: HashMap::from([
                (RANGE_CHECK_BUILTIN_NAME.to_string(), 21),
                (HASH_BUILTIN_NAME.to_string(), 4),
            ]),
        }),
        ..Default::default()
    }
}

fn validate_final_balances<S>(
    state: &mut S,
    block_context: &BlockContext,
    erc20_account_balance_storage_key: &ClassHash,
    fee: u128,
) where
    S: State + StateReader,
{
    let account_balance = state
        .get_storage_at(&(
            block_context
                .starknet_os_config()
                .fee_token_address()
                .eth_fee_token_address
                .clone(),
            erc20_account_balance_storage_key.0,
        ))
        .unwrap();
    assert_eq!(account_balance, *INITIAL_BALANCE - Felt252::from(fee));

    let sequencer_balance = state
        .get_storage_at(&(
            block_context
                .starknet_os_config()
                .fee_token_address()
                .clone()
                .eth_fee_token_address,
            TEST_ERC20_SEQUENCER_BALANCE_KEY.clone().to_bytes_be(),
        ))
        .unwrap();
    assert_eq!(sequencer_balance, fee.into());
}

#[test]
fn test_create_account_tx_test_state() {
    let (block_context, state) = create_account_tx_test_state().unwrap();

    let expected_initial_state = expected_state_before_tx();
    assert_eq!(&state.cache(), &expected_initial_state.cache());
    assert_eq!(
        (&*state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>(),
        (&*expected_initial_state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        &state.state_reader.address_to_class_hash,
        &expected_initial_state.state_reader.address_to_class_hash
    );
    assert_eq!(
        &state.state_reader.address_to_nonce,
        &expected_initial_state.state_reader.address_to_nonce
    );
    assert_eq!(
        &state.state_reader.address_to_storage,
        &expected_initial_state.state_reader.address_to_storage
    );
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 16, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 17
        ])));

    let value = state
        .get_storage_at(&(
            block_context
                .starknet_os_config()
                .fee_token_address()
                .eth_fee_token_address
                .clone(),
            TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be(),
        ))
        .unwrap();
    assert_eq!(value, *INITIAL_BALANCE);

    let class_hash = state.get_class_hash_at(&TEST_CONTRACT_ADDRESS).unwrap();
    assert_eq!(class_hash, TEST_CLASS_HASH.clone());

    let _contract_class: ContractClass = state
        .get_contract_class(&TEST_ERC20_CONTRACT_CLASS_HASH.clone())
        .unwrap()
        .try_into()
        .unwrap();
    // We cant compare this until a new implementation of Eq for programs, due to a change in the hints_ranges.
    // assert_eq!(
    //     _contract_class,
    //     ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap()
    // );
}

fn invoke_tx(calldata: Vec<Felt252>, max_fee: u128) -> InvokeFunction {
    InvokeFunction::new(
        TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        *EXECUTE_ENTRY_POINT_SELECTOR,
        VersionSpecificAccountTxFields::new_deprecated(max_fee),
        *TRANSACTION_VERSION,
        calldata,
        vec![],
        StarknetChainId::TestNet.to_felt(),
        Some(Felt252::ZERO),
    )
    .unwrap()
}

fn invoke_tx_with_nonce(calldata: Vec<Felt252>, max_fee: u128, nonce: Felt252) -> InvokeFunction {
    InvokeFunction::new(
        TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        *EXECUTE_ENTRY_POINT_SELECTOR,
        VersionSpecificAccountTxFields::new_deprecated(max_fee),
        *TRANSACTION_VERSION,
        calldata,
        vec![],
        StarknetChainId::TestNet.to_felt(),
        Some(nonce),
    )
    .unwrap()
}

fn expected_fee_transfer_info(fee: u128) -> CallInfo {
    CallInfo {
        failure_flag: false,
        gas_consumed: 0,
        caller_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        call_type: Some(CallType::Call),
        contract_address: Address(Felt252::from(4097)),
        code_address: None,
        class_hash: Some(*TEST_ERC20_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*TRANSFER_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![Felt252::from(4096), Felt252::from(fee), Felt252::ZERO],
        retdata: vec![Felt252::from(1)],
        execution_resources: Some(ExecutionResources {
            n_steps: 525,
            n_memory_holes: 59,
            builtin_instance_counter: HashMap::from([
                (RANGE_CHECK_BUILTIN_NAME.to_string(), 21),
                (HASH_BUILTIN_NAME.to_string(), 4),
            ]),
        }),
        l2_to_l1_messages: vec![],
        internal_calls: vec![],
        events: vec![OrderedEvent {
            order: 0,
            keys: vec![*TRANSFER_EVENT_SELECTOR],
            data: vec![
                Felt252::from(257),
                Felt252::from(4096),
                Felt252::from(fee),
                Felt252::ZERO,
            ],
        }],
        storage_read_values: vec![
            *INITIAL_BALANCE,
            Felt252::ZERO,
            *INITIAL_BALANCE,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
        ],
        accessed_storage_keys: HashSet::from([
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658813",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x2a2c49c4dba0d91b34f2ade85d41d09561f9a77884c15ba2ab0f2241b080dec",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658812",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x2a2c49c4dba0d91b34f2ade85d41d09561f9a77884c15ba2ab0f2241b080deb",
                )
                .unwrap()
                .to_bytes_be(),
            ),
        ]),
    }
}

fn expected_fib_fee_transfer_info(fee: u128) -> CallInfo {
    CallInfo {
        failure_flag: false,
        gas_consumed: 0,
        caller_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        call_type: Some(CallType::Call),
        contract_address: Address(Felt252::from(4097)),
        code_address: None,
        class_hash: Some(*TEST_ERC20_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*TRANSFER_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![Felt252::from(4096), Felt252::from(fee), Felt252::ZERO],
        retdata: vec![Felt252::from(1)],
        execution_resources: Some(ExecutionResources {
            n_steps: 525,
            n_memory_holes: 59,
            builtin_instance_counter: HashMap::from([
                ("range_check_builtin".to_string(), 21),
                ("pedersen_builtin".to_string(), 4),
            ]),
        }),
        l2_to_l1_messages: vec![],
        internal_calls: vec![],
        events: vec![OrderedEvent {
            order: 0,
            keys: vec![*TRANSFER_EVENT_SELECTOR],
            data: vec![
                Felt252::from(257),
                Felt252::from(4096),
                Felt252::from(fee),
                Felt252::ZERO,
            ],
        }],
        storage_read_values: vec![
            *INITIAL_BALANCE - Felt252::from(2784),
            Felt252::ZERO,
            *INITIAL_BALANCE - Felt252::from(2784),
            Felt252::ZERO,
            Felt252::from(2784),
            Felt252::ZERO,
            Felt252::from(2784),
            Felt252::ZERO,
        ],
        accessed_storage_keys: HashSet::from([
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658813",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x2a2c49c4dba0d91b34f2ade85d41d09561f9a77884c15ba2ab0f2241b080dec",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658812",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x2a2c49c4dba0d91b34f2ade85d41d09561f9a77884c15ba2ab0f2241b080deb",
                )
                .unwrap()
                .to_bytes_be(),
            ),
        ]),
    }
}

fn declare_tx() -> DeclareDeprecated {
    DeclareDeprecated {
        contract_class: ContractClass::from_path(TEST_EMPTY_CONTRACT_PATH).unwrap(),
        class_hash: *TEST_EMPTY_CONTRACT_CLASS_HASH,
        sender_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        validate_entry_point_selector: *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR,
        version: 1.into(),
        max_fee: 100000,
        signature: vec![],
        nonce: 0.into(),
        hash_value: 0.into(),
        skip_execute: false,
        skip_fee_transfer: false,
        skip_validate: false,
        skip_nonce_check: false,
    }
}

fn declarev2_tx() -> Declare {
    let program_data =
        include_bytes!("../../starknet_programs/raw_contract_classes/fibonacci.sierra");
    let sierra_contract_class: SierraContractClass = serde_json::from_slice(program_data).unwrap();
    let sierra_class_hash = compute_sierra_class_hash(&sierra_contract_class).unwrap();
    let casm_class =
        CasmContractClass::from_contract_class(sierra_contract_class.clone(), true).unwrap();
    let casm_class_hash = compute_casm_class_hash(&casm_class).unwrap();

    Declare {
        sender_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        validate_entry_point_selector: *VALIDATE_DECLARE_ENTRY_POINT_SELECTOR,
        version: 2.into(),
        account_tx_fields: VersionSpecificAccountTxFields::new_deprecated(50000000),
        signature: vec![],
        nonce: 0.into(),
        hash_value: 0.into(),
        compiled_class_hash: casm_class_hash,
        sierra_contract_class: Some(sierra_contract_class),
        sierra_class_hash,
        casm_class: casm_class.into(),
        skip_execute: false,
        skip_fee_transfer: false,
        skip_validate: false,
        skip_nonce_check: false,
    }
}

fn deploy_fib_syscall() -> Deploy {
    let program_data = include_bytes!("../../starknet_programs/cairo2/fibonacci.sierra");
    let sierra_contract_class: SierraContractClass = serde_json::from_slice(program_data).unwrap();
    let casm_class = CasmContractClass::from_contract_class(sierra_contract_class, true).unwrap();
    let contract_class = CompiledClass::Casm {
        casm: Arc::new(casm_class),
        sierra: None,
    };
    Deploy {
        hash_value: 0.into(),
        version: 1.into(),
        contract_address: TEST_FIB_CONTRACT_ADDRESS.clone(),
        contract_address_salt: 0.into(),
        contract_hash: *TEST_FIB_COMPILED_CONTRACT_CLASS_HASH,
        contract_class,
        constructor_calldata: Vec::new(),
        skip_execute: false,
        skip_fee_transfer: false,
        skip_validate: false,
    }
}

fn expected_declare_fee_transfer_info(fee: u128) -> CallInfo {
    CallInfo {
        caller_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        call_type: Some(CallType::Call),
        contract_address: TEST_ERC20_CONTRACT_ADDRESS.clone(),
        class_hash: Some(*TEST_ERC20_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*TRANSFER_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![TEST_SEQUENCER_ADDRESS.0, Felt252::from(fee), Felt252::ZERO],
        retdata: vec![1.into()],
        events: vec![OrderedEvent::new(
            0,
            vec![Felt252::from_dec_str(
                "271746229759260285552388728919865295615886751538523744128730118297934206697",
            )
            .unwrap()],
            vec![
                TEST_ACCOUNT_CONTRACT_ADDRESS.clone().0,
                TEST_SEQUENCER_ADDRESS.clone().0,
                Felt252::from(fee),
                0.into(),
            ],
        )],
        storage_read_values: vec![
            *INITIAL_BALANCE,
            Felt252::ZERO,
            *INITIAL_BALANCE,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
            Felt252::ZERO,
        ],
        accessed_storage_keys: HashSet::from([
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658813",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x2a2c49c4dba0d91b34f2ade85d41d09561f9a77884c15ba2ab0f2241b080dec",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x723973208639b7839ce298f7ffea61e3f9533872defd7abdb91023db4658812",
                )
                .unwrap()
                .to_bytes_be(),
            ),
            ClassHash(
                Felt252::from_hex(
                    "0x2a2c49c4dba0d91b34f2ade85d41d09561f9a77884c15ba2ab0f2241b080deb",
                )
                .unwrap()
                .to_bytes_be(),
            ),
        ]),

        execution_resources: Some(ExecutionResources {
            n_steps: 525,
            n_memory_holes: 59,
            builtin_instance_counter: HashMap::from([
                (RANGE_CHECK_BUILTIN_NAME.to_string(), 21),
                (HASH_BUILTIN_NAME.to_string(), 4),
            ]),
        }),
        ..Default::default()
    }
}

#[test]
fn test_declare_tx() {
    let (block_context, mut state) = create_account_tx_test_state().unwrap();
    let expected_initial_state = expected_state_before_tx();
    assert_eq!(&state.cache(), &expected_initial_state.cache());
    assert_eq!(
        (&*state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>(),
        (&*expected_initial_state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        &state.state_reader.address_to_class_hash,
        &expected_initial_state.state_reader.address_to_class_hash
    );
    assert_eq!(
        &state.state_reader.address_to_nonce,
        &expected_initial_state.state_reader.address_to_nonce
    );
    assert_eq!(
        &state.state_reader.address_to_storage,
        &expected_initial_state.state_reader.address_to_storage
    );
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 16, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 17
        ])));

    let declare_tx = declare_tx();
    // Check ContractClass is not set before the declare_tx
    assert!(state.get_contract_class(&declare_tx.class_hash).is_err());
    // Execute declare_tx
    let result = declare_tx
        .execute(
            &mut state,
            &block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();
    // Check ContractClass is set after the declare_tx
    assert!(state.get_contract_class(&declare_tx.class_hash).is_ok());

    let resources = HashMap::from([
        ("n_steps".to_string(), 2921),
        ("range_check_builtin".to_string(), 63),
        ("pedersen_builtin".to_string(), 15),
        ("l1_gas_usage".to_string(), 1652),
    ]);
    let fee = calculate_tx_fee(&resources, &block_context, &FeeType::Eth).unwrap();

    let expected_execution_info = TransactionExecutionInfo::new(
        Some(CallInfo {
            call_type: Some(CallType::Call),
            contract_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
            class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
            entry_point_selector: Some(*VALIDATE_DECLARE_ENTRY_POINT_SELECTOR),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![Felt252::from_bytes_be(&TEST_EMPTY_CONTRACT_CLASS_HASH.0)],
            execution_resources: Some(ExecutionResources {
                n_steps: 12,
                ..Default::default()
            }),
            ..Default::default()
        }),
        None,
        None,
        Some(expected_declare_fee_transfer_info(fee)),
        fee,
        resources,
        Some(TransactionType::Declare),
    );

    assert_eq!(result, expected_execution_info);
}

#[test]
fn test_declarev2_tx() {
    let (block_context, mut state) = create_account_tx_test_state().unwrap();
    let expected_initial_state = expected_state_before_tx();
    assert_eq!(&state.cache(), &expected_initial_state.cache());
    assert_eq!(
        (&*state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>(),
        (&*expected_initial_state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        &state.state_reader.address_to_class_hash,
        &expected_initial_state.state_reader.address_to_class_hash
    );
    assert_eq!(
        &state.state_reader.address_to_nonce,
        &expected_initial_state.state_reader.address_to_nonce
    );
    assert_eq!(
        &state.state_reader.address_to_storage,
        &expected_initial_state.state_reader.address_to_storage
    );
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 16, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 17
        ])));

    let declare_tx = declarev2_tx();
    // Check ContractClass is not set before the declare_tx
    assert!(state
        .get_contract_class(&felt_to_hash(&declare_tx.compiled_class_hash))
        .is_err());
    // Execute declare_tx
    let result = declare_tx
        .execute(
            &mut state,
            &block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();
    // Check ContractClass is set after the declare_tx
    assert!(state
        .get_contract_class(&ClassHash::from(declare_tx.compiled_class_hash))
        .is_ok());

    let resources = HashMap::from([
        ("n_steps".to_string(), 2921),
        ("range_check_builtin".to_string(), 63),
        ("pedersen_builtin".to_string(), 15),
        ("l1_gas_usage".to_string(), 2754),
    ]);
    let fee = calculate_tx_fee(&resources, &block_context, &FeeType::Eth).unwrap();
    let expected_execution_info = TransactionExecutionInfo::new(
        Some(CallInfo {
            call_type: Some(CallType::Call),
            contract_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
            class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
            entry_point_selector: Some(*VALIDATE_DECLARE_ENTRY_POINT_SELECTOR),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![Felt252::from_bytes_be(
                &TEST_FIB_COMPILED_CONTRACT_CLASS_HASH.0,
            )],
            execution_resources: Some(ExecutionResources {
                n_steps: 12,
                ..Default::default()
            }),
            ..Default::default()
        }),
        None,
        None,
        Some(expected_declare_fee_transfer_info(fee)),
        fee,
        resources,
        Some(TransactionType::Declare),
    );

    assert_eq!(result, expected_execution_info);
}

fn expected_execute_call_info() -> CallInfo {
    CallInfo {
        caller_address: Address(Felt252::ZERO),
        call_type: Some(CallType::Call),
        contract_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        code_address: None,
        class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*EXECUTE_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![
            Felt252::from(256),
            Felt252::from_hex("0x039a1491f76903a16feed0a6433bec78de4c73194944e1118e226820ad479701")
                .unwrap(),
            Felt252::from(1),
            Felt252::from(2),
        ],
        retdata: vec![Felt252::from(2)],
        l2_to_l1_messages: vec![],
        internal_calls: vec![CallInfo {
            caller_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
            call_type: Some(CallType::Call),
            class_hash: Some(*TEST_CLASS_HASH),
            entry_point_selector: Some(
                Felt252::from_hex(
                    "0x039a1491f76903a16feed0a6433bec78de4c73194944e1118e226820ad479701",
                )
                .unwrap(),
            ),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![Felt252::from(2)],
            retdata: vec![Felt252::from(2)],
            events: vec![],
            l2_to_l1_messages: vec![],
            internal_calls: vec![],
            contract_address: TEST_CONTRACT_ADDRESS.clone(),
            code_address: None,
            execution_resources: Some(ExecutionResources {
                n_steps: 22,
                ..Default::default()
            }),
            ..Default::default()
        }],
        events: vec![],
        execution_resources: Some(ExecutionResources {
            n_steps: 61,
            n_memory_holes: 0,
            builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 1)]),
        }),
        ..Default::default()
    }
}

fn expected_fib_execute_call_info() -> CallInfo {
    CallInfo {
        caller_address: Address(Felt252::ZERO),
        call_type: Some(CallType::Call),
        contract_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        code_address: None,
        class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*EXECUTE_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![
            Felt252::from(27728),
            Felt252::from_bytes_be(&calculate_sn_keccak(b"fib")),
            Felt252::from(3),
            Felt252::from(42),
            Felt252::from(0),
            Felt252::from(0),
        ],
        retdata: vec![Felt252::from(42)],
        execution_resources: Some(ExecutionResources {
            n_steps: 148,
            n_memory_holes: 0,
            builtin_instance_counter: HashMap::from([("range_check_builtin".to_string(), 4)]),
        }),
        l2_to_l1_messages: vec![],
        internal_calls: vec![CallInfo {
            caller_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
            call_type: Some(CallType::Call),
            class_hash: Some(*TEST_FIB_COMPILED_CONTRACT_CLASS_HASH),
            entry_point_selector: Some(Felt252::from_bytes_be(&calculate_sn_keccak(b"fib"))),
            entry_point_type: Some(EntryPointType::External),
            calldata: vec![Felt252::from(42), Felt252::from(0), Felt252::from(0)],
            retdata: vec![Felt252::from(42)],
            events: vec![],
            l2_to_l1_messages: vec![],
            internal_calls: vec![],
            contract_address: TEST_FIB_CONTRACT_ADDRESS.clone(),
            code_address: None,
            gas_consumed: 2980,
            execution_resources: Some(ExecutionResources {
                n_steps: 109,
                n_memory_holes: 0,
                builtin_instance_counter: HashMap::from([("range_check_builtin".to_string(), 3)]),
            }),
            ..Default::default()
        }],
        events: vec![],
        ..Default::default()
    }
}

fn expected_validate_call_info_2() -> CallInfo {
    CallInfo {
        caller_address: Address(Felt252::ZERO),
        call_type: Some(CallType::Call),
        contract_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*VALIDATE_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![
            Felt252::from(256),
            Felt252::from_hex("0x039a1491f76903a16feed0a6433bec78de4c73194944e1118e226820ad479701")
                .unwrap(),
            Felt252::from(1),
            Felt252::from(2),
        ],
        execution_resources: Some(ExecutionResources {
            n_steps: 21,
            n_memory_holes: 0,
            builtin_instance_counter: HashMap::from([(RANGE_CHECK_BUILTIN_NAME.to_string(), 1)]),
        }),
        ..Default::default()
    }
}

fn expected_fib_validate_call_info_2() -> CallInfo {
    CallInfo {
        caller_address: Address(Felt252::ZERO),
        call_type: Some(CallType::Call),
        contract_address: TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(*VALIDATE_ENTRY_POINT_SELECTOR),
        entry_point_type: Some(EntryPointType::External),
        calldata: vec![
            Felt252::from(27728),
            Felt252::from_bytes_be(&calculate_sn_keccak(b"fib")),
            Felt252::from(3),
            Felt252::from(42),
            Felt252::from(0),
            Felt252::from(0),
        ],
        execution_resources: Some(ExecutionResources {
            n_steps: 21,
            n_memory_holes: 0,
            builtin_instance_counter: HashMap::from([("range_check_builtin".to_string(), 1)]),
        }),
        ..Default::default()
    }
}

fn expected_transaction_execution_info(block_context: &BlockContext) -> TransactionExecutionInfo {
    let resources = HashMap::from([
        ("n_steps".to_string(), 4463),
        ("pedersen_builtin".to_string(), 16),
        ("l1_gas_usage".to_string(), 1652),
        ("range_check_builtin".to_string(), 102),
    ]);
    let fee = calculate_tx_fee(&resources, block_context, &FeeType::Eth).unwrap();
    TransactionExecutionInfo::new(
        Some(expected_validate_call_info_2()),
        Some(expected_execute_call_info()),
        None,
        Some(expected_fee_transfer_info(fee)),
        fee,
        resources,
        Some(TransactionType::InvokeFunction),
    )
}

fn expected_fib_transaction_execution_info(
    block_context: &BlockContext,
) -> TransactionExecutionInfo {
    let n_steps = 4550;
    let resources = HashMap::from([
        ("n_steps".to_string(), n_steps),
        ("l1_gas_usage".to_string(), 5197),
        ("pedersen_builtin".to_string(), 16),
        ("range_check_builtin".to_string(), 105),
    ]);
    let fee = calculate_tx_fee(&resources, block_context, &FeeType::Eth).unwrap();
    TransactionExecutionInfo::new(
        Some(expected_fib_validate_call_info_2()),
        Some(expected_fib_execute_call_info()),
        None,
        Some(expected_fib_fee_transfer_info(fee)),
        fee,
        resources,
        Some(TransactionType::InvokeFunction),
    )
}

#[test]
fn test_invoke_tx() {
    let (block_context, state) = &mut create_account_tx_test_state().unwrap();
    let Address(test_contract_address) = TEST_CONTRACT_ADDRESS.clone();
    let calldata = vec![
        test_contract_address, // CONTRACT_ADDRESS
        Felt252::from_bytes_be(&calculate_sn_keccak(b"return_result")), // CONTRACT FUNCTION SELECTOR
        Felt252::from(1),                                               // CONTRACT_CALLDATA LEN
        Felt252::from(2),                                               // CONTRACT_CALLDATA
    ];
    let invoke_tx = invoke_tx(calldata, u128::MAX);

    // Extract invoke transaction fields for testing, as it is consumed when creating an account
    // transaction.
    let result = invoke_tx
        .execute(
            state,
            block_context,
            0,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();
    let expected_execution_info = expected_transaction_execution_info(block_context);

    assert_eq_sorted!(result, expected_execution_info);
}

#[test]
fn test_invoke_tx_exceeded_max_fee() {
    let (block_context, state) = &mut create_account_tx_test_state().unwrap();
    let Address(test_contract_address) = TEST_CONTRACT_ADDRESS.clone();
    let calldata = vec![
        test_contract_address, // CONTRACT_ADDRESS
        Felt252::from_bytes_be(&calculate_sn_keccak(b"return_result")), // CONTRACT FUNCTION SELECTOR
        Felt252::from(1),                                               // CONTRACT_CALLDATA LEN
        Felt252::from(2),                                               // CONTRACT_CALLDATA
    ];
    let max_fee = 1000;
    let actual_fee = 1697;
    let invoke_tx = invoke_tx(calldata, max_fee);

    // Extract invoke transaction fields for testing, as it is consumed when creating an account
    // transaction.
    let result = invoke_tx
        .execute(
            state,
            block_context,
            0,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();
    let mut expected_result = expected_transaction_execution_info(block_context).to_revert_error(
        format!(
            "Calculated fee ({}) exceeds max fee ({})",
            actual_fee, max_fee
        )
        .as_str(),
    );
    expected_result.set_fee_info(max_fee, Some(expected_fee_transfer_info(max_fee)));

    assert_eq_sorted!(result, expected_result);

    // Check final balance
    let test_erc20_address = block_context
        .starknet_os_config()
        .fee_token_address()
        .eth_fee_token_address
        .clone();
    let test_erc20_account_balance_key = *TEST_ERC20_ACCOUNT_BALANCE_KEY;

    let balance = state
        .get_storage_at(&(
            test_erc20_address,
            test_erc20_account_balance_key.to_bytes_be(),
        ))
        .unwrap();
    let expected_balance = *INITIAL_BALANCE - Felt252::from(max_fee);

    assert_eq!(balance, expected_balance);
}

#[test]
fn test_invoke_tx_state() {
    let (starknet_general_context, state) = &mut create_account_tx_test_state().unwrap();
    let expected_initial_state = expected_state_before_tx();
    assert_eq!(&state.cache(), &expected_initial_state.cache());
    assert_eq!(
        (&*state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>(),
        (&*expected_initial_state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        &state.state_reader.address_to_class_hash,
        &expected_initial_state.state_reader.address_to_class_hash
    );
    assert_eq!(
        &state.state_reader.address_to_nonce,
        &expected_initial_state.state_reader.address_to_nonce
    );
    assert_eq!(
        &state.state_reader.address_to_storage,
        &expected_initial_state.state_reader.address_to_storage
    );
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 16, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 17
        ])));

    let Address(test_contract_address) = TEST_CONTRACT_ADDRESS.clone();
    let calldata = vec![
        test_contract_address, // CONTRACT_ADDRESS
        Felt252::from_bytes_be(&calculate_sn_keccak(b"return_result")), // CONTRACT FUNCTION SELECTOR
        Felt252::from(1),                                               // CONTRACT_CALLDATA LEN
        Felt252::from(2),                                               // CONTRACT_CALLDATA
    ];
    let invoke_tx = invoke_tx(calldata, u128::MAX);

    let result = invoke_tx
        .execute(
            state,
            starknet_general_context,
            0,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    let expected_final_state = expected_state_after_tx(result.actual_fee);

    assert_eq!(&state.cache(), &expected_final_state.cache());
    assert_eq!(
        &state.state_reader.address_to_class_hash,
        &expected_final_state.state_reader.address_to_class_hash
    );
    assert_eq!(
        &state.state_reader.address_to_nonce,
        &expected_final_state.state_reader.address_to_nonce
    );
    assert_eq!(
        &state.state_reader.address_to_storage,
        &expected_final_state.state_reader.address_to_storage
    );
}

#[test]
fn test_invoke_with_declarev2_tx() {
    let (block_context, state) = &mut create_account_tx_test_state().unwrap();
    let expected_initial_state = expected_state_before_tx();
    assert_eq!(&state.cache(), &expected_initial_state.cache());
    assert_eq!(
        (&*state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>(),
        (&*expected_initial_state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        &state.state_reader.address_to_class_hash,
        &expected_initial_state.state_reader.address_to_class_hash
    );
    assert_eq!(
        &state.state_reader.address_to_nonce,
        &expected_initial_state.state_reader.address_to_nonce
    );
    assert_eq!(
        &state.state_reader.address_to_storage,
        &expected_initial_state.state_reader.address_to_storage
    );
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 16, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 16
        ])));
    assert!(&state
        .state_reader
        .class_hash_to_compiled_class
        .contains_key(&ClassHash([
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 1, 17
        ])));

    // Declare the fibonacci contract
    let declare_tx = declarev2_tx();
    declare_tx
        .execute(
            state,
            block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    // Deploy the fibonacci contract
    let deploy = deploy_fib_syscall();
    deploy
        .execute(
            state,
            block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    let Address(test_contract_address) = TEST_FIB_CONTRACT_ADDRESS.clone();
    let calldata = vec![
        test_contract_address,                                // CONTRACT ADDRESS
        Felt252::from_bytes_be(&calculate_sn_keccak(b"fib")), // CONTRACT FUNCTION SELECTOR
        Felt252::from(3),                                     // CONTRACT CALLDATA LEN
        Felt252::from(42),                                    // a
        Felt252::from(0),                                     // b
        Felt252::from(0),                                     // n
    ];
    let invoke_tx = invoke_tx_with_nonce(calldata, u64::MAX as u128, Felt252::ONE);

    let expected_gas_consumed = 5551;
    let result = invoke_tx
        .execute(
            state,
            block_context,
            expected_gas_consumed,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    let expected_execution_info = expected_fib_transaction_execution_info(block_context);
    assert_eq_sorted!(result, expected_execution_info);
}

#[test]
fn test_deploy_account() {
    let (block_context, mut state) = create_account_tx_test_state().unwrap();

    let expected_fee = 2242;

    let deploy_account_tx = DeployAccount::new(
        *TEST_ACCOUNT_CONTRACT_CLASS_HASH,
        VersionSpecificAccountTxFields::new_deprecated(expected_fee),
        *TRANSACTION_VERSION,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        StarknetChainId::TestNet.to_felt(),
    )
    .unwrap();

    state.cache_mut().storage_initial_values_mut().insert(
        (
            block_context
                .starknet_os_config()
                .fee_token_address()
                .eth_fee_token_address
                .clone(),
            TEST_ERC20_DEPLOYED_ACCOUNT_BALANCE_KEY
                .clone()
                .to_bytes_be(),
        ),
        *INITIAL_BALANCE,
    );

    let (state_before, state_after) = expected_deploy_account_states();

    assert_eq!(&state.cache(), &state_before.cache());
    assert_eq!(
        (&*state.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>(),
        (&*state_before.contract_class_cache().clone())
            .into_iter()
            .collect::<Vec<_>>()
    );

    let tx_info = deploy_account_tx
        .execute(
            &mut state,
            &block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    use pretty_assertions_sorted::assert_eq_sorted;
    assert_eq_sorted!(state.cache(), state_after.cache());

    let expected_validate_call_info = expected_validate_call_info(
        *VALIDATE_DEPLOY_ENTRY_POINT_SELECTOR,
        [
            Felt252::from_bytes_be(&deploy_account_tx.class_hash().0),
            *deploy_account_tx.contract_address_salt(),
        ]
        .into_iter()
        .chain(deploy_account_tx.constructor_calldata().clone())
        .collect(),
        deploy_account_tx.contract_address().clone(),
    );

    let expected_execute_call_info = CallInfo {
        entry_point_type: EntryPointType::Constructor.into(),
        entry_point_selector: (*CONSTRUCTOR_ENTRY_POINT_SELECTOR).into(),
        contract_address: deploy_account_tx.contract_address().clone(),

        // Entries **not** in blockifier.
        class_hash: Some(*TEST_ACCOUNT_CONTRACT_CLASS_HASH),
        call_type: Some(CallType::Call),

        ..Default::default()
    };

    let expected_fee_transfer_call_info = expected_fee_transfer_call_info(
        &block_context,
        deploy_account_tx.contract_address(),
        expected_fee,
    );

    let resources = HashMap::from([
        ("n_steps".to_string(), 3893),
        ("range_check_builtin".to_string(), 83),
        ("pedersen_builtin".to_string(), 23),
        ("l1_gas_usage".to_string(), 2203),
    ]);

    let fee = calculate_tx_fee(&resources, &block_context, &FeeType::Eth).unwrap();

    assert_eq!(fee, expected_fee);

    let expected_execution_info = TransactionExecutionInfo::new(
        expected_validate_call_info.into(),
        expected_execute_call_info.into(),
        None,
        expected_fee_transfer_call_info.into(),
        expected_fee,
        // Entry **not** in blockifier.
        // Default::default(),
        resources,
        TransactionType::DeployAccount.into(),
    );
    assert_eq_sorted!(tx_info, expected_execution_info);

    let nonce_from_state = state
        .get_nonce_at(deploy_account_tx.contract_address())
        .unwrap();
    assert_eq!(nonce_from_state, Felt252::ONE);

    let hash = TEST_ERC20_DEPLOYED_ACCOUNT_BALANCE_KEY.to_bytes_be();
    let class_hash = ClassHash(hash);
    validate_final_balances(&mut state, &block_context, &class_hash, expected_fee);

    let class_hash_from_state = state
        .get_class_hash_at(deploy_account_tx.contract_address())
        .unwrap();
    assert_eq!(class_hash_from_state, *deploy_account_tx.class_hash());
}

fn expected_deploy_account_states() -> (
    CachedState<InMemoryStateReader, PermanentContractClassCache>,
    CachedState<InMemoryStateReader, PermanentContractClassCache>,
) {
    let fee = Felt252::from(2242);
    let mut state_before = CachedState::new(
        Arc::new(InMemoryStateReader::new(
            HashMap::from([
                (Address(0x100.into()), felt_to_hash(&0x110.into())),
                (Address(0x1001.into()), felt_to_hash(&0x1010.into())),
            ]),
            HashMap::from([
                (Address(0x101.into()), Default::default()),
                (Address(0x100.into()), Default::default()),
                (Address(0x1001.into()), Default::default()),
            ]),
            HashMap::from([(
                (
                    Address(0x1001.into()),
                    TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be(),
                ),
                *INITIAL_BALANCE,
            )]),
            HashMap::from([
                (
                    felt_to_hash(&0x110.into()),
                    CompiledClass::Deprecated(Arc::new(
                        ContractClass::from_path(TEST_CONTRACT_PATH).unwrap(),
                    )),
                ),
                (
                    felt_to_hash(&0x111.into()),
                    CompiledClass::Deprecated(Arc::new(
                        ContractClass::from_path(ACCOUNT_CONTRACT_PATH).unwrap(),
                    )),
                ),
                (
                    felt_to_hash(&0x1010.into()),
                    CompiledClass::Deprecated(Arc::new(
                        ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap(),
                    )),
                ),
            ]),
            HashMap::new(),
        )),
        Arc::new(PermanentContractClassCache::default()),
    );
    state_before
        .cache_mut()
        .storage_initial_values_mut()
        .insert(
            (
                Address(0x1001.into()),
                TEST_ERC20_DEPLOYED_ACCOUNT_BALANCE_KEY
                    .clone()
                    .to_bytes_be(),
            ),
            *INITIAL_BALANCE,
        );

    let mut state_after = state_before.clone_for_testing();

    // Make the contract cache independent (otherwise tests will fail because the initial state's
    // cache will not be empty anymore).
    *state_after.contract_class_cache_mut() = Arc::new(PermanentContractClassCache::default());

    state_after.cache_mut().nonce_initial_values_mut().insert(
        Address(
            Felt252::from_dec_str(
                "386181506763903095743576862849245034886954647214831045800703908858571591162",
            )
            .unwrap(),
        ),
        Felt252::ZERO,
    );
    state_after
        .cache_mut()
        .class_hash_initial_values_mut()
        .insert(Address(0x1001.into()), felt_to_hash(&0x1010.into()));
    state_after.cache_mut().storage_initial_values_mut().insert(
        (
            Address(0x1001.into()),
            Felt252::from_dec_str(
                "2542253978940891427830343982984992363331567580652119103860970381451088310290",
            )
            .unwrap()
            .to_bytes_be(),
        ),
        Felt252::ZERO,
    );
    state_after.cache_mut().storage_initial_values_mut().insert(
        (
            Address(0x1001.into()),
            TEST_ERC20_BALANCE_KEY_2.clone().to_bytes_be(),
        ),
        Felt252::ZERO,
    );
    state_after.cache_mut().storage_initial_values_mut().insert(
        (
            Address(0x1001.into()),
            TEST_ERC20_SEQUENCER_BALANCE_KEY.clone().to_bytes_be(),
        ),
        Felt252::ZERO,
    );

    state_after.cache_mut().nonce_writes_mut().insert(
        Address(
            Felt252::from_dec_str(
                "386181506763903095743576862849245034886954647214831045800703908858571591162",
            )
            .unwrap(),
        ),
        1.into(),
    );
    state_after.cache_mut().class_hash_writes_mut().insert(
        Address(
            Felt252::from_dec_str(
                "386181506763903095743576862849245034886954647214831045800703908858571591162",
            )
            .unwrap(),
        ),
        felt_to_hash(&0x111.into()),
    );
    // Also set the previous value as initial_value for the class hash written by the deploy
    // This will be added by update_initial_values_of_write_only_accesses when counting storage changes
    state_after
        .cache_mut()
        .class_hash_initial_values_mut()
        .insert(
            Address(
                Felt252::from_dec_str(
                    "386181506763903095743576862849245034886954647214831045800703908858571591162",
                )
                .unwrap(),
            ),
            Felt252::ZERO.into(),
        );
    state_after.cache_mut().storage_writes_mut().insert(
        (
            Address(0x1001.into()),
            Felt252::from_dec_str(
                "2542253978940891427830343982984992363331567580652119103860970381451088310290",
            )
            .unwrap()
            .to_bytes_be(),
        ),
        Felt252::ZERO,
    );
    state_after.cache_mut().storage_writes_mut().insert(
        (
            Address(0x1001.into()),
            TEST_ERC20_DEPLOYED_ACCOUNT_BALANCE_KEY.to_bytes_be(),
        ),
        *INITIAL_BALANCE - fee,
    );
    state_after.cache_mut().storage_writes_mut().insert(
        (
            Address(0x1001.into()),
            TEST_ERC20_BALANCE_KEY_2.to_bytes_be(),
        ),
        Felt252::ZERO,
    );
    state_after.cache_mut().storage_writes_mut().insert(
        (
            Address(0x1001.into()),
            TEST_ERC20_SEQUENCER_BALANCE_KEY.to_bytes_be(),
        ),
        fee,
    );
    state_after
        .set_contract_class(
            &felt_to_hash(&0x1010.into()),
            &CompiledClass::Deprecated(Arc::new(
                ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap(),
            )),
        )
        .unwrap();
    state_after
        .set_contract_class(
            &felt_to_hash(&0x111.into()),
            &CompiledClass::Deprecated(Arc::new(
                ContractClass::from_path(ACCOUNT_CONTRACT_PATH).unwrap(),
            )),
        )
        .unwrap();

    (state_before, state_after)
}

#[test]
fn test_state_for_declare_tx() {
    let (block_context, mut state) = create_account_tx_test_state().unwrap();

    let declare_tx = declare_tx();
    // Check ContractClass is not set before the declare_tx
    assert!(state.get_contract_class(&declare_tx.class_hash).is_err());
    assert!(state
        .get_nonce_at(&declare_tx.sender_address)
        .unwrap()
        .is_zero());
    // Execute declare_tx
    let fee = declare_tx
        .execute(
            &mut state,
            &block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap()
        .actual_fee
        .into();
    assert_eq!(
        state.get_nonce_at(&declare_tx.sender_address).unwrap(),
        Felt252::ONE
    );

    // Check state.state_reader
    let state_reader = state.state_reader.clone();

    assert_eq!(
        state_reader.address_to_class_hash,
        HashMap::from([
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                *TEST_ERC20_CONTRACT_CLASS_HASH
            ),
            (TEST_CONTRACT_ADDRESS.clone(), *TEST_CLASS_HASH),
            (
                TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
                *TEST_ACCOUNT_CONTRACT_CLASS_HASH
            ),
        ]),
    );

    assert_eq!(
        state_reader.address_to_nonce,
        HashMap::from([
            (TEST_ERC20_CONTRACT_ADDRESS.clone(), Felt252::ZERO),
            (TEST_CONTRACT_ADDRESS.clone(), Felt252::ZERO),
            (TEST_ACCOUNT_CONTRACT_ADDRESS.clone(), Felt252::ZERO),
        ]),
    );

    assert_eq!(
        state_reader.address_to_storage,
        HashMap::from([(
            (
                TEST_ERC20_CONTRACT_ADDRESS.clone(),
                TEST_ERC20_ACCOUNT_BALANCE_KEY.to_bytes_be()
            ),
            *INITIAL_BALANCE
        ),]),
    );
    // We cant compare this until a new implementation of Eq for programs, due to a change in the hints_ranges.
    // assert_eq!(
    //     state_reader.class_hash_to_contract_class,
    //     HashMap::from([
    //         (
    //            TEST_ERC20_CONTRACT_CLASS_HASH.to_bytes_be(),
    //             ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap()
    //         ),
    //         (
    //             felt_to_hash(&TEST_CLASS_HASH),
    //             ContractClass::from_path(TEST_CONTRACT_PATH).unwrap()
    //         ),
    //         (
    //            TEST_ACCOUNT_CONTRACT_CLASS_HASH.to_bytes_be(),
    //             ContractClass::from_path(ACCOUNT_CONTRACT_PATH).unwrap()
    //         ),
    //     ])
    // );

    // Check state.cache
    assert_eq!(
        state.cache(),
        &StateCache::new(
            HashMap::from([
                (
                    TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
                    *TEST_ACCOUNT_CONTRACT_CLASS_HASH
                ),
                (
                    TEST_ERC20_CONTRACT_ADDRESS.clone(),
                    *TEST_ERC20_CONTRACT_CLASS_HASH
                )
            ]),
            HashMap::new(),
            HashMap::from([(TEST_ACCOUNT_CONTRACT_ADDRESS.clone(), 0.into())]),
            HashMap::from([
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_BALANCE_KEY_2.clone().to_bytes_be()
                    ),
                    0.into()
                ),
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_BALANCE_KEY_1.clone().to_bytes_be()
                    ),
                    0.into()
                ),
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_SEQUENCER_BALANCE_KEY.clone().to_bytes_be()
                    ),
                    0.into()
                ),
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be()
                    ),
                    *INITIAL_BALANCE
                )
            ]),
            HashMap::new(),
            HashMap::new(),
            HashMap::from([(TEST_ACCOUNT_CONTRACT_ADDRESS.clone(), 1.into())]),
            HashMap::from([
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_BALANCE_KEY_2.clone().to_bytes_be()
                    ),
                    0.into()
                ),
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_BALANCE_KEY_1.clone().to_bytes_be()
                    ),
                    0.into()
                ),
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_SEQUENCER_BALANCE_KEY.clone().to_bytes_be()
                    ),
                    fee,
                ),
                (
                    (
                        TEST_ERC20_CONTRACT_ADDRESS.clone(),
                        TEST_ERC20_ACCOUNT_BALANCE_KEY.clone().to_bytes_be()
                    ),
                    *INITIAL_BALANCE - fee,
                ),
            ]),
            HashMap::new()
        ),
    );

    // We cant compare this until a new implementation of Eq for programs, due to a change in the hints_ranges.
    // assert_eq!(
    //     state.contract_classes(),
    //     &Some(HashMap::from([
    //         (
    //             felt_to_hash(&TEST_EMPTY_CONTRACT_CLASS_HASH),
    //             ContractClass::from_path(TEST_EMPTY_CONTRACT_PATH).unwrap()
    //         ),
    //         (
    //             felt_to_hash(&TEST_ERC20_CONTRACT_CLASS_HASH),
    //             ContractClass::from_path(ERC20_CONTRACT_PATH).unwrap()
    //         ),
    //         (
    //            TEST_ACCOUNT_CONTRACT_CLASS_HASH.to_bytes_be(),
    //             ContractClass::from_path(ACCOUNT_CONTRACT_PATH).unwrap()
    //         ),
    //     ]))
    // );
}

#[test]
fn test_invoke_tx_wrong_call_data() {
    let (starknet_general_context, state) = &mut create_account_tx_test_state().unwrap();

    // Calldata with missing inputs
    let calldata = vec![
        TEST_CONTRACT_ADDRESS.clone().0, // CONTRACT_ADDRESS
        Felt252::from_bytes_be(&calculate_sn_keccak(b"return_result")), // CONTRACT FUNCTION SELECTOR
        Felt252::from(1),                                               // CONTRACT_CALLDATA LEN
                                                                        // CONTRACT_CALLDATA
    ];
    let invoke_tx = invoke_tx(calldata, u128::MAX);

    // Execute transaction
    let result = invoke_tx.execute(
        state,
        starknet_general_context,
        0,
        #[cfg(feature = "cairo-native")]
        None,
    );

    // Assert error
    assert_matches!(
        result,
        Err(TransactionError::CairoRunner(CairoRunError::VmException(
            VmException {
                inner_exc: VirtualMachineError::DiffAssertValues(..),
                ..
            }
        )))
    );
}

#[test]
fn test_invoke_tx_wrong_entrypoint() {
    let (starknet_general_context, state) = &mut create_account_tx_test_state().unwrap();
    let Address(test_contract_address) = TEST_CONTRACT_ADDRESS.clone();

    // Invoke transaction with an entrypoint that doesn't exists
    let invoke_tx = InvokeFunction::new(
        TEST_ACCOUNT_CONTRACT_ADDRESS.clone(),
        // Entrypoiont that doesnt exits in the contract
        Felt252::from_bytes_be(&calculate_sn_keccak(b"none_function")),
        VersionSpecificAccountTxFields::new_deprecated(2483),
        *TRANSACTION_VERSION,
        vec![
            test_contract_address, // CONTRACT_ADDRESS
            Felt252::from_bytes_be(&calculate_sn_keccak(b"return_result")), // CONTRACT FUNCTION SELECTOR
            Felt252::from(1),                                               // CONTRACT_CALLDATA LEN
            Felt252::from(2),                                               // CONTRACT_CALLDATA
        ],
        vec![],
        StarknetChainId::TestNet.to_felt(),
        Some(Felt252::ZERO),
    )
    .unwrap();

    // Execute transaction
    let result = invoke_tx.execute(
        state,
        starknet_general_context,
        0,
        #[cfg(feature = "cairo-native")]
        None,
    );

    // Assert error
    assert_matches!(result, Err(TransactionError::EntryPointNotFound(_)));
}

#[test]
fn test_deploy_undeclared_account() {
    let (block_context, mut state) = create_account_tx_test_state().unwrap();

    let not_deployed_class_hash = ClassHash([1; 32]);
    // Deploy transaction with a not_deployed_class_hash class_hash
    let deploy_account_tx = DeployAccount::new(
        not_deployed_class_hash,
        Default::default(),
        *TRANSACTION_VERSION,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        StarknetChainId::TestNet.to_felt(),
    )
    .unwrap();

    // Check not_deployed_class_hash
    assert!(state.get_contract_class(&not_deployed_class_hash).is_err());

    // Execute transaction
    let result = deploy_account_tx.execute(
        &mut state,
        &block_context,
        #[cfg(feature = "cairo-native")]
        None,
    );

    // Execute transaction
    assert_matches!(
        result,
        Err(TransactionError::State(StateError::NoneCompiledHash(_)))
    );
}

#[test]
fn test_library_call_with_declare_v2() {
    let (block_context, state) = &mut create_account_tx_test_state().unwrap();

    // Declare the fibonacci contract
    let declare_tx = declarev2_tx();
    declare_tx
        .execute(
            state,
            block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    // Deploy the fibonacci contract
    let deploy = deploy_fib_syscall();
    deploy
        .execute(
            state,
            block_context,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

    //  Create program and entry point types for contract class
    let program_data = include_bytes!("../../starknet_programs/cairo2/fibonacci_dispatcher.casm");
    let contract_class: CasmContractClass = serde_json::from_slice(program_data).unwrap();
    let entrypoints = contract_class.clone().entry_points_by_type;
    let external_entrypoint_selector = &entrypoints.external.get(0).unwrap().selector;

    let address = Address(6666.into());
    let mut class_hash: ClassHash = ClassHash([0; 32]);
    class_hash.0[0] = 1;
    let nonce = Felt252::ZERO;

    state
        .cache_mut()
        .class_hash_initial_values_mut()
        .insert(address.clone(), class_hash);

    state
        .cache_mut()
        .nonce_initial_values_mut()
        .insert(address.clone(), nonce);

    state
        .set_contract_class(
            &class_hash,
            &CompiledClass::Casm {
                casm: Arc::new(contract_class),
                sierra: None,
            },
        )
        .unwrap();

    let create_execute_extrypoint = |selector: &BigUint,
                                     calldata: Vec<Felt252>,
                                     entry_point_type: EntryPointType|
     -> ExecutionEntryPoint {
        ExecutionEntryPoint::new(
            address.clone(),
            calldata,
            Felt252::from(selector),
            Address(0000.into()),
            entry_point_type,
            Some(CallType::Delegate),
            Some(class_hash),
            1000000000,
        )
    };

    // Create an execution entry point
    let calldata = vec![
        Felt252::from_bytes_be(&TEST_FIB_COMPILED_CONTRACT_CLASS_HASH.0),
        Felt252::from_bytes_be(&calculate_sn_keccak(b"fib")),
        1.into(),
        1.into(),
        10.into(),
    ];
    let send_message_exec_entry_point = create_execute_extrypoint(
        external_entrypoint_selector,
        calldata.clone(),
        EntryPointType::External,
    );

    // Execute the entrypoint
    let block_context = BlockContext::default();
    let mut tx_execution_context = TransactionExecutionContext::new(
        Address(0.into()),
        Felt252::ZERO,
        Vec::new(),
        VersionSpecificAccountTxFields::new_deprecated(100000000),
        10.into(),
        block_context.invoke_tx_max_n_steps(),
        *TRANSACTION_VERSION,
    );
    let mut resources_manager = ExecutionResourcesManager::default();

    // Run send_msg entrypoint
    let call_info = send_message_exec_entry_point
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
        .unwrap();

    let expected_internal_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: address.clone(),
        class_hash: Some(*TEST_FIB_COMPILED_CONTRACT_CLASS_HASH),
        entry_point_selector: Some(Felt252::from(external_entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        gas_consumed: 19680,
        calldata: vec![1.into(), 1.into(), 10.into()],
        retdata: vec![89.into()], // fib(10)
        execution_resources: Some(ExecutionResources {
            n_steps: 269,
            n_memory_holes: 0,
            builtin_instance_counter: HashMap::from([("range_check_builtin".to_string(), 13)]),
        }),
        ..Default::default()
    };

    let expected_call_info = CallInfo {
        caller_address: Address(0.into()),
        call_type: Some(CallType::Delegate),
        contract_address: address.clone(),
        class_hash: Some(class_hash),
        entry_point_selector: Some(Felt252::from(external_entrypoint_selector)),
        entry_point_type: Some(EntryPointType::External),
        gas_consumed: 100490,
        calldata,
        retdata: vec![89.into()], // fib(10)
        execution_resources: Some(ExecutionResources {
            n_steps: 463,
            n_memory_holes: 1,
            builtin_instance_counter: HashMap::from([("range_check_builtin".to_string(), 16)]),
        }),
        internal_calls: vec![expected_internal_call_info],
        ..Default::default()
    };

    assert_eq!(call_info.call_info.unwrap(), expected_call_info);
}
