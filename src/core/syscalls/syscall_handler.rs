use super::business_logic_syscall_handler::BusinessLogicSyscallHandler;
use super::hint_code::*;
use super::os_syscall_handler::OsSyscallHandler;
use super::syscall_request::*;
use super::syscall_response::GetBlockNumberResponse;
use super::syscall_response::GetContractAddressResponse;
use super::syscall_response::{
    CallContractResponse, GetBlockTimestampResponse, GetCallerAddressResponse,
    GetSequencerAddressResponse, GetTxInfoResponse, GetTxSignatureResponse, WriteSyscallResponse,
};
use crate::business_logic::execution::objects::TxInfoStruct;
use crate::business_logic::state::state_api_objects::BlockInfo;
use crate::core::errors::syscall_handler_errors::SyscallHandlerError;
use crate::starknet_storage::errors::storage_errors::StorageError;
use crate::utils::get_big_int;
use crate::utils::Address;
use cairo_rs::any_box;
use cairo_rs::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::{
    BuiltinHintProcessor, HintProcessorData,
};
use cairo_rs::hint_processor::builtin_hint_processor::hint_utils::get_relocatable_from_var_name;
use cairo_rs::hint_processor::hint_processor_definition::{HintProcessor, HintReference};
use cairo_rs::serde::deserialize_program::ApTracking;
use cairo_rs::types::exec_scope::ExecutionScopes;
use cairo_rs::types::relocatable::{MaybeRelocatable, Relocatable};
use cairo_rs::vm::errors::hint_errors::HintError;
use cairo_rs::vm::errors::vm_errors::VirtualMachineError;
use cairo_rs::vm::vm_core::VirtualMachine;
use felt::Felt;
use felt::NewFelt;
use std::any::Any;
use std::collections::HashMap;

//* ---------------------
//* SyscallHandler Trait
//* ---------------------

