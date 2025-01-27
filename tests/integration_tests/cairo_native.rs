#![cfg(feature = "cairo-native")]

use assert_matches::assert_matches;
use cairo_vm::Felt252;
use pretty_assertions_sorted::*;
use starknet_in_rust::definitions::block_context::StarknetOsConfig;
use starknet_in_rust::execution::TransactionExecutionInfo;
use starknet_in_rust::hash_utils::calculate_contract_address;
use starknet_in_rust::transaction::Transaction;
use starknet_in_rust::{
    definitions::{block_context::BlockContext, constants::TRANSACTION_VERSION},
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        in_memory_state_reader::InMemoryStateReader,
        ExecutionResourcesManager,
    },
    transaction::{Address, ClassHash},
    utils::{calculate_sn_keccak, felt_to_hash},
    CasmContractClass, ContractClass as SierraContractClass, EntryPointType,
};
use std::{fs, path::Path, sync::Arc};

#[test]
fn get_block_hash_test() {
    let class_hash = ClassHash([1; 32]);
    let caller_address = Address(1.into());
    let callee_address = Address(1.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            caller_address.clone(),
            "starknet_programs/cairo2/get_block_hash_basic.cairo",
        )
        .unwrap();

    let mut state = state.finalize();
    state.insert_initial_storage_value(
        (Address(Felt252::ONE), felt_to_hash(&10.into()).0),
        Felt252::from_bytes_be(&[5; 32]),
    );

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_hex(
                    "377ae94b690204c74c8d21938c5b72e80fdaee3d21c780fd7557a7f84a8b379",
                )
                .unwrap(),
            ),
            &[
                10.into(), // Block number.
            ],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
