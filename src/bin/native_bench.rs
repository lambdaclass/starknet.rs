// Usage:
// For executing the binary with cairo native you can run it like:
// $ native_bench <n_executions> native
// otherwise it will run it using the Cairo VM
// native_bench <n_executions>
// You can also choose which benchmark to run by passing the name of the benchmark as the third argument:
// $ native_bench <n_executions> native <fibo|fact>
// where fibo executes a fibonacci function and fact a factorial n times.

use cairo_native::cache::AotProgramCache;
use cairo_native::cache::JitProgramCache;
use cairo_native::cache::ProgramCache;
use cairo_native::context::NativeContext;
use cairo_vm::Felt252;
use lazy_static::lazy_static;
use starknet_in_rust::definitions::block_context::BlockContext;
use starknet_in_rust::definitions::block_context::StarknetChainId;
use starknet_in_rust::services::api::contract_classes::compiled_class::CompiledClass;
use starknet_in_rust::state::contract_class_cache::ContractClassCache;
use starknet_in_rust::state::contract_class_cache::PermanentContractClassCache;
use starknet_in_rust::state::state_api::State;
use starknet_in_rust::transaction::DeployAccount;
use starknet_in_rust::utils::calculate_sn_keccak;
use starknet_in_rust::utils::get_native_context;
use starknet_in_rust::CasmContractClass;
use starknet_in_rust::EntryPointType;
use starknet_in_rust::{
    definitions::constants::TRANSACTION_VERSION,
    execution::{
        execution_entry_point::ExecutionEntryPoint, CallInfo, CallType, TransactionExecutionContext,
    },
    state::cached_state::CachedState,
    state::{in_memory_state_reader::InMemoryStateReader, ExecutionResourcesManager},
    transaction::{Address, ClassHash},
};
use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

enum BenchType {
    VM,
    Jit,
    Aot,
}

impl FromStr for BenchType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "vm" => Ok(Self::VM),
            "aot" => Ok(Self::Aot),
            "jit" => Ok(Self::Jit),
            _ => Err(()),
        }
    }
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(3).map(|s| s.as_str()) {
        Some("fib") => bench_fibo(
            args.get(1)
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(1),
            BenchType::from_str(args.get(2).expect("missing argument"))
                .expect("invalid bench type"),
        ),
        Some("fact") => bench_fact(
            args.get(1)
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(1),
            BenchType::from_str(args.get(2).expect("missing argument"))
                .expect("invalid bench type"),
        ),
        Some("erc20") => bench_erc20(
            args.get(1)
                .and_then(|x| x.parse::<usize>().ok())
                .unwrap_or(1),
            BenchType::from_str(args.get(2).expect("missing argument"))
                .expect("invalid bench type"),
        ),
        _ => panic!("missing bench name"),
    }
}

fn bench_fibo(executions: usize, bench_type: BenchType) {
    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();
    static CASM_CLASS_HASH: ClassHash = ClassHash([2; 32]);

    let casm_data = include_bytes!("../../starknet_programs/cairo2/fibonacci.casm");
    let casm_contract_class: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

    let (contract_class, constructor_selector) = match bench_type {
        BenchType::VM => {
            let entrypoints = casm_contract_class.clone().entry_points_by_type;
            let constructor_selector = entrypoints.external.first().unwrap().selector.clone();

            (
                CompiledClass::Casm {
                    casm: Arc::new(casm_contract_class),
                    sierra: None,
                },
                constructor_selector,
            )
        }
        _ => {
            let sierra_data = include_bytes!("../../starknet_programs/cairo2/fibonacci.sierra");
            let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
                serde_json::from_slice(sierra_data).unwrap();

            let entrypoints = sierra_contract_class.clone().entry_points_by_type;
            let constructor_selector = entrypoints.external.first().unwrap().selector.clone();
            let sierra_program = sierra_contract_class.extract_sierra_program().unwrap();
            let entrypoints = sierra_contract_class.entry_points_by_type;
            (
                CompiledClass::Casm {
                    casm: Arc::new(casm_contract_class),
                    sierra: Some(Arc::new((sierra_program, entrypoints))),
                },
                constructor_selector,
            )
        }
    };

    let caller_address = Address(123456789.into());

    contract_class_cache.set_contract_class(CASM_CLASS_HASH, contract_class);
    let mut state_reader = InMemoryStateReader::default();
    let nonce = Felt252::ZERO;

    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), CASM_CLASS_HASH);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let state_reader = Arc::new(state_reader);
    let state = CachedState::new(state_reader, Arc::new(contract_class_cache));

    /* f0, f1, N */
    let mut calldata = [1.into(), 1.into(), 2000000.into()];

    let native_ctx = NativeContext::new();
    let program_cache = Rc::new(RefCell::new(match bench_type {
        BenchType::Aot => ProgramCache::Aot(AotProgramCache::new(&native_ctx)),
        BenchType::Jit => ProgramCache::Jit(JitProgramCache::new(&native_ctx)),
        BenchType::VM => ProgramCache::Jit(JitProgramCache::new(&native_ctx)),
    }));

    let constructor_selector: Felt252 = (&constructor_selector).into();

    for _ in 0..executions {
        calldata[2] = &calldata[2] + 1;
        let result = execute(
            &mut state.clone_for_testing(),
            &caller_address,
            &caller_address,
            &constructor_selector,
            &calldata,
            EntryPointType::External,
            &CASM_CLASS_HASH,
            program_cache.clone(),
        );

        _ = std::hint::black_box(result);
    }
}

