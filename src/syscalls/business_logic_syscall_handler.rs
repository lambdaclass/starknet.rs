use super::{
    syscall_handler_errors::SyscallHandlerError,
    syscall_info::get_syscall_size_from_name,
    syscall_request::{
        CallContractRequest, DeployRequest, EmitEventRequest, FromPtr, GetBlockHashRequest,
        GetBlockTimestampRequest, KeccakRequest, LibraryCallRequest, ReplaceClassRequest,
        SecpAddRequest, SendMessageToL1Request, StorageReadRequest, StorageWriteRequest,
        SyscallRequest,
    },
    syscall_response::{
        CallContractResponse, DeployResponse, FailureReason, GetBlockHashResponse,
        GetBlockTimestampResponse, KeccakResponse, ResponseBody, SyscallResponse,
    },
};
use crate::{
    core::errors::state_errors::StateError,
    definitions::{
        block_context::BlockContext,
        constants::{
            BLOCK_HASH_CONTRACT_ADDRESS, CONSTRUCTOR_ENTRY_POINT_SELECTOR, EVENT_MAX_DATA_LENGTH,
            EVENT_MAX_KEYS_LENGTH, MAX_N_EMITTED_EVENTS,
        },
    },
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        CallInfo, CallResult, CallType, OrderedEvent, OrderedL2ToL1Message,
        TransactionExecutionContext,
    },
    hash_utils::calculate_contract_address,
    services::api::{
        contract_class_errors::ContractClassError,
        contract_classes::{
            compiled_class::CompiledClass, deprecated_contract_class::EntryPointType,
        },
    },
    state::{
        cached_state::CachedState,
        contract_class_cache::ContractClassCache,
        contract_storage_state::ContractStorageState,
        state_api::{State, StateReader},
        BlockInfo, ExecutionResourcesManager,
    },
    transaction::{error::TransactionError, Address, ClassHash},
    utils::{felt_to_hash, get_big_int, get_felt_range},
};
use cairo_vm::Felt252;
use cairo_vm::{
    types::{
        errors::math_errors::MathError,
        relocatable::{MaybeRelocatable, Relocatable},
    },
    vm::{errors::memory_errors::MemoryError, vm_core::VirtualMachine},
};
use lazy_static::lazy_static;
use num_traits::{One, ToPrimitive, Zero};
use std::{
    collections::HashMap,
    ops::{Add, Sub},
};

#[cfg(feature = "cairo-native")]
use {
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

lazy_static! {
    static ref SYSCALLS: Vec<String> = Vec::from([
        "emit_event".to_string(),
        "deploy".to_string(),
        "get_tx_info".to_string(),
        "send_message_to_l1".to_string(),
        "library_call".to_string(),
        "get_caller_address".to_string(),
        "get_contract_address".to_string(),
        "get_sequencer_address".to_string(),
        "get_block_timestamp".to_string(),
    ]);

    /// Felt->syscall map that was extracted from new_syscalls.json (Cairo 1.0 syscalls)
    static ref SELECTOR_TO_SYSCALL: HashMap<Felt252, &'static str> = {
            let mut map: HashMap<Felt252, &'static str> = HashMap::with_capacity(9);

            map.insert(92376026794327011772951660_u128.into(), "library_call");
            map.insert(25500403217443378527601783667_u128.into(), "replace_class");
            map.insert(
                94901967946959054011942058057773508207_u128.into(),
                "get_execution_info",
            );
            map.insert(22096086224907272360718070632_u128.into(), "get_block_hash");
            map.insert(100890693370601760042082660_u128.into(), "storage_read");
            map.insert(20853273475220472486191784820_u128.into(), "call_contract");
            map.insert(
                433017908768303439907196859243777073_u128.into(),
                "send_message_to_l1",
            );
            map.insert(75202468540281_u128.into(), "deploy");
            map.insert(1280709301550335749748_u128.into(), "emit_event");
            map.insert(25828017502874050592466629733_u128.into(), "storage_write");
            map.insert(Felt252::from_bytes_be_slice("get_block_timestamp".as_bytes()), "get_block_timestamp");
            map.insert(Felt252::from_bytes_be_slice("get_block_number".as_bytes()), "get_block_number");
            map.insert(Felt252::from_bytes_be_slice("Keccak".as_bytes()), "keccak");

            // SECP256k1 syscalls
            let secp_syscalls = [
                ("Secp256k1New", "secp256k1_new"),
                ("Secp256k1Add", "secp256k1_add"),
                ("Secp256k1Mul", "secp256k1_mul"),
                ("Secp256k1GetPointFromX", "secp256k1_get_point_from_x"),
                ("Secp256k1GetXy", "secp256k1_get_xy"),
                ("Secp256r1New", "secp256r1_new"),
                ("Secp256r1Add", "secp256r1_add"),
                ("Secp256r1Mul", "secp256r1_mul"),
                ("Secp256r1GetPointFromX", "secp256r1_get_point_from_x"),
                ("Secp256r1GetXy", "secp256r1_get_xy")
            ];

            for (syscall, syscall_name) in secp_syscalls {
                map.insert(Felt252::from_bytes_be_slice(syscall.as_bytes()), syscall_name);
            }
            map
    };

    // TODO: There is no reason why this could not be in the syscall enum itself AFAICT
    // Taken from starkware/starknet/constants.py in cairo-lang
    // See further documentation on cairo_programs/constants.cairo
    /// Maps syscall name to gas costs
    pub(crate) static ref SYSCALL_GAS_COST: HashMap<&'static str, u128> = {
        let mut map = HashMap::new();

        map.insert("initial", 100_000_000 * STEP);
        map.insert("entry_point_initial_budget", 100 * STEP);

        map.insert("entry_point", map["entry_point_initial_budget"] + 500 * STEP);
        map.insert("fee_transfer", map["entry_point"] + 100 * STEP);
        map.insert("transaction", 2 * map["entry_point"] + map["fee_transfer"] + 100 * STEP);

        map.insert("call_contract", SYSCALL_BASE + 10 * STEP + map["entry_point"]);
        map.insert("deploy", SYSCALL_BASE + 200 * STEP + map["entry_point"]);
        map.insert("get_execution_info", SYSCALL_BASE + 10 * STEP);
        map.insert("library_call", map["call_contract"]);
        map.insert("replace_class", SYSCALL_BASE + 50 * STEP);
        map.insert("storage_read", SYSCALL_BASE + 50 * STEP);
        map.insert("storage_write", SYSCALL_BASE + 50 * STEP);
        map.insert("emit_event", SYSCALL_BASE + 10 * STEP);
        map.insert("send_message_to_l1", SYSCALL_BASE + 50 * STEP);
        map.insert("get_block_timestamp", 0);
        map.insert("keccak", 0);
        map.insert("get_block_hash", SYSCALL_BASE + 50 * STEP);

        // Secp256k1
        map.insert("secp256k1_add", 406 * STEP + 29 * RANGE_CHECK);
        map.insert("secp256k1_get_point_from_x", 391 * STEP + 30 * RANGE_CHECK + 20 * MEMORY_HOLE);
        map.insert("secp256k1_get_xy", 239 * STEP + 11 * RANGE_CHECK + 40 * MEMORY_HOLE);
        map.insert("secp256k1_mul", 76501 * STEP + 7045 * RANGE_CHECK + 2 * MEMORY_HOLE);
        map.insert("secp256k1_new", 475 * STEP + 35 * RANGE_CHECK + 40 * MEMORY_HOLE);

        // Secp256r1
        map.insert("secp256r1_add", 589 * STEP + 57 * RANGE_CHECK);
        map.insert("secp256r1_get_point_from_x", 510 * STEP + 44 * RANGE_CHECK + 20 * MEMORY_HOLE);
        map.insert("secp256r1_get_xy", 241 * STEP + 11 * RANGE_CHECK + 40 * MEMORY_HOLE);
        map.insert("secp256r1_mul", 125340 * STEP + 13961 * RANGE_CHECK + 2 * MEMORY_HOLE);
        map.insert("secp256r1_new", 594 * STEP + 49 * RANGE_CHECK + 40 * MEMORY_HOLE);

        map
    };
}