pub(crate) trait SyscallHandler {
    fn emit_event(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError>;

    fn send_message_to_l1(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError>;

    fn library_call(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError>;

    fn _get_tx_info_ptr(
        &mut self,
        vm: &mut VirtualMachine,
    ) -> Result<Relocatable, SyscallHandlerError>;

    fn _deploy(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<Address, SyscallHandlerError>;

    fn _read_and_validate_syscall_request(
        &mut self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError>;

    // Executes the contract call and fills the CallContractResponse struct.
    fn _call_contract_and_write_response(
        &mut self,
        syscall_name: &str,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let retdata = self._call_contract(syscall_name, vm, syscall_ptr)?;

        let retdata_maybe_reloc = retdata
            .clone()
            .into_iter()
            .map(|item| MaybeRelocatable::from(Felt::new(item)))
            .collect::<Vec<MaybeRelocatable>>();

        let response = CallContractResponse::new(
            retdata.len(),
            self.allocate_segment(vm, retdata_maybe_reloc)?,
        );

        self._write_syscall_response(&response, vm, syscall_ptr)
    }

    fn _call_contract(
        &mut self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<Vec<u64>, SyscallHandlerError>;

    fn _get_caller_address(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<Address, SyscallHandlerError>;

    fn _get_contract_address(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<Address, SyscallHandlerError>;

    fn _storage_read(&mut self, address: Address) -> Result<u64, SyscallHandlerError>;

    fn _storage_write(&mut self, address: Address, value: u64);

    fn allocate_segment(
        &mut self,
        vm: &mut VirtualMachine,
        data: Vec<MaybeRelocatable>,
    ) -> Result<Relocatable, SyscallHandlerError>;

    fn _write_syscall_response<T: WriteSyscallResponse>(
        &self,
        response: &T,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        response.write_syscall_response(vm, syscall_ptr)
    }

    fn get_block_info(&self) -> &BlockInfo;

    fn get_block_number(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        self._read_and_validate_syscall_request("get_block_number", vm, syscall_ptr)?;
        GetBlockNumberResponse::new(self.get_block_info().block_number)
            .write_syscall_response(vm, syscall_ptr)
    }

    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    // ***********************************
    //  Implementation of Default methods
    // ***********************************
    // ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

    fn get_tx_info(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let _request =
            match self._read_and_validate_syscall_request("get_tx_info", vm, syscall_ptr)? {
                SyscallRequest::GetTxInfo(request) => request,
                _ => Err(SyscallHandlerError::InvalidSyscallReadRequest)?,
            };

        let tx_info = self._get_tx_info_ptr(vm)?;

        let response = GetTxInfoResponse::new(tx_info);
        response.write_syscall_response(vm, syscall_ptr)
    }

    fn get_tx_signature(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let request =
            match self._read_and_validate_syscall_request("get_tx_signature", vm, syscall_ptr)? {
                SyscallRequest::GetTxSignature(request) => request,
                _ => return Err(SyscallHandlerError::ExpectedGetTxSignatureRequest),
            };

        let tx_info_pr = self._get_tx_info_ptr(vm)?;
        let tx_info = TxInfoStruct::from_ptr(vm, tx_info_pr)?;
        let response = GetTxSignatureResponse::new(tx_info.signature, tx_info.signature_len);

        response.write_syscall_response(vm, syscall_ptr)
    }

    fn get_block_timestamp(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let _request = if let SyscallRequest::GetBlockTimestamp(request) =
            self._read_and_validate_syscall_request("get_block_timestamp", vm, syscall_ptr)?
        {
            request
        } else {
            return Err(SyscallHandlerError::ExpectedGetBlockTimestampRequest);
        };

        let block_timestamp = self.get_block_info().block_timestamp;

        let response = GetBlockTimestampResponse::new(block_timestamp);

        response.write_syscall_response(vm, syscall_ptr)
    }

    fn get_caller_address(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let caller_address = self._get_caller_address(vm, syscall_ptr)?;
        let response = GetCallerAddressResponse::new(caller_address);
        response.write_syscall_response(vm, syscall_ptr)
    }

    fn get_contract_address(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let contract_address = self._get_contract_address(vm, syscall_ptr)?;
        let response = GetContractAddressResponse::new(contract_address);
        response.write_syscall_response(vm, syscall_ptr)
    }

    fn get_sequencer_address(
        &mut self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let _request = if let SyscallRequest::GetSequencerAddress(request) =
            self._read_and_validate_syscall_request("get_sequencer_address", vm, syscall_ptr)?
        {
            request
        } else {
            return Err(SyscallHandlerError::ExpectedGetSequencerAddressRequest);
        };

        let sequencer_address = self.get_block_info().sequencer_address.clone();

        let response = GetSequencerAddressResponse::new(sequencer_address);

        response.write_syscall_response(vm, syscall_ptr)
    }

    fn read_syscall_request(
        &self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        match syscall_name {
            "emit_event" => EmitEventStruct::from_ptr(vm, syscall_ptr),
            "get_tx_info" => GetTxInfoRequest::from_ptr(vm, syscall_ptr),
            "deploy" => DeployRequestStruct::from_ptr(vm, syscall_ptr),
            "send_message_to_l1" => SendMessageToL1SysCall::from_ptr(vm, syscall_ptr),
            "library_call" => LibraryCallStruct::from_ptr(vm, syscall_ptr),
            "get_caller_address" => GetCallerAddressRequest::from_ptr(vm, syscall_ptr),
            "get_contract_address" => GetContractAddressRequest::from_ptr(vm, syscall_ptr),
            "get_sequencer_address" => GetSequencerAddressRequest::from_ptr(vm, syscall_ptr),
            "get_block_number" => GetBlockNumberRequest::from_ptr(vm, syscall_ptr),
            "get_tx_signature" => GetTxSignatureRequest::from_ptr(vm, syscall_ptr),
            "get_block_timestamp" => GetBlockTimestampRequest::from_ptr(vm, syscall_ptr),
            _ => Err(SyscallHandlerError::UnknownSyscall(
                syscall_name.to_string(),
            )),
        }
    }
}

//* ------------------------
//* Structs implementations
//* ------------------------

pub(crate) struct SyscallHintProcessor<H: SyscallHandler> {
    pub(crate) builtin_hint_processor: BuiltinHintProcessor,
    #[allow(unused)] // TODO: remove after using.
    pub(crate) syscall_handler: H,
}

impl SyscallHintProcessor<BusinessLogicSyscallHandler> {
    #[allow(unused)] // TODO: Remove once used.
    pub fn new_empty(
    ) -> Result<SyscallHintProcessor<BusinessLogicSyscallHandler>, SyscallHandlerError> {
        Ok(SyscallHintProcessor {
            builtin_hint_processor: BuiltinHintProcessor::new_empty(),
            syscall_handler: BusinessLogicSyscallHandler::new(BlockInfo::default()),
        })
    }

    pub fn new_empty_os() -> Result<SyscallHintProcessor<OsSyscallHandler>, SyscallHandlerError> {
        Ok(SyscallHintProcessor {
            builtin_hint_processor: BuiltinHintProcessor::new_empty(),
            syscall_handler: OsSyscallHandler::default(),
        })
    }
}

impl<H: SyscallHandler> SyscallHintProcessor<H> {
    pub fn should_run_syscall_hint(
        &mut self,
        vm: &mut VirtualMachine,
        exec_scopes: &mut ExecutionScopes,
        hint_data: &Box<dyn Any>,
        constants: &HashMap<String, Felt>,
    ) -> Result<bool, HintError> {
        match self
            .builtin_hint_processor
            .execute_hint(vm, exec_scopes, hint_data, constants)
        {
            Ok(()) => Ok(false),
            Err(HintError::UnknownHint(_)) => Ok(true),
            Err(e) => Err(e),
        }
    }

    fn execute_syscall_hint(
        &mut self,
        vm: &mut VirtualMachine,
        _exec_scopes: &mut ExecutionScopes,
        hint_data: &Box<dyn Any>,
        _constants: &HashMap<String, Felt>,
    ) -> Result<(), SyscallHandlerError> {
        let hint_data = hint_data
            .downcast_ref::<HintProcessorData>()
            .ok_or(SyscallHandlerError::WrongHintData)?;

        match &*hint_data.code {
            DEPLOY => Err(SyscallHandlerError::NotImplemented),
            EMIT_EVENT_CODE => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.emit_event(vm, syscall_ptr)
            }
            GET_BLOCK_NUMBER => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_block_number(vm, syscall_ptr)
            }
            GET_BLOCK_TIMESTAMP => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_block_timestamp(vm, syscall_ptr)
            }
            GET_CALLER_ADDRESS => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_caller_address(vm, syscall_ptr)
            }
            GET_SEQUENCER_ADDRESS => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_sequencer_address(vm, syscall_ptr)
            }
            LIBRARY_CALL => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.library_call(vm, syscall_ptr)
            }
            SEND_MESSAGE_TO_L1 => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.send_message_to_l1(vm, syscall_ptr)
            }
            GET_TX_SIGNATURE => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_tx_signature(vm, syscall_ptr)
            }
            GET_TX_INFO => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_tx_info(vm, syscall_ptr)
            }
            GET_CONTRACT_ADDRESS => {
                let syscall_ptr = get_syscall_ptr(vm, &hint_data.ids_data, &hint_data.ap_tracking)?;
                self.syscall_handler.get_contract_address(vm, syscall_ptr)
            }
            _ => Err(SyscallHandlerError::NotImplemented),
        }
    }
}

