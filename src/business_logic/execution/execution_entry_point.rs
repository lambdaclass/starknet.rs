use super::objects::{CallInfo, CallType, TransactionExecutionContext};
use crate::{
    business_logic::{
        fact_state::{
            in_memory_state_reader::InMemoryStateReader, state::ExecutionResourcesManager,
        },
        state::state_api::State,
        state::{cached_state::CachedState, state_api::StateReader, state_cache::StorageEntry},
        transaction::error::TransactionError,
    },
    core::syscalls::{
        business_logic_syscall_handler::BusinessLogicSyscallHandler,
        syscall_handler::{SyscallHandler, SyscallHintProcessor},
    },
    definitions::{
        constants::{DEFAULT_ENTRY_POINT_SELECTOR, TRANSACTION_VERSION},
        general_config::StarknetGeneralConfig,
    },
    services::api::contract_class::{self, ContractClass, ContractEntryPoint, EntryPointType},
    starknet_runner::runner::StarknetRunner,
    utils::{
        calculate_sn_keccak, get_deployed_address_class_hash_at_address,
        validate_contract_deployed, Address,
    },
};
use cairo_rs::{
    types::{
        program::Program,
        relocatable::{MaybeRelocatable, Relocatable},
    },
    vm::{
        runners::cairo_runner::{CairoArg, CairoRunner, ExecutionResources},
        vm_core::VirtualMachine,
    },
    with_std::collections::HashMap,
};
use felt::Felt252;
use starknet_api::state;

/// Represents a Cairo entry point execution of a StarkNet contract.
#[derive(Debug)]
pub struct ExecutionEntryPoint {
    call_type: CallType,
    contract_address: Address,
    code_address: Option<Address>,
    class_hash: Option<[u8; 32]>,
    calldata: Vec<Felt252>,
    caller_address: Address,
    entry_point_selector: Felt252,
    entry_point_type: EntryPointType,
}

impl ExecutionEntryPoint {
    pub fn new(
        contract_address: Address,
        calldata: Vec<Felt252>,
        entry_point_selector: Felt252,
        caller_address: Address,
        entry_point_type: EntryPointType,
        call_type: Option<CallType>,
        class_hash: Option<[u8; 32]>,
    ) -> Self {
        ExecutionEntryPoint {
            call_type: call_type.unwrap_or(CallType::Call),
            contract_address,
            code_address: None,
            class_hash,
            calldata,
            caller_address,
            entry_point_selector,
            entry_point_type,
        }
    }

    /// Executes the selected entry point with the given calldata in the specified contract.
    /// The information collected from this run (number of steps required, modifications to the
    /// contract storage, etc.) is saved on the resources manager.
    /// Returns a CallInfo object that represents the execution.
    pub fn execute<T>(
        &self,
        state: &mut T,
        general_config: &StarknetGeneralConfig,
        resources_manager: &mut ExecutionResourcesManager,
        tx_execution_context: &TransactionExecutionContext,
    ) -> Result<CallInfo, TransactionError>
    where
        T: Default + State + StateReader,
    {
        let previous_cairo_usage = resources_manager.cairo_usage.clone();

        let runner = self.run(
            state,
            resources_manager,
            general_config,
            tx_execution_context,
        )?;

        // Update resources usage (for bouncer).
        resources_manager.cairo_usage =
            &resources_manager.cairo_usage + &runner.get_execution_resources()?;

        let retdata = runner.get_return_values()?;
        self.build_call_info::<T>(
            previous_cairo_usage,
            runner.hint_processor.syscall_handler,
            retdata,
        )
    }