/// Structure representing the [BusinessLogicSyscallHandler].
#[derive(Debug)]
pub struct BusinessLogicSyscallHandler<'a, S: StateReader, C: ContractClassCache> {
    /// Events emitted by the current contract call.
    pub(crate) events: Vec<OrderedEvent>,
    /// Get the expected pointer to the syscall
    pub(crate) expected_syscall_ptr: Relocatable,
    /// Manages execution resources
    pub(crate) resources_manager: ExecutionResourcesManager,
    /// Context of the transaction being executed
    pub(crate) tx_execution_context: TransactionExecutionContext,
    /// Messages from L2 to L1
    pub(crate) l2_to_l1_messages: Vec<OrderedL2ToL1Message>,
    /// Address of the contract
    pub(crate) contract_address: Address,
    /// Address of the caller
    pub(crate) caller_address: Address,
    /// A list of dynamically allocated segments that are expected to be read-only.
    pub(crate) read_only_segments: Vec<(Relocatable, MaybeRelocatable)>,
    /// List of internal calls during the syscall execution
    pub(crate) internal_calls: Vec<CallInfo>,
    /// Context information related to the current block
    pub(crate) block_context: BlockContext,
    /// State of the storage related to Starknet contract
    pub(crate) starknet_storage_state: ContractStorageState<'a, S, C>,
    /// Indicates whether the current execution supports the "reverted" status.
    pub(crate) support_reverted: bool,
    /// Get the selector for the entry point of the contract.
    pub(crate) entry_point_selector: Felt252,
    /// Map selectors to their corresponding syscall names.
    pub(crate) selector_to_syscall: &'a HashMap<Felt252, &'static str>,
    pub(crate) execution_info_ptr: Option<Relocatable>,
}

// TODO: execution entry point may no be a parameter field, but there is no way to generate a default for now