impl<H: SyscallHandler> HintProcessor for SyscallHintProcessor<H> {
    fn execute_hint(
        &mut self,
        vm: &mut VirtualMachine,
        exec_scopes: &mut ExecutionScopes,
        hint_data: &Box<dyn Any>,
        constants: &HashMap<String, Felt>,
    ) -> Result<(), HintError> {
        if self.should_run_syscall_hint(vm, exec_scopes, hint_data, constants)? {
            self.execute_syscall_hint(vm, exec_scopes, hint_data, constants)
                .map_err(|e| HintError::UnknownHint(e.to_string()))?;
        }
        Ok(())
    }
}

fn get_ids_data(
    reference_ids: &HashMap<String, usize>,
    references: &HashMap<usize, HintReference>,
) -> Result<HashMap<String, HintReference>, HintError> {
    let mut ids_data = HashMap::<String, HintReference>::new();
    for (path, ref_id) in reference_ids {
        let name = path.rsplit('.').next().ok_or(HintError::FailedToGetIds)?;
        ids_data.insert(
            name.to_string(),
            references
                .get(ref_id)
                .ok_or(HintError::FailedToGetIds)?
                .clone(),
        );
    }
    Ok(ids_data)
}

fn get_syscall_ptr(
    vm: &VirtualMachine,
    ids_data: &HashMap<String, HintReference>,
    ap_tracking: &ApTracking,
) -> Result<Relocatable, SyscallHandlerError> {
    let location = get_relocatable_from_var_name("syscall_ptr", vm, ids_data, ap_tracking)
        .map_err(|_| SyscallHandlerError::SegmentationFault)?;
    let syscall_ptr = vm
        .get_relocatable(&location)
        .map_err(|_| SyscallHandlerError::SegmentationFault)?;
    Ok(syscall_ptr)
}

