use super::syscall_request::*;
use super::syscall_response::GetBlockNumberResponse;
use crate::business_logic::execution::objects::*;
use crate::business_logic::execution::state::ExecutionResourcesManager;
use crate::core::errors::syscall_handler_errors::SyscallHandlerError;
use crate::core::syscalls::syscall_handler::SyscallHandler;
use crate::definitions::general_config::StarknetGeneralConfig;
use crate::hash_utils::calculate_contract_address_from_hash;
use crate::state::state_api_objects::BlockInfo;
use crate::utils::*;
use cairo_rs::types::relocatable::{MaybeRelocatable, Relocatable};
use cairo_rs::vm::vm_core::VirtualMachine;
use num_bigint::BigInt;
use num_traits::{One, Zero};
use std::cell::RefCell;

//* -----------------------------------
//* BusinessLogicHandler implementation
//* -----------------------------------

pub struct BusinessLogicSyscallHandler {
    tx_execution_context: RefCell<TransactionExecutionContext>,
    /// Events emitted by the current contract call.
    events: RefCell<Vec<OrderedEvent>>,
    /// A list of dynamically allocated segments that are expected to be read-only.
    read_only_segments: RefCell<Vec<(Relocatable, MaybeRelocatable)>>,
    resources_manager: RefCell<ExecutionResourcesManager>,
    contract_address: u64,
    l2_to_l1_messages: Vec<OrderedL2ToL1Message>,
    general_config: StarknetGeneralConfig,
    tx_info_ptr: RefCell<Option<MaybeRelocatable>>,
    block_info: BlockInfo,
}

impl BusinessLogicSyscallHandler {
    pub fn new(block_info: BlockInfo) -> Self {
        let events = RefCell::new(Vec::new());
        let tx_execution_context = RefCell::new(TransactionExecutionContext::new());
        let read_only_segments = RefCell::new(Vec::new());
        let resources_manager = RefCell::new(ExecutionResourcesManager::default());
        let general_config = StarknetGeneralConfig::new();
        let tx_info_ptr = RefCell::new(None);

        BusinessLogicSyscallHandler {
            events,
            tx_execution_context,
            general_config,
            tx_info_ptr,
            read_only_segments,
            resources_manager,
            contract_address: 0,
            l2_to_l1_messages: Vec::new(),
            block_info,
        }
    }

    /// Increments the syscall count for a given `syscall_name` by 1.
    fn increment_syscall_count(&self, syscall_name: &str) {
        self.resources_manager
            .borrow_mut()
            .increment_syscall_counter(syscall_name, 1);
    }
}

impl SyscallHandler for BusinessLogicSyscallHandler {
    fn emit_event(
        &self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let request = match self._read_and_validate_syscall_request("emit_event", vm, syscall_ptr) {
            Ok(SyscallRequest::EmitEvent(emit_event_struct)) => emit_event_struct,
            _ => return Err(SyscallHandlerError::InvalidSyscallReadRequest),
        };

        let keys_len = request.keys_len;
        let data_len = request.data_len;

        let order = self.tx_execution_context.borrow_mut().n_emitted_events;
        let keys: Vec<BigInt> = get_integer_range(vm, &request.keys, keys_len)?;
        let data: Vec<BigInt> = get_integer_range(vm, &request.data, data_len)?;

        self.events
            .borrow_mut()
            .push(OrderedEvent::new(order, keys, data));

        // Update events count.
        self.tx_execution_context.borrow_mut().n_emitted_events += 1;
        Ok(())
    }

    fn get_tx_info(
        &self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let _request =
            match self._read_and_validate_syscall_request("get_tx_info", vm, syscall_ptr)? {
                SyscallRequest::GetTxInfo(request) => request,
                _ => Err(SyscallHandlerError::InvalidSyscallReadRequest)?,
            };

        Err(SyscallHandlerError::NotImplemented)
    }