#[cfg(feature = "cairo-native")]
fn get_block_hash_test_failure2() {
    let class_hash = ClassHash([1; 32]);
    let caller_address = Address(1.into());
    let callee_address = Address(1.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            caller_address.clone(),
            "starknet_programs/cairo2/get_block_hash_basic.cairo",
        )
        .unwrap();

    let mut state = state.finalize();
    state.insert_initial_storage_value(
        (Address(Felt252::ONE), felt_to_hash(&10.into()).0),
        Felt252::from_bytes_be(&[5; 32]),
    );

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_hex(
                    "377ae94b690204c74c8d21938c5b72e80fdaee3d21c780fd7557a7f84a8b379",
                )
                .unwrap(),
            ),
            &[
                25.into(), // block number (is not inside a valid range)
            ],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn integration_test_erc20() {
    let class_hash = ClassHash([1; 32]);
    let caller_address = Address(1.into());
    let callee_address = Address(1.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            caller_address.clone(),
            "starknet_programs/cairo2/erc20.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::Constructor,
                &Felt252::from_bytes_be(&calculate_sn_keccak("constructor".as_bytes())),
            ),
            &[
                caller_address.0, // recipient
                2.into(),         // name
                3.into(),         // decimals
                4.into(),         // initial_supply
                5.into(),         // symbol
            ],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET TOTAL SUPPLY -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_total_supply".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET DECIMALS -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_decimals".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET NAME -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_name".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET SYMBOL -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_symbol".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET BALANCE OF CALLER -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("balance_of".as_bytes())),
            ),
            &[caller_address.0],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- ALLOWANCE OF ADDRESS 1 -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("allowance".as_bytes())),
            ),
            &[caller_address.0, 1.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- INCREASE ALLOWANCE OF ADDRESS 1 by 10_000 -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("increase_allowance".as_bytes())),
            ),
            &[1.into(), 10_000.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- ALLOWANCE OF ADDRESS 1 -----------------

    // Checking again because allowance changed with previous call.
    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("allowance".as_bytes())),
            ),
            &[caller_address.0, 1.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // ---------------- APPROVE ADDRESS 1 TO MAKE TRANSFERS ON BEHALF OF THE CALLER ----------------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("approve".as_bytes())),
            ),
            &[1.into(), 5000.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // ---------------- TRANSFER 3 TOKENS FROM CALLER TO ADDRESS 2 ---------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("transfer".as_bytes())),
            ),
            &[2.into(), 3.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET BALANCE OF CALLER -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("balance_of".as_bytes())),
            ),
            &[caller_address.0],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET BALANCE OF ADDRESS 2 -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("balance_of".as_bytes())),
            ),
            &[2.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // ---------------- TRANSFER 1 TOKEN FROM CALLER TO ADDRESS 2, CALLED FROM ADDRESS 1 ----------------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("transfer_from".as_bytes())),
            ),
            &[1.into(), 2.into(), 1.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET BALANCE OF ADDRESS 2 -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("balance_of".as_bytes())),
            ),
            &[2.into()],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // --------------- GET BALANCE OF CALLER -----------------

    let (result_vm, result_native) = state
        .execute(
            &callee_address,
            &caller_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("balance_of".as_bytes())),
            ),
            &[caller_address.0],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn call_contract_test() {
    let class_hash = ClassHash([1; 32]);
    let caller_address = Address(1.into());
    let callee_class_hash = ClassHash([2; 32]);
    let callee_address = Address(2.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            caller_address.clone(),
            "starknet_programs/cairo2/caller.cairo",
        )
        .unwrap();

    state
        .load_contract_at_address(
            callee_class_hash,
            callee_address.clone(),
            "starknet_programs/cairo2/callee.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &caller_address,
            &callee_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("call_callee_contract".as_bytes())),
            ),
            &[Felt252::from_bytes_be(&calculate_sn_keccak(
                "return_44".as_bytes(),
            ))],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn call_echo_contract_test() {
    let class_hash = ClassHash([1; 32]);
    let caller_address = Address(1.into());
    let callee_class_hash = ClassHash([2; 32]);
    let callee_address = Address(2.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            caller_address.clone(),
            "starknet_programs/cairo2/echo_caller.cairo",
        )
        .unwrap();

    state
        .load_contract_at_address(
            callee_class_hash,
            callee_address.clone(),
            "starknet_programs/cairo2/echo.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &caller_address,
            &callee_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("call_echo_contract".as_bytes())),
            ),
            &[
                Felt252::from_bytes_be(&calculate_sn_keccak("echo".as_bytes())),
                99999999.into(),
            ],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn call_events_contract_test() {
    let class_hash = ClassHash([1; 32]);
    let caller_address = Address(1.into());
    let callee_class_hash = ClassHash([2; 32]);
    let callee_address = Address(2.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            caller_address.clone(),
            "starknet_programs/cairo2/caller.cairo",
        )
        .unwrap();

    state
        .load_contract_at_address(
            callee_class_hash,
            callee_address.clone(),
            "starknet_programs/cairo2/event_emitter.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &caller_address,
            &callee_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("call_callee_contract".as_bytes())),
            ),
            &[Felt252::from_bytes_be(&calculate_sn_keccak(
                "trigger_event".as_bytes(),
            ))],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn replace_class_test() {
    let class_hash_a = ClassHash([1; 32]);
    let address = Address(1.into());
    let class_hash_b = ClassHash([2; 32]);

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash_a,
            address.clone(),
            "starknet_programs/cairo2/get_number_a.cairo",
        )
        .unwrap();

    state
        .load_contract(class_hash_b, "starknet_programs/cairo2/get_number_b.cairo")
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &address,
            &address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("upgrade".as_bytes())),
            ),
            &[Felt252::from_bytes_be(&class_hash_b.0)],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn replace_class_contract_call_test() {
    let class_hash_a = ClassHash([1; 32]);
    let get_number_address = Address(1.into());
    let class_hash_b = ClassHash([2; 32]);
    let wrapper_address = Address(3.into());
    let class_hash_wrapper = ClassHash([3; 32]);

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash_a,
            get_number_address.clone(),
            "starknet_programs/cairo2/get_number_a.cairo",
        )
        .unwrap();

    state
        .load_contract(class_hash_b, "starknet_programs/cairo2/get_number_b.cairo")
        .unwrap();

    state
        .load_contract_at_address(
            class_hash_wrapper,
            wrapper_address.clone(),
            "starknet_programs/cairo2/get_number_wrapper.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    // CALL GET_NUMBER BEFORE REPLACE_CLASS

    let (result_vm, result_native) = state
        .execute(
            &wrapper_address,
            &wrapper_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_number".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // REPLACE_CLASS

    let (result_vm, result_native) = state
        .execute(
            &wrapper_address,
            &wrapper_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("upgrade".as_bytes())),
            ),
            &[Felt252::from_bytes_be(&class_hash_b.0)],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);

    // CALL GET_NUMBER AFTER REPLACE_CLASS

    let (result_vm, result_native) = state
        .execute(
            &wrapper_address,
            &wrapper_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_number".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn keccak_syscall_test() {
    let class_hash = ClassHash([1; 32]);
    let address = Address(1.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            address.clone(),
            "starknet_programs/cairo2/test_cairo_keccak.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &address,
            &address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("cairo_keccak_test".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn library_call() {
    let sr_class_hash = ClassHash([1; 32]);
    let sr_address = Address(1.into());
    let lib_class_hash = ClassHash([2; 32]);
    let lib_address = Address(2.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            sr_class_hash,
            sr_address.clone(),
            "starknet_programs/cairo2/square_root.cairo",
        )
        .unwrap();
    state
        .load_contract_at_address(
            lib_class_hash,
            lib_address.clone(),
            "starknet_programs/cairo2/math_lib.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &sr_address,
            &sr_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("square_root".as_bytes())),
            ),
            &[
                25.into(),
                Felt252::from_bytes_be_slice(lib_class_hash.to_bytes_be()),
            ],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn deploy_syscall_test() {
    let deployer_class_hash = ClassHash([1; 32]);
    let deployer_address = Address(1.into());

    let deployee_class_hash = ClassHash([2; 32]);
    let salt = Felt252::ONE;

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            deployer_class_hash,
            deployer_address.clone(),
            "starknet_programs/cairo2/deploy.cairo",
        )
        .unwrap();

    state
        .load_contract(deployee_class_hash, "starknet_programs/cairo2/echo.cairo")
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &deployer_address,
            &deployer_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("deploy_test".as_bytes())),
            ),
            &[Felt252::from_bytes_be(&deployee_class_hash.0), salt],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn deploy_syscall_address_unavailable_test() {
    let deployer_class_hash = ClassHash([1; 32]);
    let deployer_address = Address(1.into());

    let deployee_class_hash = ClassHash([2; 32]);
    let salt = Felt252::ONE;
    let deployee_constructor_calldata = vec![100.into()];
    let deployee_address = Address(
        calculate_contract_address(
            &salt,
            &Felt252::from_bytes_be(&deployee_class_hash.0),
            &deployee_constructor_calldata,
            deployer_address.clone(),
        )
        .unwrap(),
    );

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            deployer_class_hash,
            deployer_address.clone(),
            "starknet_programs/cairo2/deploy.cairo",
        )
        .unwrap();

    // Insert contract to be deployed so that its address is taken
    state
        .load_contract_at_address(
            deployee_class_hash,
            deployee_address.clone(),
            "starknet_programs/cairo2/echo.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &deployer_address,
            &deployer_address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("deploy_test".as_bytes())),
            ),
            &[Felt252::from_bytes_be(&deployee_class_hash.0), salt],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[test]