#[cfg(test)]
mod tests {

    use crate::business_logic::execution::objects::{
        OrderedEvent, OrderedL2ToL1Message, TransactionExecutionContext,
    };
    use crate::utils::test_utils::ids_data;
    use crate::utils::{get_big_int, get_integer, get_relocatable};
    use crate::{
        add_segments, core::syscalls::os_syscall_handler::OsSyscallHandler, utils::test_utils::vm,
    };
    use crate::{allocate_selector, memory_insert};
    use cairo_rs::relocatable;
    use num_traits::ToPrimitive;

    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn read_send_message_to_l1_request() {
        let syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();
        add_segments!(vm, 3);

        memory_insert!(
            vm,
            [((1, 0), 0), ((1, 1), 1), ((1, 2), 2), ((1, 4), (2, 0))]
        );
        assert_eq!(
            syscall.read_syscall_request("send_message_to_l1", &vm, relocatable!(1, 0)),
            Ok(SyscallRequest::SendMessageToL1(SendMessageToL1SysCall {
                _selector: 0.into(),
                to_address: Address(1.into()),
                payload_size: 2,
                payload_ptr: relocatable!(2, 0)
            }))
        )
    }

    #[test]
    fn read_deploy_syscall_request() {
        let syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();
        add_segments!(vm, 2);

        memory_insert!(
            vm,
            [
                ((1, 0), 0),
                ((1, 1), 1),
                ((1, 2), 2),
                ((1, 3), 3),
                ((1, 4), (1, 20)),
                ((1, 5), 4)
            ]
        );

        assert_eq!(
            syscall.read_syscall_request("deploy", &vm, relocatable!(1, 0)),
            Ok(SyscallRequest::Deploy(DeployRequestStruct {
                _selector: 0.into(),
                class_hash: 1.into(),
                contract_address_salt: 2.into(),
                constructor_calldata_size: 3.into(),
                constructor_calldata: relocatable!(1, 20),
                deploy_from_zero: 4,
            }))
        )
    }

    #[test]
    fn get_block_timestamp_for_business_logic() {
        let mut syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();
        add_segments!(vm, 2);

        memory_insert!(
            vm,
            [
                ((1, 0), (1, 1)), // syscall_ptr
                ((1, 1), 18)
            ]
        );

        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_SEQUENCER_ADDRESS.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler = SyscallHintProcessor::new_empty().unwrap();
        syscall_handler
            .execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
            .unwrap();

        // Check that syscall.get_block_timestamp insert syscall.get_block_info().block_timestamp in the (1,2) position
        assert_eq!(
            get_big_int(&vm, &relocatable!(1, 2)).unwrap(),
            syscall.get_block_info().block_timestamp.into()
        );
    }