    fn send_message_to_l1(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let request = if let SyscallRequest::SendMessageToL1(request) =
            self._read_and_validate_syscall_request("send_message_to_l1", vm, syscall_ptr)?
        {
            request
        } else {
            return Err(SyscallHandlerError::ExpectedSendMessageToL1);
        };

        let payload = get_integer_range(vm, &request.payload_ptr, request.payload_size)?;

        self.l2_to_l1_messages.push(OrderedL2ToL1Message::new(
            self.tx_execution_context.borrow().n_sent_messages,
            request.to_address,
            payload,
        ));

        // Update messages count.
        self.tx_execution_context.borrow_mut().n_sent_messages += 1;
        Ok(())
    }

    fn library_call(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        self._call_contract_and_write_response("library_call", vm, syscall_ptr)?;
        Ok(())
    }

    fn _get_tx_info_ptr(
        &self,
        vm: &mut VirtualMachine,
    ) -> Result<MaybeRelocatable, SyscallHandlerError> {
        let mut tx_info_ptr = self.tx_info_ptr.borrow_mut();

        if let Some(ptr) = &*tx_info_ptr {
            Ok(ptr.clone())
        } else {
            let tx = self.tx_execution_context.borrow();

            let version = tx.version;
            let account_contract_address = tx.account_contract_address.clone();
            let max_fee = tx.max_fee.clone();
            let transaction_hash = tx.transaction_hash.clone();
            let nonce = tx.nonce.clone();
            let signature = vm.add_memory_segment();
            let signature = vm
                .write_arg(&signature, &tx.signature)
                .map_err(|x| SyscallHandlerError::VirtualMachineError(x.into()))?;
            let signature = signature.get_relocatable()?.clone();
            let signature_len = signature.offset;

            let chain_id = self.general_config.starknet_os_config.chain_id as usize;

            let tx_info = TxInfoStruct {
                version,
                account_contract_address,
                max_fee,
                transaction_hash,
                nonce,
                signature,
                signature_len,
                chain_id,
            };

            let segment = vm.add_memory_segment();

            let tx_info_ptr_temp = vm
                .write_arg(&segment, &tx_info)
                .map_err(|x| SyscallHandlerError::VirtualMachineError(x.into()))?;

            *tx_info_ptr = Some(tx_info_ptr_temp.clone());

            Ok(tx_info_ptr_temp)
        }
    }