fn get_execution_info_test() {
    let class_hash = ClassHash([1; 32]);
    let address = Address(1.into());

    let mut state = TestStateSetup::default();
    state
        .load_contract_at_address(
            class_hash,
            address.clone(),
            "starknet_programs/cairo2/get_execution_info.cairo",
        )
        .unwrap();

    let mut state = state.finalize();

    let (result_vm, result_native) = state
        .execute(
            &address,
            &address,
            (
                EntryPointType::External,
                &Felt252::from_bytes_be(&calculate_sn_keccak("get_info".as_bytes())),
            ),
            &[],
        )
        .unwrap();

    assert_eq_sorted!(result_vm, result_native);
}

#[derive(Debug, Default)]
pub struct TestStateSetup {
    state_reader: InMemoryStateReader,

    cache_native: PermanentContractClassCache,
    cache_vm: PermanentContractClassCache,
}

impl TestStateSetup {
    pub fn load_contract(
        &mut self,
        class_hash: ClassHash,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let casm_contract_class_data = fs::read_to_string(path.as_ref().with_extension("casm"))?;
        let sierra_contract_class_data =
            fs::read_to_string(path.as_ref().with_extension("sierra"))?;

        let casm_contract_class: CasmContractClass =
            serde_json::from_str(&casm_contract_class_data)?;
        let sierra_contract_class: SierraContractClass =
            serde_json::from_str(&sierra_contract_class_data)?;

        let casm_contract_class = Arc::new(casm_contract_class);
        let sierra_contract_class = Arc::new((
            sierra_contract_class.extract_sierra_program().unwrap(),
            sierra_contract_class.entry_points_by_type,
        ));

        // Insert the CASM class in both native and vm contract caches, but Sierra only in the
        // native one. Otherwise, the VM would use Native instead.
        self.cache_vm.set_contract_class(
            class_hash,
            CompiledClass::Casm {
                casm: casm_contract_class.clone(),
                sierra: None,
            },
        );
        self.cache_native.set_contract_class(
            class_hash,
            CompiledClass::Casm {
                casm: casm_contract_class,
                sierra: Some(sierra_contract_class),
            },
        );

        Ok(())
    }