fn bench_fact(executions: usize, bench_type: BenchType) {
    // Create state reader with class hash data
    let contract_class_cache = PermanentContractClassCache::default();
    static CASM_CLASS_HASH: ClassHash = ClassHash([2; 32]);

    let casm_data = include_bytes!("../../starknet_programs/cairo2/factorial_tr.casm");
    let casm_contract_class: CasmContractClass = serde_json::from_slice(casm_data).unwrap();

    let (contract_class, constructor_selector) = match bench_type {
        BenchType::VM => {
            let entrypoints = casm_contract_class.clone().entry_points_by_type;
            let constructor_selector = entrypoints.external.first().unwrap().selector.clone();

            (
                CompiledClass::Casm {
                    casm: Arc::new(casm_contract_class),
                    sierra: None,
                },
                constructor_selector,
            )
        }
        _ => {
            let sierra_data = include_bytes!("../../starknet_programs/cairo2/factorial_tr.sierra");
            let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
                serde_json::from_slice(sierra_data).unwrap();

            let entrypoints = sierra_contract_class.clone().entry_points_by_type;
            let constructor_selector = entrypoints.external.first().unwrap().selector.clone();
            let sierra_program = sierra_contract_class.extract_sierra_program().unwrap();
            let entrypoints = sierra_contract_class.entry_points_by_type;
            (
                CompiledClass::Casm {
                    casm: Arc::new(casm_contract_class),
                    sierra: Some(Arc::new((sierra_program, entrypoints))),
                },
                constructor_selector,
            )
        }
    };

    let caller_address = Address(123456789.into());
    // FACT 1M
    // FIBO 2M

    contract_class_cache.set_contract_class(CASM_CLASS_HASH, contract_class);
    let mut state_reader = InMemoryStateReader::default();
    let nonce = Felt252::ZERO;

    state_reader
        .address_to_class_hash_mut()
        .insert(caller_address.clone(), CASM_CLASS_HASH);
    state_reader
        .address_to_nonce_mut()
        .insert(caller_address.clone(), nonce);

    // Create state from the state_reader and contract cache.
    let state_reader = Arc::new(state_reader);
    let state = CachedState::new(state_reader, Arc::new(contract_class_cache));

    /* N */
    let mut calldata = [2000000.into()];

    let native_ctx = NativeContext::new();
    let program_cache = Rc::new(RefCell::new(match bench_type {
        BenchType::Aot => ProgramCache::Aot(AotProgramCache::new(&native_ctx)),
        BenchType::Jit => ProgramCache::Jit(JitProgramCache::new(&native_ctx)),
        _ => unreachable!(),
    }));

    let constructor_selector: Felt252 = (&constructor_selector).into();

    for _ in 0..executions {
        calldata[0] = &calldata[0] + 1;
        let result = execute(
            &mut state.clone_for_testing(),
            &caller_address,
            &caller_address,
            &constructor_selector,
            &calldata,
            EntryPointType::External,
            &CASM_CLASS_HASH,
            program_cache.clone(),
        );

        _ = std::hint::black_box(result);
    }
}