    #[test]
    fn get_sequencer_address_for_business_logic() {
        let mut syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();
        add_segments!(vm, 2);

        memory_insert!(vm, [((1, 0), (1, 1)), ((1, 1), 18)]);

        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_SEQUENCER_ADDRESS.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler = SyscallHintProcessor::new_empty().unwrap();
        syscall_handler
            .execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
            .unwrap();

        // Check that syscall.get_sequencer insert syscall.get_block_info().sequencer_address in the (1,1) position
        assert_eq!(get_big_int(&vm, &relocatable!(1, 2)).unwrap(), 0.into())
    }

    #[test]
    fn emit_event_test() {
        // create data and variables to execute hint

        let mut vm = vm!();
        add_segments!(vm, 4);

        // insert keys and data to generate the event
        // keys ptr points to (3,0)
        // data ptr points to (3,3)

        // selector of syscall
        let selector = "1280709301550335749748";

        allocate_selector!(vm, ((2, 0), selector.as_bytes()));
        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), // syscall ptr
                ((2, 1), 2),      // keys len
                ((2, 2), (3, 0)), // keys ptr
                ((2, 3), 2),      // data len
                ((2, 4), (3, 3)), // data ptr
                ((3, 0), 1),      // keys pointed by key ptr
                ((3, 1), 1),
                ((3, 3), 1), // data pointed by data ptr
                ((3, 4), 1)
            ]
        );
        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(EMIT_EVENT_CODE.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler = SyscallHintProcessor::new_empty().unwrap();
        syscall_handler
            .execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
            .unwrap();

        let event = syscall_handler
            .syscall_handler
            .events
            .get(0)
            .unwrap()
            .clone();

        assert_eq!(
            OrderedEvent::new(
                0,
                Vec::from([1.into(), 1.into()]),
                Vec::from([1.into(), 1.into()])
            ),
            event
        );
        assert_eq!(
            syscall_handler
                .syscall_handler
                .tx_execution_context
                .n_emitted_events,
            1
        );
    }

    #[test]
    fn get_tx_info_for_business_logic_test() {
        let mut vm = vm!();
        add_segments!(vm, 3);

        // insert data to form the request
        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), //  syscall_ptr
                ((2, 0), 8)       //  GetTxInfoRequest.selector
            ]
        );

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_TX_INFO.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler_hint_processor = SyscallHintProcessor::new_empty().unwrap();

        let tx_execution_context = TransactionExecutionContext {
            n_emitted_events: 50,
            version: 51,
            account_contract_address: Address(260.into()),
            max_fee: 261,
            transaction_hash: 262.into(),
            signature: vec![300.into(), 301.into()],
            nonce: 263.into(),
            n_sent_messages: 52,
        };
        syscall_handler_hint_processor
            .syscall_handler
            .tx_execution_context = tx_execution_context.clone();

        let result = syscall_handler_hint_processor.execute_hint(
            &mut vm,
            &mut ExecutionScopes::new(),
            &any_box!(hint_data),
            &HashMap::new(),
        );

        assert_eq!(result, Ok(()));

        // Check VM inserts

        // TransactionExecutionContext.signature
        assert_eq!(
            vm.get_integer(&relocatable!(3, 0)).unwrap().into_owned(),
            tx_execution_context.signature[0]
        );
        assert_eq!(
            vm.get_integer(&relocatable!(3, 1)).unwrap().into_owned(),
            tx_execution_context.signature[1]
        );

        // TxInfoStruct
        assert_eq!(
            get_integer(&vm, &relocatable!(4, 0)),
            Ok(tx_execution_context.version)
        );
        assert_eq!(
            get_big_int(&vm, &relocatable!(4, 1)),
            Ok(tx_execution_context.account_contract_address.0)
        );
        assert_eq!(
            get_integer(&vm, &relocatable!(4, 2)),
            Ok(tx_execution_context.max_fee as usize)
        );
        assert_eq!(
            get_integer(&vm, &relocatable!(4, 3)),
            Ok(tx_execution_context.signature.len())
        );
        assert_eq!(
            get_relocatable(&vm, &relocatable!(4, 4)),
            Ok(relocatable!(3, 0))
        );
        assert_eq!(
            get_big_int(&vm, &relocatable!(4, 5)),
            Ok(tx_execution_context.transaction_hash)
        );
        assert_eq!(
            get_big_int(&vm, &relocatable!(4, 6)),
            Ok(syscall_handler_hint_processor
                .syscall_handler
                .general_config
                .starknet_os_config
                .chain_id
                .to_felt())
        );

        assert_eq!(
            get_big_int(&vm, &relocatable!(4, 7)),
            Ok(tx_execution_context.nonce)
        );

        // GetTxInfoResponse
        assert_eq!(
            vm.get_relocatable(&relocatable!(2, 1)),
            Ok(relocatable!(4, 0))
        );
    }

    #[test]
    fn get_tx_info_for_business_logic_with_tx_info_ptr() {
        let mut vm = vm!();
        add_segments!(vm, 3);

        // insert data to form the request
        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), //  syscall_ptr
                ((2, 0), 8)       //  GetTxInfoRequest.selector
            ]
        );

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_TX_INFO.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler_hint_processor = SyscallHintProcessor::new_empty().unwrap();

        syscall_handler_hint_processor.syscall_handler.tx_info_ptr =
            Some(relocatable!(7, 0).into());

        let result = syscall_handler_hint_processor.execute_hint(
            &mut vm,
            &mut ExecutionScopes::new(),
            &any_box!(hint_data),
            &HashMap::new(),
        );

        assert_eq!(result, Ok(()));

        // GetTxInfoResponse
        assert_eq!(
            vm.get_relocatable(&relocatable!(2, 1)),
            Ok(relocatable!(7, 0))
        );
    }

    #[test]
    fn get_tx_info_for_os_syscall_test() {
        let mut vm = vm!();
        add_segments!(vm, 3);

        // insert data to form the request
        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), //  syscall_ptr
                ((2, 0), 8)       //  GetTxInfoRequest.selector
            ]
        );

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_TX_INFO.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler_hint_processor = SyscallHintProcessor::new_empty_os().unwrap();

        syscall_handler_hint_processor.syscall_handler.tx_info_ptr = Some(relocatable!(18, 12));

        let result = syscall_handler_hint_processor.execute_hint(
            &mut vm,
            &mut ExecutionScopes::new(),
            &any_box!(hint_data),
            &HashMap::new(),
        );

        assert_eq!(result, Ok(()));

        // Check VM inserts
        // GetTxInfoResponse
        assert_eq!(
            vm.get_relocatable(&relocatable!(2, 1)),
            Ok(relocatable!(18, 12))
        );
    }

    #[test]
    fn test_get_caller_address_ok() {
        let mut vm = vm!();

        add_segments!(vm, 2);

        // direction (1,0) is the sycall_ptr
        memory_insert!(vm, [((1, 0), (1, 1)), ((1, 1), 0)]);

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_CALLER_ADDRESS.to_string(), ids_data);
        // invoke syscall
        let mut hint_processor = SyscallHintProcessor::new_empty().unwrap();
        hint_processor
            .execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
            .unwrap();

        // response is written in direction (1,2)
        assert_eq!(
            get_big_int(&vm, &relocatable!(1, 2)).unwrap(),
            hint_processor.syscall_handler.caller_address.0
        )
    }

    #[test]
    fn test_send_message_to_l1_ok() {
        let mut vm = vm!();

        add_segments!(vm, 3);

        // parameters are read from memory location (1,1)
        memory_insert!(
            vm,
            [
                ((1, 0), (1, 1)), // syscall_ptr
                ((1, 1), 0),
                ((1, 2), 1),
                ((1, 3), 2),
                ((1, 5), (2, 0)),
                ((2, 0), 18),
                ((2, 1), 12)
            ]
        );

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(SEND_MESSAGE_TO_L1.to_string(), ids_data);
        // invoke syscall
        let mut hint_processor = SyscallHintProcessor::new_empty().unwrap();
        hint_processor
            .execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
            .unwrap();

        assert_eq!(
            hint_processor
                .syscall_handler
                .tx_execution_context
                .n_sent_messages,
            1
        );
        assert_eq!(
            hint_processor.syscall_handler.l2_to_l1_messages,
            vec![OrderedL2ToL1Message::new(
                hint_processor
                    .syscall_handler
                    .tx_execution_context
                    .n_sent_messages
                    - 1,
                Address(1.into()),
                vec![18.into(), 12.into()],
            )]
        );
    }

    #[test]
    fn test_get_block_number() {
        let mut syscall = OsSyscallHandler::new(
            VecDeque::new(),
            VecDeque::new(),
            VecDeque::new(),
            VecDeque::new(),
            VecDeque::new(),
            VecDeque::new(),
            HashMap::new(),
            None,
            None,
            BlockInfo::default(),
        );
        let mut vm = vm!();

        add_segments!(vm, 4);
        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), // Syscall pointer.
                ((2, 0), 0)       // selector
            ]
        );

        let mut hint_processor =
            SyscallHintProcessor::new_empty().expect("Could not create the syscall hint processor");

        let hint_data =
            HintProcessorData::new_default(GET_BLOCK_NUMBER.to_string(), ids_data!["syscall_ptr"]);
        assert_eq!(
            hint_processor.execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            ),
            Ok(()),
        );
        assert_eq!(get_integer(&vm, &relocatable!(2, 1)), Ok(0));
    }

    #[test]
    fn test_get_contract_address_ok() {
        let mut vm = vm!();

        add_segments!(vm, 2);

        // direction (1,0) is the sycall_ptr
        memory_insert!(vm, [((1, 0), (1, 1)), ((1, 1), 0)]);

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_CONTRACT_ADDRESS.to_string(), ids_data);
        // invoke syscall
        let mut hint_processor = SyscallHintProcessor::new_empty().unwrap();
        hint_processor
            .execute_hint(
                &mut vm,
                &mut ExecutionScopes::new(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
            .unwrap();

        // response is written in direction (1,2)
        assert_eq!(
            get_big_int(&vm, &relocatable!(1, 2)).unwrap(),
            hint_processor.syscall_handler.contract_address.0
        )
    }

    #[test]
    fn test_gt_tx_signature() {
        let mut vm = vm!();

        add_segments!(vm, 3);

        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), //  syscall_ptr
                ((2, 0), 8)       //  GetTxInfoRequest.selector
            ]
        );

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_TX_SIGNATURE.to_string(), ids_data);
        // invoke syscall
        let mut syscall_handler_hint_processor = SyscallHintProcessor::new_empty().unwrap();

        let tx_execution_context = TransactionExecutionContext {
            n_emitted_events: 50,
            version: 51,
            account_contract_address: Address(260.into()),
            max_fee: 261,
            transaction_hash: 262.into(),
            signature: vec![300.into(), 301.into()],
            nonce: 263.into(),
            n_sent_messages: 52,
        };
        syscall_handler_hint_processor
            .syscall_handler
            .tx_execution_context = tx_execution_context.clone();

        let result = syscall_handler_hint_processor.execute_hint(
            &mut vm,
            &mut ExecutionScopes::new(),
            &any_box!(hint_data),
            &HashMap::new(),
        );

        assert!(result.is_ok());
        assert_eq!(
            get_integer(&vm, &relocatable!(2, 1)).unwrap(),
            tx_execution_context.signature.len()
        );
        assert_eq!(
            vm.get_relocatable(&relocatable!(2, 2)).unwrap(),
            relocatable!(3, 0)
        );
    }
}