    pub fn load_contract_at_address(
        &mut self,
        class_hash: ClassHash,
        contract_address: Address,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.load_contract(class_hash, path)?;

        self.state_reader
            .address_to_class_hash_mut()
            .insert(contract_address.clone(), class_hash);
        self.state_reader
            .address_to_nonce_mut()
            .insert(contract_address, Felt252::default());

        Ok(())
    }

    pub fn finalize(self) -> TestState {
        let state_reader = Arc::new(self.state_reader);

        TestState {
            state_vm: CachedState::new(state_reader.clone(), Arc::new(self.cache_vm)),
            state_native: CachedState::new(state_reader, Arc::new(self.cache_native)),

            starknet_os_config: Default::default(),
        }
    }

    pub fn finalize_with_starknet_os_config(
        self,
        starknet_os_config: StarknetOsConfig,
    ) -> TestState {
        let state_reader = Arc::new(self.state_reader);

        TestState {
            state_vm: CachedState::new(state_reader.clone(), Arc::new(self.cache_vm)),
            state_native: CachedState::new(state_reader, Arc::new(self.cache_native)),

            starknet_os_config,
        }
    }
}

#[derive(Debug)]
pub struct TestState {
    state_native: CachedState<InMemoryStateReader, PermanentContractClassCache>,
    state_vm: CachedState<InMemoryStateReader, PermanentContractClassCache>,

    starknet_os_config: StarknetOsConfig,
}

impl TestState {
    pub fn storage_writes_at(&self, k: (Address, [u8; 32])) -> (Option<Felt252>, Option<Felt252>) {
        (
            self.state_vm.cache().storage_writes().get(&k).cloned(),
            self.state_native.cache().storage_writes().get(&k).cloned(),
        )
    }

    pub fn insert_initial_storage_value(&mut self, k: (Address, [u8; 32]), v: Felt252) {
        self.state_native
            .cache_mut()
            .storage_initial_values_mut()
            .insert(k.clone(), v);
        self.state_vm
            .cache_mut()
            .storage_initial_values_mut()
            .insert(k, v);
    }