    /// Runs the selected entry point with the given calldata in the code of the contract deployed
    /// at self.code_address.
    /// The execution is done in the context (e.g., storage) of the contract at
    /// self.contract_address.
    /// Returns the corresponding CairoFunctionRunner and BusinessLogicSysCallHandler in order to
    /// retrieve the execution information.
    fn run<'a, T>(
        &self,
        state: &'a mut T,
        resources_manager: &ExecutionResourcesManager,
        general_config: &StarknetGeneralConfig,
        tx_execution_context: &TransactionExecutionContext,
    ) -> Result<StarknetRunner<BusinessLogicSyscallHandler<'a, T>>, TransactionError>
    where
        T: Default + State + StateReader,
    {
        // Prepare input for Starknet runner.
        let class_hash = self.get_code_class_hash(state)?;
        let contract_class = state
            .get_contract_class(&class_hash)
            .map_err(|_| TransactionError::MissigContractClass)?;

        // fetch selected entry point
        let entry_point = self.get_selected_entry_point(&contract_class, class_hash)?;
        // create starknet runner

        let mut vm = VirtualMachine::new(false);
        let mut cairo_runner = CairoRunner::new(&contract_class.program, "all", false)?;
        cairo_runner.initialize_function_runner(&mut vm)?;

        let mut tmp_state = T::default();
        let hint_processor =
            SyscallHintProcessor::new(BusinessLogicSyscallHandler::default_with(&mut tmp_state));
        let mut runner = StarknetRunner::new(cairo_runner, vm, hint_processor);

        // prepare OS context
        let os_context = runner.prepare_os_context();

        validate_contract_deployed(state, &self.contract_address)?;

        // fetch syscall_ptr
        let initial_syscall_ptr: Relocatable = match os_context.get(0) {
            Some(MaybeRelocatable::RelocatableValue(ptr)) => ptr.to_owned(),
            _ => return Err(TransactionError::NotARelocatableValue),
        };

        let syscall_handler = BusinessLogicSyscallHandler::new(
            tx_execution_context.clone(),
            state,
            resources_manager.clone(),
            self.caller_address.clone(),
            self.contract_address.clone(),
            general_config.clone(),
            initial_syscall_ptr,
        );

        let mut runner = runner.map_hint_processor(SyscallHintProcessor::new(syscall_handler));

        // Positional arguments are passed to *args in the 'run_from_entrypoint' function.
        let data = self.calldata.clone().iter().map(|d| d.into()).collect();
        let alloc_pointer = runner
            .hint_processor
            .syscall_handler
            .allocate_segment(&mut runner.vm, data)?
            .into();

        let entry_point_args = [
            &CairoArg::Single(self.entry_point_selector.clone().into()),
            &CairoArg::Array(os_context.clone()),
            &CairoArg::Single(MaybeRelocatable::Int(self.calldata.len().into())),
            &CairoArg::Single(alloc_pointer),
        ];

        let entrypoint = entry_point.offset;

        // cairo runner entry point
        runner.run_from_entrypoint(entrypoint, &entry_point_args)?;
        runner.validate_and_process_os_context(os_context)?;

        // When execution starts the stack holds entry_points_args + [ret_fp, ret_pc].
        let args_ptr = (runner
            .cairo_runner
            .get_initial_fp()
            .ok_or(TransactionError::MissingInitialFp)?
            - (entry_point_args.len() + 2))?;

        runner
            .vm
            .mark_address_range_as_accessed(args_ptr, entry_point_args.len())?;

        Ok(runner)
    }

    /// Returns the entry point with selector corresponding with self.entry_point_selector, or the
    /// default if there is one and the requested one is not found.
    fn get_selected_entry_point(
        &self,
        contract_class: &ContractClass,
        _class_hash: [u8; 32],
    ) -> Result<ContractEntryPoint, TransactionError> {
        let entry_points = contract_class
            .entry_points_by_type
            .get(&self.entry_point_type)
            .ok_or(TransactionError::InvalidEntryPoints)?;

        let mut default_entry_point = None;
        let entry_point = entry_points
            .iter()
            .filter_map(|x| {
                if x.selector == *DEFAULT_ENTRY_POINT_SELECTOR {
                    default_entry_point = Some(x);
                }

                (x.selector == self.entry_point_selector).then_some(x)
            })
            .fold(Ok(None), |acc, x| match acc {
                Ok(None) => Ok(Some(x)),
                _ => Err(TransactionError::NonUniqueEntryPoint),
            })?;

        entry_point
            .or(default_entry_point)
            .cloned()
            .ok_or(TransactionError::EntryPointNotFound)
    }

    fn build_call_info<S>(
        &self,
        previous_cairo_usage: ExecutionResources,
        syscall_handler: BusinessLogicSyscallHandler<S>,
        retdata: Vec<Felt252>,
    ) -> Result<CallInfo, TransactionError>
    where
        S: State + StateReader,
    {
        let execution_resources =
            &syscall_handler.resources_manager.cairo_usage - &previous_cairo_usage;

        Ok(CallInfo {
            caller_address: self.caller_address.clone(),
            call_type: Some(self.call_type.clone()),
            contract_address: self.contract_address.clone(),
            code_address: self.code_address.clone(),
            class_hash: Some(
                self.get_code_class_hash(syscall_handler.starknet_storage_state.state)?,
            ),
            entry_point_selector: Some(self.entry_point_selector.clone()),
            entry_point_type: Some(self.entry_point_type),
            calldata: self.calldata.clone(),
            retdata,
            execution_resources: execution_resources.filter_unused_builtins(),
            events: syscall_handler.events,
            l2_to_l1_messages: syscall_handler.l2_to_l1_messages,
            storage_read_values: syscall_handler.starknet_storage_state.read_values,
            accessed_storage_keys: syscall_handler.starknet_storage_state.accessed_keys,
            internal_calls: syscall_handler.internal_calls,
        })
    }

    /// Returns the hash of the executed contract class.
    fn get_code_class_hash<S: StateReader>(
        &self,
        state: &mut S,
    ) -> Result<[u8; 32], TransactionError> {
        if self.class_hash.is_some() {
            match self.call_type {
                CallType::Delegate => return Ok(self.class_hash.unwrap()),
                _ => return Err(TransactionError::CallTypeIsNotDelegate),
            }
        }
        let code_address = match self.call_type {
            CallType::Call => Some(self.contract_address.clone()),
            CallType::Delegate => {
                if self.code_address.is_some() {
                    self.code_address.clone()
                } else {
                    return Err(TransactionError::AttempToUseNoneCodeAddress);
                }
            }
        };

        get_deployed_address_class_hash_at_address(state, &code_address.unwrap())
    }
}