fn bench_erc20(executions: usize, bench_type: BenchType) {
    // 1. setup ERC20 contract and state.
    // Create state reader and preload the contract classes.
    let contract_class_cache = PermanentContractClassCache::default();

    lazy_static! {
        static ref ERC20_CLASS_HASH: ClassHash =
            ClassHash::from(Felt252::from_dec_str("2").unwrap());
        static ref DEPLOYER_CLASS_HASH: ClassHash =
            ClassHash::from(Felt252::from_dec_str("10").unwrap());
        static ref ACCOUNT1_CLASS_HASH: ClassHash =
            ClassHash::from(Felt252::from_dec_str("1").unwrap());
        static ref DEPLOYER_ADDRESS: Address = Address(1111.into());
        static ref ERC20_NAME: Felt252 = Felt252::from_bytes_be_slice(b"be");
        static ref ERC20_SYMBOL: Felt252 = Felt252::from_bytes_be_slice(b"be");
        static ref ERC20_DECIMALS: Felt252 = Felt252::from(24);
        static ref ERC20_INITIAL_SUPPLY: Felt252 = Felt252::from(1_000_000);
        static ref ERC20_RECIPIENT: Felt252 = Felt252::from_dec_str("111").unwrap();
        static ref ERC20_SALT: Felt252 = Felt252::from_dec_str("1234").unwrap();
        static ref ERC20_DEPLOYER_CALLDATA: [Felt252; 7] = [
            Felt252::from_bytes_be(&ERC20_CLASS_HASH.0),
            *ERC20_SALT,
            *ERC20_RECIPIENT,
            *ERC20_NAME,
            *ERC20_DECIMALS,
            *ERC20_INITIAL_SUPPLY,
            *ERC20_SYMBOL,
        ];
        static ref ERC20_DEPLOYMENT_CALLER_ADDRESS: Address = Address(0000.into());
    }

    let program_cache = Rc::new(RefCell::new(match bench_type {
        BenchType::Aot => ProgramCache::Aot(AotProgramCache::new(get_native_context())),
        BenchType::Jit => ProgramCache::Jit(JitProgramCache::new(get_native_context())),
        BenchType::VM => ProgramCache::Jit(JitProgramCache::new(get_native_context())),
    }));

    // read the ERC20 contract class
    let erc20_casm_class = include_bytes!("../../starknet_programs/cairo2/erc20.casm");
    let casm_contract_class: CasmContractClass = serde_json::from_slice(erc20_casm_class).unwrap();

    // we also need to read the contract class of the deployERC20 contract.
    // this contract is used as a deployer of the erc20.
    let erc20_deployer_code = include_bytes!("../../starknet_programs/cairo2/deploy_erc20.casm");
    let erc20_deployer_class: CasmContractClass =
        serde_json::from_slice(erc20_deployer_code).unwrap();

    let (erc20_address, mut state): (
        Address,
        CachedState<InMemoryStateReader, PermanentContractClassCache>,
    ) = match bench_type {
        BenchType::VM => {
            let erc20_contract_class = CompiledClass::Casm {
                casm: Arc::new(casm_contract_class),
                sierra: None,
            };

            let entrypoints = erc20_deployer_class.clone().entry_points_by_type;
            let deploy_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

            // insert deployer and erc20 classes into the cache.
            contract_class_cache.set_contract_class(
                *DEPLOYER_CLASS_HASH,
                CompiledClass::Casm {
                    casm: Arc::new(erc20_deployer_class),
                    sierra: None,
                },
            );
            contract_class_cache.set_contract_class(*ERC20_CLASS_HASH, erc20_contract_class);

            let mut state_reader = InMemoryStateReader::default();
            // setup deployer nonce and address into the state reader
            state_reader
                .address_to_class_hash_mut()
                .insert(DEPLOYER_ADDRESS.clone(), *DEPLOYER_CLASS_HASH);
            state_reader
                .address_to_nonce_mut()
                .insert(DEPLOYER_ADDRESS.clone(), Felt252::ZERO);

            // Create state from the state_reader and contract cache.
            let mut state =
                CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

            // deploy the erc20 contract by calling the deployer contract.

            let exec_entry_point = ExecutionEntryPoint::new(
                DEPLOYER_ADDRESS.clone(),
                ERC20_DEPLOYER_CALLDATA.to_vec(),
                Felt252::from(deploy_entrypoint_selector),
                ERC20_DEPLOYMENT_CALLER_ADDRESS.clone(),
                EntryPointType::External,
                Some(CallType::Delegate),
                Some(*DEPLOYER_CLASS_HASH),
                100_000_000_000,
            );

            // create required structures for execution
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

            // execute the deployment
            let call_info = exec_entry_point
                .execute(
                    &mut state,
                    &block_context,
                    &mut resources_manager,
                    &mut tx_execution_context,
                    false,
                    block_context.invoke_tx_max_n_steps(),
                    Some(program_cache.clone()),
                )
                .unwrap();

            // obtain the address of the deployed erc20 contract
            let erc20_address = *call_info.call_info.unwrap().retdata.first().unwrap();

            (Address(erc20_address), state)
        }
        _ => {
            let erc20_sierra_class = include_bytes!("../../starknet_programs/cairo2/erc20.sierra");
            let sierra_contract_class: cairo_lang_starknet_classes::contract_class::ContractClass =
                serde_json::from_slice(erc20_sierra_class).unwrap();
            let sierra_program = sierra_contract_class.extract_sierra_program().unwrap();
            let entrypoints = sierra_contract_class.entry_points_by_type;
            let erc20_contract_class = CompiledClass::Casm {
                casm: Arc::new(casm_contract_class),
                sierra: Some(Arc::new((sierra_program, entrypoints))),
            };

            let entrypoints = erc20_deployer_class.clone().entry_points_by_type;
            let deploy_entrypoint_selector = &entrypoints.external.first().unwrap().selector;

            // insert deployer and erc20 classes into the cache.
            contract_class_cache.set_contract_class(
                *DEPLOYER_CLASS_HASH,
                CompiledClass::Casm {
                    casm: Arc::new(erc20_deployer_class),
                    sierra: None,
                },
            );
            contract_class_cache.set_contract_class(*ERC20_CLASS_HASH, erc20_contract_class);

            let mut state_reader = InMemoryStateReader::default();
            // setup deployer nonce and address into the state reader
            state_reader
                .address_to_class_hash_mut()
                .insert(DEPLOYER_ADDRESS.clone(), *DEPLOYER_CLASS_HASH);
            state_reader
                .address_to_nonce_mut()
                .insert(DEPLOYER_ADDRESS.clone(), Felt252::ZERO);

            // Create state from the state_reader and contract cache.
            let mut state =
                CachedState::new(Arc::new(state_reader), Arc::new(contract_class_cache));

            // deploy the erc20 contract by calling the deployer contract.

            let exec_entry_point = ExecutionEntryPoint::new(
                DEPLOYER_ADDRESS.clone(),
                ERC20_DEPLOYER_CALLDATA.to_vec(),
                Felt252::from(deploy_entrypoint_selector),
                ERC20_DEPLOYMENT_CALLER_ADDRESS.clone(),
                EntryPointType::External,
                Some(CallType::Delegate),
                Some(*DEPLOYER_CLASS_HASH),
                100_000_000_000,
            );

            // create required structures for execution
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

            // execute the deployment
            let call_info = exec_entry_point
                .execute(
                    &mut state,
                    &block_context,
                    &mut resources_manager,
                    &mut tx_execution_context,
                    false,
                    block_context.invoke_tx_max_n_steps(),
                    Some(program_cache.clone()),
                )
                .unwrap();

            // obtain the address of the deployed erc20 contract
            let erc20_address = *call_info.call_info.unwrap().retdata.first().unwrap();

            (Address(erc20_address), state)
        }
    };

    // 2. setup accounts (here we need to execute a deploy_account,
    //    so we execute it in the vm only).
    //    Further executions (transfers) will be executed with Native.
    //    (or the VM, depending on configuration)
    // 2a. setup for first account:
    let account_casm_file =
        include_bytes!("../../starknet_programs/cairo2/hello_world_account.casm");
    let account_contract_class: CasmContractClass =
        serde_json::from_slice(account_casm_file).unwrap();

    state
        .set_contract_class(
            &ACCOUNT1_CLASS_HASH,
            &CompiledClass::Casm {
                casm: Arc::new(account_contract_class),
                sierra: None,
            },
        )
        .unwrap();
    state
        .set_compiled_class_hash(
            &Felt252::from_bytes_be(&ACCOUNT1_CLASS_HASH.0),
            &Felt252::from_bytes_be(&ACCOUNT1_CLASS_HASH.0),
        )
        .unwrap();

    let contract_address_salt = Felt252::from_dec_str(
        "2669425616857739096022668060305620640217901643963991674344872184515580705509",
    )
    .unwrap();

    // create a transaction for deploying the first account
    let account1_deploy_tx = DeployAccount::new(
        *ACCOUNT1_CLASS_HASH, // class hash
        Default::default(),   // max fee
        1.into(),             // tx version
        Felt252::ZERO,        // nonce
        vec![2.into()],       // constructor calldata
        vec![
            Felt252::from_dec_str(
                "3233776396904427614006684968846859029149676045084089832563834729503047027074",
            )
            .unwrap(),
            Felt252::from_dec_str(
                "707039245213420890976709143988743108543645298941971188668773816813012281203",
            )
            .unwrap(),
        ], // signature
        contract_address_salt, // salt
        StarknetChainId::TestNet.to_felt(), // network
    )
    .unwrap();

    // execute the deploy_account transaction.
    // this will create the account and after that,
    // we can extract its address.
    let account1_address = account1_deploy_tx
        .execute(&mut state, &Default::default(), Some(program_cache.clone()))
        .expect("failed to execute the deployment of account 1")
        .validate_info
        .expect("validate_info missing")
        .contract_address;

    // now we need to deploy account2
    let account2_deploy_tx = DeployAccount::new(
        *ACCOUNT1_CLASS_HASH, // class hash
        Default::default(),   // max fee
        1.into(),             // tx version
        Felt252::ZERO,        // nonce
        vec![3.into()],       // constructor calldata
        vec![
            Felt252::from_dec_str(
                "3233776396904427614006684968846859029149676045084089832563834729503047027074",
            )
            .unwrap(),
            Felt252::from_dec_str(
                "707039245213420890976709143988743108543645298941971188668773816813012281203",
            )
            .unwrap(),
        ], // signature
        contract_address_salt, // salt
        StarknetChainId::TestNet.to_felt(), // network
    )
    .unwrap();

    // execute the deploy_account transaction and retrieve the deployed account address.
    let _account2_address = account2_deploy_tx
        .execute(&mut state, &Default::default(), Some(program_cache.clone()))
        .expect("failed to execute the deployment of account 2")
        .validate_info
        .expect("validate_info missing")
        .contract_address;

    // 4. do transfers between the accounts

    let transfer_entrypoint_selector = Felt252::from_bytes_be(&calculate_sn_keccak(b"transfer"));
    // calldata for transfering 123 tokens from account1 to account2
    let calldata = vec![Felt252::from(12), Felt252::from(123)];

    for _ in 0..executions {
        let result = execute(
            &mut state.clone_for_testing(),
            &account1_address,
            &erc20_address,
            &transfer_entrypoint_selector,
            &calldata.clone(),
            EntryPointType::External,
            &ERC20_CLASS_HASH,
            program_cache.clone(),
        );

        _ = std::hint::black_box(result);
    }
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn execute(
    state: &mut CachedState<InMemoryStateReader, PermanentContractClassCache>,
    caller_address: &Address,
    callee_address: &Address,
    selector: &Felt252,
    calldata: &[Felt252],
    entrypoint_type: EntryPointType,
    class_hash: &ClassHash,
    program_cache: Rc<RefCell<ProgramCache<'_, ClassHash>>>,
) -> CallInfo {
    let exec_entry_point = ExecutionEntryPoint::new(
        (*callee_address).clone(),
        calldata.to_vec(),
        *selector,
        (*caller_address).clone(),
        entrypoint_type,
        Some(CallType::Delegate),
        Some(*class_hash),
        u64::MAX.into(), // gas is u64 in cairo-native and sierra
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
            Some(program_cache),
        )
        .unwrap()
        .call_info
        .unwrap()
}