    pub fn execute(
        &mut self,
        caller_address: &Address,
        callee_address: &Address,
        entry_point: (EntryPointType, &Felt252),
        call_data: &[Felt252],
    ) -> Result<(ExecutionResult, ExecutionResult), Box<dyn std::error::Error>> {
        let (class_hash_vm, contract_class_vm) =
            Self::get_contract_class_for_address(&self.state_vm, caller_address)
                .ok_or("The contract address doesn't exist.")?;
        let (class_hash_native, contract_class_native) =
            Self::get_contract_class_for_address(&self.state_native, caller_address)
                .ok_or("The contract address doesn't exist.")?;

        assert_matches!(
            contract_class_vm,
            CompiledClass::Casm { sierra: None, .. },
            "The VM contract class contains the Sierra."
        );
        assert_matches!(
            contract_class_native,
            CompiledClass::Casm {
                sierra: Some(_),
                ..
            },
            "The Native contract class doesn't contain the Sierra."
        );

        let mut block_context = BlockContext::default();
        block_context.block_info_mut().block_number = 30;

        let mut execution_result_vm = ExecutionEntryPoint::new(
            callee_address.clone(),
            call_data.to_vec(),
            *entry_point.1,
            caller_address.clone(),
            entry_point.0,
            Some(CallType::Delegate),
            Some(class_hash_vm),
            u128::MAX,
        )
        .execute(
            &mut self.state_vm,
            &block_context,
            &mut ExecutionResourcesManager::default(),
            &mut TransactionExecutionContext::new(
                Address(Felt252::default()),
                Felt252::default(),
                Vec::default(),
                Default::default(),
                10.into(),
                block_context.invoke_tx_max_n_steps(),
                *TRANSACTION_VERSION,
            ),
            false,
            block_context.invoke_tx_max_n_steps(),
            #[cfg(feature = "cairo-native")]
            None,
        )?;
        // Overwrite the execution result's execution_resources as native doesn't output it
        if let Some(callinfo) = execution_result_vm.call_info.as_mut() {
            callinfo.execution_resources = None;
            for callinfo in callinfo.internal_calls.iter_mut() {
                callinfo.execution_resources = None;
            }
        }

        let execution_result_native = ExecutionEntryPoint::new(
            callee_address.clone(),
            call_data.to_vec(),
            *entry_point.1,
            caller_address.clone(),
            entry_point.0,
            Some(CallType::Delegate),
            Some(class_hash_native),
            u128::MAX,
        )
        .execute(
            &mut self.state_native,
            &block_context,
            &mut ExecutionResourcesManager::default(),
            &mut TransactionExecutionContext::new(
                Address(Felt252::default()),
                Felt252::default(),
                Vec::default(),
                Default::default(),
                10.into(),
                block_context.invoke_tx_max_n_steps(),
                *TRANSACTION_VERSION,
            ),
            false,
            block_context.invoke_tx_max_n_steps(),
            #[cfg(feature = "cairo-native")]
            None,
        )?;

        Ok((execution_result_vm, execution_result_native))
    }

    pub fn execute_transaction(
        &mut self,
        tx: Transaction,
    ) -> Result<TransactionExecutionInfo, Box<dyn std::error::Error>> {
        let (_, contract_class_native) =
            Self::get_contract_class_for_address(&self.state_native, &tx.contract_address())
                .ok_or("The contract address doesn't exist.")?;

        assert_matches!(
            contract_class_native,
            CompiledClass::Casm {
                sierra: Some(_),
                ..
            },
            "The Native contract class doesn't contain the Sierra."
        );

        let mut block_context = BlockContext::default();
        block_context.block_info_mut().block_number = 30;
        let context = block_context.starknet_os_config_mut();
        *context = self.starknet_os_config.clone();

        let execution_result_native = tx
            .execute(
                &mut self.state_native,
                &block_context,
                u128::MAX,
                #[cfg(feature = "cairo-native")]
                None,
            )
            .unwrap();

        Ok(execution_result_native)
    }

    fn get_contract_class_for_address(
        state: &CachedState<InMemoryStateReader, PermanentContractClassCache>,
        address: &Address,
    ) -> Option<(ClassHash, CompiledClass)> {
        let class_hash = *state.state_reader.address_to_class_hash.get(address)?;
        let compiled_class = state
            .contract_class_cache()
            .get_contract_class(class_hash)?;

        Some((class_hash, compiled_class))
    }
}
