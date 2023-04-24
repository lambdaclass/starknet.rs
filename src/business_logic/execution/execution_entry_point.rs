use super::objects::{CallInfo, CallType, TransactionExecutionContext};
use crate::{
    business_logic::{
        fact_state::state::ExecutionResourcesManager, state::state_api::State,
        state::state_api::StateReader, transaction::error::TransactionError,
    },
    core::syscalls::{
        deprecated_business_logic_syscall_handler::DeprecatedBLSyscallHandler,
        deprecated_syscall_handler::{DeprecatedSyscallHandler, SyscallHintProcessor},
    },
    definitions::{constants::DEFAULT_ENTRY_POINT_SELECTOR, general_config::StarknetGeneralConfig},
    services::api::contract_classes::{
        compiled_class::CompiledClass,
        deprecated_contract_class::{ContractClass, ContractEntryPoint, EntryPointType},
    },
    starknet_runner::runner::StarknetRunner,
    utils::{get_deployed_address_class_hash_at_address, validate_contract_deployed, Address},
};
use cairo_rs::{
    types::relocatable::{MaybeRelocatable, Relocatable},
    vm::{
        runners::cairo_runner::{CairoArg, CairoRunner, ExecutionResources},
        vm_core::VirtualMachine,
    },
};
use felt::Felt252;

/// Represents a Cairo entry point execution of a StarkNet contract.

// TODO:initial_gas is a new field added in the current changes, it should be checked if we delete it once the new execution entry point is done
#[derive(Debug)]
pub struct ExecutionEntryPoint {
    call_type: CallType,
    pub(crate) contract_address: Address,
    code_address: Option<Address>,
    class_hash: Option<[u8; 32]>,
    calldata: Vec<Felt252>,
    caller_address: Address,
    entry_point_selector: Felt252,
    entry_point_type: EntryPointType,
    #[allow(unused)]
    initial_gas: u64,
}
#[allow(clippy::too_many_arguments)]
impl ExecutionEntryPoint {
    pub fn new(
        contract_address: Address,
        calldata: Vec<Felt252>,
        entry_point_selector: Felt252,
        caller_address: Address,
        entry_point_type: EntryPointType,
        call_type: Option<CallType>,
        class_hash: Option<[u8; 32]>,
        initial_gas: u64,
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
            initial_gas,
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

        let class_hash = self.get_code_class_hash(state)?;
        let contract_class = state
            .get_compiled_class(&class_hash)
            .map_err(|_| TransactionError::MissingCompiledClass)?;

        let runner = match contract_class {
            CompiledClass::Deprecated(contract_class) => self._execute_version0_class(
                state,
                resources_manager,
                general_config,
                tx_execution_context,
                contract_class,
                class_hash,
            )?,
            CompiledClass::Casm(_contract_class) => {
                todo!()
            }
        };

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

    /// Executes the selected entry point with the given calldata in the specified contract.
    /// The information collected from this run (number of steps required, modifications to the
    /// contract storage, etc.) is saved on the resources manager.
    /// Returns a CallInfo object that represents the execution.

    pub fn execute_v2<S>(
        &self,
        _state: &mut S,
        _general_config: &mut StarknetGeneralConfig,
        _support_reverted: bool,
    ) -> Result<CallInfo, TransactionError>
    where
        S: Default + State + StateReader,
    {
        todo!()
    }

    /// Runs the selected entry point with the given calldata in the code of the contract deployed
    /// at self.code_address.
    /// The execution is done in the context (e.g., storage) of the contract at
    /// self.contract_address.
    /// Returns the corresponding CairoFunctionRunner and DeprecatedBLSyscallHandler in order to
    /// retrieve the execution information.
    #[allow(dead_code)]
    fn run<'a, T>(
        &self,
        _state: &'a mut T,
        _resources_manager: &ExecutionResourcesManager,
        _general_config: &StarknetGeneralConfig,
        _tx_execution_context: &TransactionExecutionContext,
    ) -> Result<StarknetRunner<DeprecatedBLSyscallHandler<'a, T>>, TransactionError>
    where
        T: Default + State + StateReader,
    {
        todo!()
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
        syscall_handler: DeprecatedBLSyscallHandler<S>,
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
            failure_flag: false,
            gas_consumed: 0,
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

    fn _execute_version0_class<'a, T>(
        &self,
        state: &'a mut T,
        resources_manager: &ExecutionResourcesManager,
        general_config: &StarknetGeneralConfig,
        tx_execution_context: &TransactionExecutionContext,
        contract_class: Box<ContractClass>,
        class_hash: [u8; 32],
    ) -> Result<StarknetRunner<DeprecatedBLSyscallHandler<'a, T>>, TransactionError>
    where
        T: Default + State + StateReader,
    {
        // fetch selected entry point
        let entry_point = self.get_selected_entry_point(&contract_class, class_hash)?;

        // create starknet runner
        let mut vm = VirtualMachine::new(false);
        let mut cairo_runner = CairoRunner::new(&contract_class.program, "all_cairo", false)?;
        cairo_runner.initialize_function_runner(&mut vm, true)?;

        let mut tmp_state = T::default();
        let hint_processor =
            SyscallHintProcessor::new(DeprecatedBLSyscallHandler::default_with(&mut tmp_state));
        let mut runner = StarknetRunner::new(cairo_runner, vm, hint_processor);

        // prepare OS context
        let os_context = runner.prepare_os_context();

        validate_contract_deployed(state, &self.contract_address)?;

        // fetch syscall_ptr
        let initial_syscall_ptr: Relocatable = match os_context.get(0) {
            Some(MaybeRelocatable::RelocatableValue(ptr)) => ptr.to_owned(),
            _ => return Err(TransactionError::NotARelocatableValue),
        };

        let syscall_handler = DeprecatedBLSyscallHandler::new(
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
}