#[test]
fn test_execute() {
    //* --------------------------------------------
    //*       Create a default contract data
    //* --------------------------------------------

    let contract_address = Address(1111.into());
    let class_hash = [1; 32];
    let call_data = Vec![Felt252::one];
    let contract_class = ContractClass::new(
        Program::default(),
        HashMap::from([(
            EntryPointType::Constructor,
            vec![ContractEntryPoint::default()],
        )]),
        abi::None,
    );

    //* --------------------------------------------
    //*          Create default context
    //* --------------------------------------------

    let general_config = StarknetGeneralConfig::default();

    let tx_execution_context = TransactionExecutionContext::create_for_testing(
        Address(0.into()),
        10,
        0.into(),
        general_config.invoke_tx_max_n_steps(),
        TRANSACTION_VERSION,
    );

    //* --------------------------------------------
    //*  Create starknet state with the contract
    //*  (This would be the equivalent of
    //*  declaring and deploying the contract)
    //* -------------------------------------------

    let mut state_reader = InMemoryStateReader::new(
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
    );

    state_reader
        .address_to_class_hash
        .insert(contract_address, class_hash);
    state_reader
        .address_to_nonce
        .insert(contract_address, *tx_execution_context.nonce());
    state_reader
        .address_to_storage
        .insert((contract_address, class_hash), Felt252::new(1));
    state_reader
        .class_hash_to_contract_class
        .insert(class_hash, contract_class.unwrap());

    let mut state = CachedState::new(
        state_reader,
        Some(HashMap::from([class_hash, contract_class])),
    );

    //* ------------------------------------
    //*    Create execution entry point
    //* ------------------------------------

    let caller_address = Address(0.into());

    let entry_point_selector = Felt252::from_bytes_be(&calculate_sn_keccak(entry_point.as_bytes()));
    let entry_point = ExecutionEntryPoint::new(
        contract_address,
        call_data,
        entry_point_selector,
        caller_address,
        EntryPointType::External,
        CallType::Delegate.into(),
        class_hash.into(),
    );

    let execution = entry_point.execute(
        state,
        &general_config,
        execution_resources_manager,
        &tx_execution_context,
    );

    assert_eq!(execution.unwrap(), CallInfo())
}