impl<'a, S: StateReader, C: ContractClassCache> BusinessLogicSyscallHandler<'a, S, C> {
    /// Constructor creates a new [BusinessLogicSyscallHandler] instance
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tx_execution_context: TransactionExecutionContext,
        state: &'a mut CachedState<S, C>,
        resources_manager: ExecutionResourcesManager,
        caller_address: Address,
        contract_address: Address,
        block_context: BlockContext,
        syscall_ptr: Relocatable,
        support_reverted: bool,
        entry_point_selector: Felt252,
    ) -> Self {
        let events = Vec::new();
        let read_only_segments = Vec::new();
        let l2_to_l1_messages = Vec::new();
        let starknet_storage_state = ContractStorageState::new(state, contract_address.clone());
        let internal_calls = Vec::new();

        BusinessLogicSyscallHandler {
            tx_execution_context,
            events,
            read_only_segments,
            resources_manager,
            contract_address,
            caller_address,
            l2_to_l1_messages,
            block_context,
            starknet_storage_state,
            internal_calls,
            expected_syscall_ptr: syscall_ptr,
            support_reverted,
            entry_point_selector,
            selector_to_syscall: &SELECTOR_TO_SYSCALL,
            execution_info_ptr: None,
        }
    }

    /// Constructor with default values, used for testing
    pub fn default_with_state(state: &'a mut CachedState<S, C>) -> Self {
        BusinessLogicSyscallHandler::new_for_testing(
            BlockInfo::default(),
            Default::default(),
            state,
        )
    }

    ///  System calls allow a contract to requires services from the Starknet OS
    ///  See further documentation on https://docs.starknet.io/documentation/architecture_and_concepts/Contracts/system-calls/
    /// Constructor for testing purposes
    pub fn new_for_testing(
        block_info: BlockInfo,
        _contract_address: Address,
        state: &'a mut CachedState<S, C>,
    ) -> Self {
        let events = Vec::new();
        let tx_execution_context = Default::default();
        let read_only_segments = Vec::new();
        let resources_manager =
            ExecutionResourcesManager::new(SYSCALLS.clone(), Default::default());
        let contract_address = Address(1.into());
        let caller_address = Address(0.into());
        let l2_to_l1_messages = Vec::new();
        let mut block_context = BlockContext::default();
        block_context.block_info = block_info;
        let starknet_storage_state = ContractStorageState::new(state, contract_address.clone());

        let internal_calls = Vec::new();
        let expected_syscall_ptr = Relocatable::from((0, 0));
        let entry_point_selector = 333.into();

        BusinessLogicSyscallHandler {
            tx_execution_context,
            events,
            read_only_segments,
            resources_manager,
            contract_address,
            caller_address,
            l2_to_l1_messages,
            block_context,
            starknet_storage_state,
            internal_calls,
            expected_syscall_ptr,
            support_reverted: false,
            entry_point_selector,
            selector_to_syscall: &SELECTOR_TO_SYSCALL,
            execution_info_ptr: None,
        }
    }

    /// Increments the syscall count for a given `syscall_name` by 1.
    fn increment_syscall_count(&mut self, syscall_name: &str) {
        self.resources_manager
            .increment_syscall_counter(syscall_name, 1);
    }

    /// Helper function to execute a call to a contract
    fn call_contract_helper(
        &mut self,
        vm: &mut VirtualMachine,
        remaining_gas: u128,
        execution_entry_point: ExecutionEntryPoint,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let ExecutionResult {
            call_info,
            revert_error,
            ..
        } = execution_entry_point
            .execute(
                self.starknet_storage_state.state,
                &self.block_context,
                &mut self.resources_manager,
                &mut self.tx_execution_context,
                false,
                self.block_context.invoke_tx_max_n_steps,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )
            .map_err(|err| SyscallHandlerError::ExecutionError(err.to_string()))?;

        let call_info = call_info.ok_or(SyscallHandlerError::ExecutionError(
            revert_error.unwrap_or_else(|| "Execution error".to_string()),
        ))?;

        let retdata_maybe_reloc = call_info
            .retdata
            .clone()
            .into_iter()
            .map(MaybeRelocatable::from)
            .collect::<Vec<MaybeRelocatable>>();

        let retdata_start = self.allocate_segment(vm, retdata_maybe_reloc)?;
        let retdata_end = (retdata_start + call_info.retdata.len())?;

        let remaining_gas = remaining_gas.saturating_sub(call_info.gas_consumed);

        let gas = remaining_gas;
        let body = if call_info.failure_flag {
            Some(ResponseBody::Failure(FailureReason {
                retdata_start,
                retdata_end,
            }))
        } else {
            Some(ResponseBody::CallContract(CallContractResponse {
                retdata_start,
                retdata_end,
            }))
        };

        // update syscall handler information
        self.starknet_storage_state
            .read_values
            .extend(call_info.storage_read_values.clone());
        self.starknet_storage_state
            .accessed_keys
            .extend(call_info.accessed_storage_keys.clone());

        self.internal_calls.push(call_info);

        Ok(SyscallResponse { gas, body })
    }

    /// Checks if constructor entry points are empty
    fn constructor_entry_points_empty(
        &self,
        contract_class: CompiledClass,
    ) -> Result<bool, StateError> {
        Ok(match contract_class {
            CompiledClass::Deprecated(class) => class
                .entry_points_by_type
                .get(&EntryPointType::Constructor)
                .ok_or(ContractClassError::NoneEntryPointType)?
                .is_empty(),
            CompiledClass::Casm { casm: class, .. } => {
                class.entry_points_by_type.constructor.is_empty()
            }
        })
    }

    /// Execute a constructor entry point
    fn execute_constructor_entry_point(
        &mut self,
        contract_address: &Address,
        class_hash_bytes: ClassHash,
        constructor_calldata: Vec<Felt252>,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<CallResult, StateError> {
        let compiled_class = if let Ok(compiled_class) = self
            .starknet_storage_state
            .state
            .get_contract_class(&class_hash_bytes)
        {
            compiled_class
        } else {
            return Ok(CallResult {
                gas_consumed: 0,
                is_success: false,
                retdata: vec![Felt252::from_bytes_be_slice(b"CLASS_HASH_NOT_FOUND").into()],
            });
        };

        if self.constructor_entry_points_empty(compiled_class)? {
            if !constructor_calldata.is_empty() {
                return Err(StateError::ConstructorCalldataEmpty);
            }

            let call_info = CallInfo::empty_constructor_call(
                contract_address.clone(),
                self.contract_address.clone(),
                Some(class_hash_bytes),
            );
            self.internal_calls.push(call_info.clone());

            return Ok(call_info.result());
        }

        let call = ExecutionEntryPoint::new(
            contract_address.clone(),
            constructor_calldata,
            *CONSTRUCTOR_ENTRY_POINT_SELECTOR,
            self.contract_address.clone(),
            EntryPointType::Constructor,
            Some(CallType::Call),
            None,
            remaining_gas,
        );

        let ExecutionResult {
            call_info,
            revert_error,
            ..
        } = call
            .execute(
                self.starknet_storage_state.state,
                &self.block_context,
                &mut self.resources_manager,
                &mut self.tx_execution_context,
                self.support_reverted,
                self.block_context.invoke_tx_max_n_steps,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )
            .map_err(|_| StateError::ExecutionEntryPoint)?;

        let call_info = call_info.ok_or(StateError::CustomError(
            revert_error.unwrap_or_else(|| "Execution error".to_string()),
        ))?;

        self.internal_calls.push(call_info.clone());

        Ok(call_info.result())
    }

    /// Writes a value to the storage state using the specified address.
    fn syscall_storage_write(&mut self, key: Felt252, value: Felt252) {
        self.starknet_storage_state.write(Address(key), value)
    }

    /// Reads the syscall request, checks and reduces gas, executes the syscall, and writes the syscall response.
    pub fn syscall(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<(), SyscallHandlerError> {
        let selector = get_big_int(vm, syscall_ptr)?;
        let syscall_name = self.selector_to_syscall.get(&selector).ok_or(
            SyscallHandlerError::SelectorNotInHandlerMap(selector.to_string()),
        )?;

        let initial_gas: Felt252 = get_big_int(vm, (syscall_ptr + 1)?)?;
        let initial_gas = initial_gas
            .to_u128()
            .ok_or(MathError::Felt252ToU64Conversion(Box::new(initial_gas)))?;

        // Advance SyscallPointer as the first two cells contain the selector & gas
        let mut syscall_ptr: Relocatable =
            (syscall_ptr + 2_usize).map_err(SyscallHandlerError::from)?;

        let request = self.read_and_validate_syscall_request(vm, &mut syscall_ptr, syscall_name)?;

        // Check and reduce gas (after validating the syscall selector for consistency wth the OS).
        let required_gas = SYSCALL_GAS_COST
            .get(syscall_name)
            .map(|&x| x.saturating_sub(SYSCALL_BASE))
            .ok_or(SyscallHandlerError::SelectorDoesNotHaveAssociatedGas(
                selector.to_string(),
            ))?;

        let response = if initial_gas < required_gas {
            let out_of_gas_felt = Felt252::from_bytes_be_slice("Out of gas".as_bytes());
            let retdata_start =
                self.allocate_segment(vm, vec![MaybeRelocatable::from(out_of_gas_felt)])?;
            let response_body = ResponseBody::Failure(FailureReason {
                retdata_start,
                retdata_end: (retdata_start + 1)?,
            });

            SyscallResponse {
                gas: initial_gas,
                body: Some(response_body),
            }
        } else {
            // Execute with remaining gas.
            let remaining_gas = initial_gas - required_gas;
            self.execute_syscall(
                request,
                remaining_gas,
                vm,
                #[cfg(feature = "cairo-native")]
                program_cache,
            )?
        };

        // Write response to the syscall segment.
        self.expected_syscall_ptr = vm
            .write_arg(syscall_ptr, &response.to_cairo_compatible_args())?
            .get_relocatable()
            .ok_or(MemoryError::WriteArg)?;

        Ok(())
    }

    /// Executes the specific syscall based on the request.
    fn execute_syscall(
        &mut self,
        request: SyscallRequest,
        remaining_gas: u128,
        vm: &mut VirtualMachine,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        match request {
            SyscallRequest::LibraryCall(req) => self.library_call(
                vm,
                req,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            SyscallRequest::CallContract(req) => self.call_contract(
                vm,
                req,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            SyscallRequest::Deploy(req) => self.deploy(
                vm,
                req,
                remaining_gas,
                #[cfg(feature = "cairo-native")]
                program_cache,
            ),
            SyscallRequest::StorageRead(req) => self.storage_read(vm, req, remaining_gas),
            SyscallRequest::StorageWrite(req) => self.storage_write(vm, req, remaining_gas),
            SyscallRequest::GetExecutionInfo => self.get_execution_info(vm, remaining_gas),
            SyscallRequest::SendMessageToL1(req) => self.send_message_to_l1(vm, req, remaining_gas),
            SyscallRequest::EmitEvent(req) => self.emit_event(vm, req, remaining_gas),
            SyscallRequest::GetBlockNumber => self.get_block_number(vm, remaining_gas),
            SyscallRequest::GetBlockTimestamp(req) => {
                self.get_block_timestamp(vm, req, remaining_gas)
            }
            SyscallRequest::GetBlockHash(req) => self.get_block_hash(vm, req, remaining_gas),
            SyscallRequest::ReplaceClass(req) => self.replace_class(vm, req, remaining_gas),
            SyscallRequest::Keccak(req) => self.keccak(vm, req, remaining_gas),
        }
    }

    /// Returns the hash of a specific block, with an error if the block number is out of range.
    fn get_block_hash(
        &mut self,
        vm: &mut VirtualMachine,
        request: GetBlockHashRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let block_number = request.block_number;
        let current_block_number = self.block_context.block_info.block_number;

        if current_block_number < 10 || block_number > current_block_number - 10 {
            let out_of_range_felt =
                Felt252::from_bytes_be_slice("Block number out of range".as_bytes());
            let retdata_start =
                self.allocate_segment(vm, vec![MaybeRelocatable::from(out_of_range_felt)])?;
            let failure = FailureReason {
                retdata_start,
                retdata_end: (retdata_start + 1)?,
            };

            return Ok(SyscallResponse {
                gas: remaining_gas,
                body: Some(ResponseBody::Failure(failure)),
            });
        }

        // FIXME: Update this after release.
        const V_0_12_0_FIRST_BLOCK: u64 = 0;
        #[allow(clippy::absurd_extreme_comparisons)]
        let block_hash = if block_number < V_0_12_0_FIRST_BLOCK {
            Felt252::ZERO
        } else {
            self.starknet_storage_state.state.get_storage_at(&(
                BLOCK_HASH_CONTRACT_ADDRESS.clone(),
                Felt252::from(block_number).to_bytes_be(),
            ))?
        };

        Ok(SyscallResponse {
            gas: remaining_gas,
            body: Some(ResponseBody::GetBlockHash(GetBlockHashResponse {
                block_hash,
            })),
        })
    }

    /// Validates stop pointers and read-only segments after the syscall execution.
    pub(crate) fn post_run(
        &self,
        runner: &mut VirtualMachine,
        syscall_stop_ptr: Relocatable,
    ) -> Result<(), TransactionError> {
        let expected_stop_ptr = self.expected_syscall_ptr;
        if syscall_stop_ptr != expected_stop_ptr {
            return Err(TransactionError::InvalidStopPointer(
                expected_stop_ptr,
                syscall_stop_ptr,
            ));
        }
        self.validate_read_only_segments(runner)
    }

    /// Validates that there were no out of bounds writes to read-only segments and marks them as accessed.
    pub(crate) fn validate_read_only_segments(
        &self,
        vm: &mut VirtualMachine,
    ) -> Result<(), TransactionError> {
        for (segment_ptr, segment_size) in self.read_only_segments.clone() {
            let used_size = vm
                .get_segment_used_size(segment_ptr.segment_index as usize)
                .ok_or(TransactionError::InvalidSegmentSize)?;

            let seg_size = match segment_size {
                MaybeRelocatable::Int(size) => size,
                _ => return Err(TransactionError::NotAFelt),
            };

            if seg_size != used_size.into() {
                return Err(TransactionError::OutOfBound);
            }
            vm.mark_address_range_as_accessed(segment_ptr, used_size)?;
        }
        Ok(())
    }
}

impl<'a, S: StateReader, C: ContractClassCache> BusinessLogicSyscallHandler<'a, S, C> {
    /// Emit an event.
    fn emit_event(
        &mut self,
        vm: &VirtualMachine,
        request: EmitEventRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let order = self.tx_execution_context.n_emitted_events;
        let keys: Vec<Felt252> = get_felt_range(vm, request.keys_start, request.keys_end)?;
        let data: Vec<Felt252> = get_felt_range(vm, request.data_start, request.data_end)?;
        // Check event limits
        if order >= MAX_N_EMITTED_EVENTS {
            return Err(SyscallHandlerError::MaxNumberOfEmittedEventsExceeded(
                MAX_N_EMITTED_EVENTS,
            ));
        }
        if keys.len() > EVENT_MAX_KEYS_LENGTH {
            return Err(SyscallHandlerError::EventMaxKeysLengthExceeded(
                keys.len(),
                EVENT_MAX_KEYS_LENGTH,
            ));
        }
        if data.len() > EVENT_MAX_DATA_LENGTH {
            return Err(SyscallHandlerError::EventMaxKeysLengthExceeded(
                data.len(),
                EVENT_MAX_DATA_LENGTH,
            ));
        }
        self.events.push(OrderedEvent::new(order, keys, data));

        // Update events count.
        self.tx_execution_context.n_emitted_events += 1;
        Ok(SyscallResponse {
            gas: remaining_gas,
            body: None,
        })
    }

    /// Returns the block number.
    fn get_block_number(
        &mut self,
        _vm: &mut VirtualMachine,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        Ok(SyscallResponse {
            gas: remaining_gas,
            body: Some(ResponseBody::GetBlockNumber {
                number: self.block_context.block_info.block_number.into(),
            }),
        })
    }

    /// Reads the value associated with the given key from the storage state.
    fn _storage_read(&mut self, key: [u8; 32]) -> Result<Felt252, StateError> {
        match self
            .starknet_storage_state
            .read(Address(Felt252::from_bytes_be(&key)))
        {
            Ok(value) => Ok(value),
            Err(e @ StateError::Io(_)) => Err(e),
            Err(_) => Ok(Felt252::ZERO),
        }
    }

    /// Performs a storage write operation.
    fn storage_write(
        &mut self,
        vm: &mut VirtualMachine,
        request: StorageWriteRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        if request.reserved != 0.into() {
            let retdata_start = self.allocate_segment(
                vm,
                vec![Felt252::from_bytes_be_slice(b"Unsupported address domain").into()],
            )?;
            let retdata_end = retdata_start.add(1)?;

            return Ok(SyscallResponse {
                gas: remaining_gas,
                body: Some(ResponseBody::Failure(FailureReason {
                    retdata_start,
                    retdata_end,
                })),
            });
        }

        self.syscall_storage_write(request.key, request.value);

        Ok(SyscallResponse {
            gas: remaining_gas,
            body: None,
        })
    }

    // Returns the pointer to the segment with the execution info if it was already written.
    // If it wasn't, it writes the execution info into memory and returns its start address.
    fn get_or_allocate_execution_info(
        &mut self,
        vm: &mut VirtualMachine,
    ) -> Result<Relocatable, SyscallHandlerError> {
        if let Some(ptr) = self.execution_info_ptr {
            return Ok(ptr);
        }

        // Allocate block_info
        let block_info = &self.block_context.block_info;
        let block_info_data = vec![
            MaybeRelocatable::from(Felt252::from(block_info.block_number)),
            MaybeRelocatable::from(Felt252::from(block_info.block_timestamp)),
            MaybeRelocatable::from(&block_info.sequencer_address.0),
        ];
        let block_info_ptr = self.allocate_segment(vm, block_info_data)?;

        // Allocate signature
        let signature: Vec<MaybeRelocatable> = self
            .tx_execution_context
            .signature
            .iter()
            .map(MaybeRelocatable::from)
            .collect();
        let signature_start_ptr = self.allocate_segment(vm, signature)?;
        let signature_end_ptr = (signature_start_ptr + self.tx_execution_context.signature.len())?;

        // Allocate tx info
        let tx_info = &self.tx_execution_context;
        let mut tx_info_data = vec![
            MaybeRelocatable::from(&tx_info.version),
            MaybeRelocatable::from(&tx_info.account_contract_address.0),
            MaybeRelocatable::from(Felt252::from(
                tx_info.account_tx_fields.max_fee_for_execution_info(),
            )),
            signature_start_ptr.into(),
            signature_end_ptr.into(),
            MaybeRelocatable::from(&tx_info.transaction_hash),
            MaybeRelocatable::from(&self.block_context.starknet_os_config.chain_id),
            MaybeRelocatable::from(&tx_info.nonce),
        ];
        self.allocate_version_specific_tx_info(vm, &mut tx_info_data)?;

        let tx_info_ptr = self.allocate_segment(vm, tx_info_data)?;

        // Allocate execution_info
        let execution_info = vec![
            block_info_ptr.into(),
            tx_info_ptr.into(),
            MaybeRelocatable::from(&self.caller_address.0),
            MaybeRelocatable::from(&self.contract_address.0),
            MaybeRelocatable::from(&self.entry_point_selector),
        ];
        let execution_info_ptr = self.allocate_segment(vm, execution_info)?;

        self.execution_info_ptr = Some(execution_info_ptr);
        Ok(execution_info_ptr)
    }

    fn allocate_version_specific_tx_info(
        &mut self,
        vm: &mut VirtualMachine,
        tx_info_data: &mut Vec<MaybeRelocatable>,
    ) -> Result<(), SyscallHandlerError> {
        match self.tx_execution_context.account_tx_fields.clone() {
            crate::transaction::VersionSpecificAccountTxFields::Deprecated(_) => {
                tx_info_data.extend_from_slice(&[
                    Felt252::ZERO.into(), // Resource Bounds (start ptr).
                    Felt252::ZERO.into(), // Resource Bounds (end ptr).
                    Felt252::ZERO.into(), // Tip.
                    Felt252::ZERO.into(), // Paymaster Data (start ptr).
                    Felt252::ZERO.into(), // Paymaster Data (end ptr).
                    Felt252::ZERO.into(), // Nonce DA mode.
                    Felt252::ZERO.into(), // Fee DA mode.
                    Felt252::ZERO.into(), // Account deployment Data (start ptr).
                    Felt252::ZERO.into(), // Account deployment Data (end ptr).
                ])
            }
            crate::transaction::VersionSpecificAccountTxFields::Current(fields) => {
                // Allocate resource bounds
                lazy_static! {
                    static ref L1_GAS: Felt252 = Felt252::from_hex(
                        "0x00000000000000000000000000000000000000000000000000004c315f474153"
                    )
                    .unwrap();
                    static ref L2_GAS: Felt252 = Felt252::from_hex(
                        "0x00000000000000000000000000000000000000000000000000004c325f474153"
                    )
                    .unwrap();
                };
                let mut resource_bounds_data = vec![
                    *L1_GAS,
                    fields.l1_resource_bounds.max_amount.into(),
                    fields.l1_resource_bounds.max_price_per_unit.into(),
                ];
                if let Some(ref resource_bounds) = fields.l2_resource_bounds {
                    resource_bounds_data.extend_from_slice(&[
                        *L2_GAS,
                        resource_bounds.max_amount.into(),
                        resource_bounds.max_price_per_unit.into(),
                    ])
                }
                let (resource_bounds_start_ptr, resource_bounds_end_ptr) =
                    self.allocate_felt_segment(vm, &resource_bounds_data)?;
                // Allocate paymaster data
                let (paymaster_data_start_ptr, paymaster_data_end_ptr) =
                    self.allocate_felt_segment(vm, &fields.paymaster_data)?;
                // Allocate account deployment data
                let (account_deployment_start_ptr, account_deployment_end_ptr) =
                    self.allocate_felt_segment(vm, &fields.account_deployment_data)?;
                // Extend tx_info_data with version specific data
                tx_info_data.extend_from_slice(&[
                    resource_bounds_start_ptr.into(), // Resource Bounds (start ptr).
                    resource_bounds_end_ptr.into(),   // Resource Bounds (end ptr).
                    Felt252::from(fields.tip).into(), // Tip.
                    paymaster_data_start_ptr.into(),  // Paymaster Data (start ptr).
                    paymaster_data_end_ptr.into(),    // Paymaster Data (end ptr).
                    Into::<Felt252>::into(fields.nonce_data_availability_mode).into(), // Nonce DA mode.
                    Into::<Felt252>::into(fields.fee_data_availability_mode).into(), // Fee DA mode.
                    account_deployment_start_ptr.into(), // Account deployment Data (start ptr).
                    account_deployment_end_ptr.into(),   // Account deployment Data (end ptr).
                ])
            }
        }
        Ok(())
    }

    fn get_execution_info(
        &mut self,
        vm: &mut VirtualMachine,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let exec_info_ptr = self.get_or_allocate_execution_info(vm)?;
        Ok(SyscallResponse {
            gas: remaining_gas,
            body: Some(ResponseBody::GetExecutionInfo { exec_info_ptr }),
        })
    }

    /// Executes a contract call
    fn call_contract(
        &mut self,
        vm: &mut VirtualMachine,
        request: CallContractRequest,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let calldata = get_felt_range(vm, request.calldata_start, request.calldata_end)?;
        let execution_entry_point = ExecutionEntryPoint::new(
            request.contract_address,
            calldata,
            request.selector,
            self.contract_address.clone(),
            EntryPointType::External,
            Some(CallType::Call),
            None,
            remaining_gas,
        );

        self.call_contract_helper(
            vm,
            remaining_gas,
            execution_entry_point,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )
    }

    /// Performs a storage read operation.
    fn storage_read(
        &mut self,
        vm: &mut VirtualMachine,
        request: StorageReadRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        if request.reserved != Felt252::ZERO {
            let retdata_start = self.allocate_segment(
                vm,
                vec![Felt252::from_bytes_be_slice(b"Unsupported address domain").into()],
            )?;
            let retdata_end = retdata_start.add(1)?;

            return Ok(SyscallResponse {
                gas: remaining_gas,
                body: Some(ResponseBody::Failure(FailureReason {
                    retdata_start,
                    retdata_end,
                })),
            });
        }

        let value = self._storage_read(request.key)?;

        Ok(SyscallResponse {
            gas: remaining_gas,
            body: Some(ResponseBody::StorageReadResponse { value: Some(value) }),
        })
    }

    /// Deploys a contract.
    fn syscall_deploy(
        &mut self,
        vm: &VirtualMachine,
        request: DeployRequest,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<(Address, CallResult), SyscallHandlerError> {
        if !(request.deploy_from_zero.is_zero() || request.deploy_from_zero.is_one()) {
            return Err(SyscallHandlerError::DeployFromZero(
                request.deploy_from_zero,
            ));
        };

        let constructor_calldata =
            get_felt_range(vm, request.calldata_start, request.calldata_end)?;

        let class_hash = &request.class_hash;

        let deployer_address = if request.deploy_from_zero.is_zero() {
            self.contract_address.clone()
        } else {
            Address::default()
        };

        let contract_address = Address(calculate_contract_address(
            &request.salt,
            class_hash,
            &constructor_calldata,
            deployer_address,
        )?);

        // Initialize the contract.
        let class_hash_bytes: ClassHash = felt_to_hash(&request.class_hash);

        if (self
            .starknet_storage_state
            .state
            .deploy_contract(contract_address.clone(), class_hash_bytes))
        .is_err()
        {
            return Ok((
                Address::default(),
                (CallResult {
                    gas_consumed: 0,
                    is_success: false,
                    retdata: vec![
                        Felt252::from_bytes_be_slice(b"CONTRACT_ADDRESS_UNAVAILABLE").into(),
                    ],
                }),
            ));
        }
        let result = self.execute_constructor_entry_point(
            &contract_address,
            class_hash_bytes,
            constructor_calldata,
            remaining_gas,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )?;

        Ok((contract_address, result))
    }

    /// Deploys a contract to the virtual machine.
    fn deploy(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_request: DeployRequest,
        mut remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let (contract_address, result) = self.syscall_deploy(
            vm,
            syscall_request,
            remaining_gas,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )?;

        remaining_gas -= result.gas_consumed;

        let retdata_len = result.retdata.len();

        let retdata_start = self.allocate_segment(vm, result.retdata)?;
        let retdata_end = retdata_start.add(retdata_len)?;

        let ok = result.is_success;

        let body: ResponseBody = if ok {
            let contract_address = contract_address.0;
            ResponseBody::Deploy(DeployResponse {
                contract_address,
                retdata_start,
                retdata_end,
            })
        } else {
            ResponseBody::Failure(FailureReason {
                retdata_start,
                retdata_end,
            })
        };
        let response = SyscallResponse {
            gas: remaining_gas,
            body: Some(body),
        };

        Ok(response)
    }

    /// Reads and validates syscall requests. Matches syscall names to their corresponding requests.
    fn read_syscall_request(
        &self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
        syscall_name: &str,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        match syscall_name {
            "emit_event" => EmitEventRequest::from_ptr(vm, syscall_ptr),
            "storage_read" => StorageReadRequest::from_ptr(vm, syscall_ptr),
            "call_contract" => CallContractRequest::from_ptr(vm, syscall_ptr),
            "library_call" => LibraryCallRequest::from_ptr(vm, syscall_ptr),
            "deploy" => DeployRequest::from_ptr(vm, syscall_ptr),
            "get_block_number" => Ok(SyscallRequest::GetBlockNumber),
            "get_block_hash" => GetBlockHashRequest::from_ptr(vm, syscall_ptr),
            "storage_write" => StorageWriteRequest::from_ptr(vm, syscall_ptr),
            "get_execution_info" => Ok(SyscallRequest::GetExecutionInfo),
            "send_message_to_l1" => SendMessageToL1Request::from_ptr(vm, syscall_ptr),
            "replace_class" => ReplaceClassRequest::from_ptr(vm, syscall_ptr),
            "keccak" => KeccakRequest::from_ptr(vm, syscall_ptr),
            "secp256k1_add" => SecpAddRequest::from_ptr(vm, syscall_ptr),
            "secp256r1_add" => SecpAddRequest::from_ptr(vm, syscall_ptr),
            // "secp256k1_get_point_from_x" => Secp256,
            // "secp256k1_get_xy".to_string(),
            // "secp256k1_get_xy".to_string(),
            // "secp256k1_mul".to_string(),
            // "secp256k1_new".to_string(),
            // "secp256r1_get_point_from_x".to_string(),
            // "secp256r1_get_xy".to_string(),
            // "secp256r1_mul".to_string(),
            // "secp256r1_new".to_string(),
            _ => Err(SyscallHandlerError::UnknownSyscall(
                syscall_name.to_string(),
            )),
        }
    }
    /// Allocate a segment in memory.
    pub(crate) fn allocate_segment(
        &mut self,
        vm: &mut VirtualMachine,
        data: Vec<MaybeRelocatable>,
    ) -> Result<Relocatable, SyscallHandlerError> {
        let segment_start = vm.add_memory_segment();
        let segment_end = vm.load_data(segment_start, &data)?;
        let sub = segment_end.sub(segment_start)?;
        let segment = (segment_start.to_owned(), sub.into());
        self.read_only_segments.push(segment);

        Ok(segment_start)
    }

    /// Allocate a segment in memory.
    /// Returns start and end ptrs for the segment
    pub(crate) fn allocate_felt_segment(
        &mut self,
        vm: &mut VirtualMachine,
        data: &[Felt252],
    ) -> Result<(Relocatable, Relocatable), SyscallHandlerError> {
        let segment_start = vm.add_memory_segment();
        let segment_end = vm.load_data(segment_start, &data.iter().map(|f| f.into()).collect())?;
        let sub = segment_end.sub(segment_start)?;
        let segment = (segment_start.to_owned(), sub.into());
        self.read_only_segments.push(segment);

        Ok((segment_start, segment_end))
    }

    /// Sends a message from L2 to L1, including the destination address and payload.
    fn send_message_to_l1(
        &mut self,
        vm: &VirtualMachine,
        request: SendMessageToL1Request,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let payload = get_felt_range(vm, request.payload_start, request.payload_end)?;

        self.l2_to_l1_messages.push(OrderedL2ToL1Message::new(
            self.tx_execution_context.n_sent_messages,
            request.to_address,
            payload,
        ));

        // Update messages count.
        self.tx_execution_context.n_sent_messages += 1;
        Ok(SyscallResponse {
            gas: remaining_gas,
            body: None,
        })
    }

    /// Reads and validates a syscall request, and updates the expected syscall pointer offset.
    fn read_and_validate_syscall_request(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: &mut Relocatable,
        syscall_name: &str,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        self.increment_syscall_count(syscall_name);
        let syscall_request = self.read_syscall_request(vm, *syscall_ptr, syscall_name)?;

        *syscall_ptr += get_syscall_size_from_name(syscall_name);
        Ok(syscall_request)
    }

    /// Executes a library call
    fn library_call(
        &mut self,
        vm: &mut VirtualMachine,
        request: LibraryCallRequest,
        remaining_gas: u128,
        #[cfg(feature = "cairo-native")] program_cache: Option<
            Rc<RefCell<ProgramCache<'_, ClassHash>>>,
        >,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let calldata = get_felt_range(vm, request.calldata_start, request.calldata_end)?;
        let class_hash = ClassHash::from(request.class_hash);
        let execution_entry_point = ExecutionEntryPoint::new(
            self.contract_address.clone(),
            calldata,
            request.selector,
            self.caller_address.clone(),
            EntryPointType::External,
            Some(CallType::Delegate),
            Some(class_hash),
            remaining_gas,
        );

        self.call_contract_helper(
            vm,
            remaining_gas,
            execution_entry_point,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )
    }

    /// Get the time stamp of the block.
    fn get_block_timestamp(
        &mut self,
        _vm: &VirtualMachine,
        _request: GetBlockTimestampRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        Ok(SyscallResponse {
            gas: remaining_gas,
            body: Some(ResponseBody::GetBlockTimestamp(GetBlockTimestampResponse {
                timestamp: self.block_context.block_info.block_timestamp.into(),
            })),
        })
    }

    /// Replaces class at the specified address with a new one based on the request.
    fn replace_class(
        &mut self,
        _vm: &VirtualMachine,
        request: ReplaceClassRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        self.starknet_storage_state.state.set_class_hash_at(
            self.contract_address.clone(),
            ClassHash::from(request.class_hash),
        )?;
        Ok(SyscallResponse {
            gas: remaining_gas,
            body: None,
        })
    }

    /// Calculates the Keccak hash of a given input.
    fn keccak(
        &mut self,
        vm: &mut VirtualMachine,
        request: KeccakRequest,
        remaining_gas: u128,
    ) -> Result<SyscallResponse, SyscallHandlerError> {
        let length = (request.input_end - request.input_start)?;
        let mut gas = remaining_gas;

        if length % 17 != 0 {
            let response = self.failure_from_error_msg(vm, b"Invalid keccak input size")?;
            return Ok(SyscallResponse {
                gas,
                body: Some(response),
            });
        }
        let n_chunks = length / 17;
        let mut state = [0u64; 25];
        for i in 0..n_chunks {
            // TODO: check this before the loop, taking care to preserve functionality.
            if gas < KECCAK_ROUND_COST {
                let response = self.failure_from_error_msg(vm, b"Syscall out of gas")?;
                return Ok(SyscallResponse {
                    gas,
                    body: Some(response),
                });
            }
            gas -= KECCAK_ROUND_COST;
            let chunk_start = (request.input_start + i * 17)?;
            let chunk = get_felt_range(vm, chunk_start, (chunk_start + 17)?)?;
            for (i, val) in chunk.iter().enumerate() {
                state[i] ^= val.to_u64().ok_or_else(|| {
                    SyscallHandlerError::Conversion("Felt252".to_string(), "u64".to_string())
                })?;
            }
            keccak::f1600(&mut state)
        }
        let shift = Felt252::TWO.pow(64u32);
        let hash_low = (Felt252::from(state[1]) * shift) + Felt252::from(state[0]);
        let hash_high = (Felt252::from(state[3]) * shift) + Felt252::from(state[2]);

        Ok(SyscallResponse {
            gas,
            body: Some(ResponseBody::Keccak(KeccakResponse {
                hash_low,
                hash_high,
            })),
        })
    }

    // TODO: refactor code to use this function
    /// Constructs a failure response from an error message.
    fn failure_from_error_msg(
        &mut self,
        vm: &mut VirtualMachine,
        error_msg: &[u8],
    ) -> Result<ResponseBody, SyscallHandlerError> {
        let felt_encoded_msg = Felt252::from_bytes_be_slice(error_msg);
        let retdata_start =
            self.allocate_segment(vm, vec![MaybeRelocatable::from(felt_encoded_msg)])?;
        Ok(ResponseBody::Failure(FailureReason {
            retdata_start,
            retdata_end: (retdata_start + 1)?,
        }))
    }
}