    fn _deploy(
        &mut self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<u32, SyscallHandlerError> {
        let request = if let SyscallRequest::Deploy(request) =
            self._read_and_validate_syscall_request("deploy", vm, syscall_ptr)?
        {
            request
        } else {
            return Err(SyscallHandlerError::ExpectedDeployRequestStruct);
        };

        if !(request.deploy_from_zero.is_zero() || request.deploy_from_zero.is_one()) {
            return Err(SyscallHandlerError::DeployFromZero(
                request.deploy_from_zero,
            ));
        };

        let constructor_calldata = get_integer_range(
            vm,
            &request.constructor_calldata,
            bigint_to_usize(&request.constructor_calldata_size)?,
        )?;

        let class_hash = &request.class_hash;

        let deployer_address = if request.deploy_from_zero.is_zero() {
            self.contract_address
        } else {
            0
        };

        let _contract_address = calculate_contract_address_from_hash(
            &request.contract_address_salt,
            class_hash,
            &constructor_calldata,
            deployer_address,
        )?;

        // Initialize the contract.
        let (_sign, _class_hash_bytes) = request.class_hash.to_bytes_be();

        todo!()
    }

    fn _read_and_validate_syscall_request(
        &self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        self.increment_syscall_count(syscall_name);
        self.read_syscall_request(syscall_name, vm, syscall_ptr)
    }

    fn _call_contract_and_write_response(
        &mut self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let _response_data = self._call_contract(syscall_name, vm, syscall_ptr)?;
        // TODO: Should we build a response struct to pass to _write_syscall_response?
        // self._write_syscall_response(response_data, vm, syscall_ptr);
        todo!()
    }

    fn _call_contract(
        &mut self,
        _syscall_name: &str,
        _vm: &VirtualMachine,
        _syscall_ptr: Relocatable,
    ) -> Result<Vec<u32>, SyscallHandlerError> {
        todo!()
    }
    fn _get_caller_address(
        &self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<u64, SyscallHandlerError> {
        let _request = if let SyscallRequest::GetCallerAddress(request) =
            self._read_and_validate_syscall_request("get_caller_address", vm, syscall_ptr)?
        {
            request
        } else {
            return Err(SyscallHandlerError::ExpectedGetCallerAddressRequest);
        };

        Ok(self.contract_address)
    }
    fn _get_contract_address(
        &self,
        _vm: VirtualMachine,
        _syscall_ptr: Relocatable,
    ) -> Result<u32, SyscallHandlerError> {
        todo!()
    }
    fn _storage_read(&mut self, _address: u32) -> Result<u32, SyscallHandlerError> {
        todo!()
    }
    fn _storage_write(&mut self, _address: u32, _value: u32) {
        todo!()
    }

    fn allocate_segment(
        &self,
        vm: &mut VirtualMachine,
        data: Vec<MaybeRelocatable>,
    ) -> Result<Relocatable, SyscallHandlerError> {
        let segment_start = vm.add_memory_segment();
        let segment_end = vm
            .write_arg(&segment_start, &data)
            .map_err(|_| SyscallHandlerError::SegmentationFault)?;
        let sub = segment_end
            .sub(&segment_start.to_owned().into(), vm.get_prime())
            .map_err(|_| SyscallHandlerError::SegmentationFault)?;
        let segment = (segment_start.to_owned(), sub);
        self.read_only_segments.borrow_mut().push(segment);

        Ok(segment_start)
    }

    fn get_block_info(&self) -> &BlockInfo {
        &self.block_info
    }

    fn get_block_number(
        &self,
        vm: &mut VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        self._read_and_validate_syscall_request("get_block_number", vm, syscall_ptr.clone())?;

        self._write_syscall_response(
            &GetBlockNumberResponse::new(self.block_info.block_number),
            vm,
            syscall_ptr,
        )?;
        Ok(())
    }
}

impl Default for BusinessLogicSyscallHandler {
    fn default() -> Self {
        Self::new(BlockInfo::default())
    }
}

#[cfg(test)]
mod tests {
    use crate::bigint;
    use crate::business_logic::execution::objects::{OrderedEvent, OrderedL2ToL1Message};
    use crate::core::errors::syscall_handler_errors::SyscallHandlerError;
    use crate::core::syscalls::business_logic_syscall_handler::BusinessLogicSyscallHandler;
    use crate::core::syscalls::hint_code::{DEPLOY_SYSCALL_CODE, EMIT_EVENT_CODE, GET_TX_INFO};
    use crate::core::syscalls::syscall_handler::*;
    use crate::state::state_api_objects::BlockInfo;
    use crate::utils::test_utils::*;
    use cairo_rs::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::{
        BuiltinHintProcessor, HintProcessorData,
    };
    use cairo_rs::hint_processor::hint_processor_definition::HintProcessor;
    use cairo_rs::relocatable;
    use cairo_rs::types::exec_scope::ExecutionScopes;
    use cairo_rs::types::relocatable::{MaybeRelocatable, Relocatable};
    use cairo_rs::vm::errors::memory_errors::MemoryError;
    use cairo_rs::vm::errors::vm_errors::VirtualMachineError;
    use cairo_rs::vm::errors::vm_errors::VirtualMachineError::UnknownHint;
    use cairo_rs::vm::vm_core::VirtualMachine;
    use num_bigint::{BigInt, Sign};
    use std::any::Any;
    use std::collections::HashMap;

    #[test]
    fn run_alloc_hint_ap_is_not_empty() {
        let hint_code = "memory[ap] = segments.add()";
        let mut vm = vm!();
        //Add 3 segments to the memory
        add_segments!(vm, 3);
        vm.set_ap(6);
        //Insert something into ap
        let key = Relocatable::from((1, 6));
        vm.insert_value(&key, (1, 6)).unwrap();
        //ids and references are not needed for this test
        assert_eq!(
            run_hint!(vm, HashMap::new(), hint_code),
            Err(VirtualMachineError::MemoryError(
                MemoryError::InconsistentMemory(
                    MaybeRelocatable::from((1, 6)),
                    MaybeRelocatable::from((1, 6)),
                    MaybeRelocatable::from((3, 0))
                )
            ))
        );
    }

    // tests that we are executing correctly our syscall hint processor.
    #[test]
    fn cannot_run_syscall_hints() {
        let hint_code = DEPLOY_SYSCALL_CODE;
        let mut vm = vm!();
        assert_eq!(
            run_syscall_hint!(vm, HashMap::new(), hint_code),
            Err(UnknownHint("Hint not implemented".to_string()))
        );
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
        let syscall_handler = SyscallHintProcessor::new_empty().unwrap();
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
            .borrow()
            .get(0)
            .unwrap()
            .clone();

        assert_eq!(
            OrderedEvent::new(
                0,
                Vec::from([bigint!(1), bigint!(1)]),
                Vec::from([bigint!(1), bigint!(1)])
            ),
            event
        );
        assert_eq!(
            syscall_handler
                .syscall_handler
                .tx_execution_context
                .borrow()
                .n_emitted_events,
            1
        );
    }

    #[test]
    fn get_tx_info_test() {
        let mut vm = vm!();
        add_segments!(vm, 3);

        // insert data to form the request
        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)), //  syscall_ptr
                ((2, 0), 1),      //  version
                ((2, 1), 1),      //  account_contract_address
                ((2, 2), 2),      //  max_fee
                ((2, 3), 1),      //  signature_len
                ((2, 4), (3, 0)), //  signature
                ((2, 5), 1),      //  transaction_hash
                ((2, 6), 1),      //  chain_id
                ((2, 7), 1)       //  nonce
            ]
        );

        // syscall_ptr
        let ids_data = ids_data!["syscall_ptr"];

        let hint_data = HintProcessorData::new_default(GET_TX_INFO.to_string(), ids_data);
        // invoke syscall
        let syscall_handler = SyscallHintProcessor::new_empty().unwrap();
        let err = syscall_handler.execute_hint(
            &mut vm,
            &mut ExecutionScopes::new(),
            &any_box!(hint_data),
            &HashMap::new(),
        );

        assert_eq!(
            err,
            Err(VirtualMachineError::UnknownHint(
                "Hint not implemented".to_string()
            ))
        )
    }

    #[test]
    fn deploy_from_zero_error() {
        let mut syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
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
            syscall._deploy(&vm, relocatable!(1, 0)),
            Err(SyscallHandlerError::DeployFromZero(4))
        )
    }

    #[test]
    fn can_allocate_segment() {
        let syscall_handler = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();
        let data = vec![MaybeRelocatable::Int(7.into())];

        let segment_start = syscall_handler.allocate_segment(&mut vm, data).unwrap();
        let expected_value = vm
            .get_integer(&Relocatable::from((0, 0)))
            .unwrap()
            .into_owned();
        assert_eq!(Relocatable::from((0, 0)), segment_start);
        assert_eq!(expected_value, 7.into());
    }

    #[test]
    fn test_send_message_to_l1_ok() {
        let mut syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();

        add_segments!(vm, 3);

        memory_insert!(
            vm,
            [
                ((1, 0), 0),
                ((1, 1), 1),
                ((1, 2), 2),
                ((1, 4), (2, 0)),
                ((2, 0), 18),
                ((2, 1), 12)
            ]
        );

        assert_eq!(syscall.tx_execution_context.borrow().n_sent_messages, 0);
        assert_eq!(syscall.l2_to_l1_messages, Vec::new());

        syscall
            .send_message_to_l1(&vm, relocatable!(1, 0))
            .expect("Failed to send message to L1");

        assert_eq!(syscall.tx_execution_context.borrow().n_sent_messages, 1);
        assert_eq!(
            syscall.l2_to_l1_messages,
            vec![OrderedL2ToL1Message::new(
                syscall.tx_execution_context.borrow().n_sent_messages - 1,
                1,
                vec![bigint!(18), bigint!(12)],
            )]
        );
    }

    #[test]
    fn test_get_caller_address_ok() {
        let syscall = BusinessLogicSyscallHandler::new(BlockInfo::default());
        let mut vm = vm!();

        add_segments!(vm, 2);

        vm.insert_value(&relocatable!(1, 0), bigint!(0)).unwrap();

        assert_eq!(
            syscall._get_caller_address(&vm, relocatable!(1, 0)),
            Ok(syscall.contract_address)
        )
    }
}
