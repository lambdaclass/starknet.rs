use std::cell::RefCell;
use std::rc::Rc;

use super::syscall_request::*;
use crate::business_logic::execution::objects::*;
use crate::core::errors::syscall_handler_errors::SyscallHandlerError;
use crate::core::syscalls::syscall_handler::SyscallHandler;
use crate::utils::*;
use cairo_rs::types::relocatable::{MaybeRelocatable, Relocatable};
use cairo_rs::vm::vm_core::VirtualMachine;
use num_bigint::BigInt;

//* -----------------------------------
//* BusinessLogicHandler implementation
//* -----------------------------------

pub struct BusinessLogicSyscallHandler {
    tx_execution_context: Rc<RefCell<TransactionExecutionContext>>,
    events: Rc<RefCell<Vec<OrderedEvent>>>,
}

impl BusinessLogicSyscallHandler {
    pub fn new() -> Result<Self, SyscallHandlerError> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let tx_execution_context = Rc::new(RefCell::new(TransactionExecutionContext::new()));
        Ok(BusinessLogicSyscallHandler {
            events,
            tx_execution_context,
        })
    }
}

impl SyscallHandler for BusinessLogicSyscallHandler {
    fn emit_event(
        &self,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<(), SyscallHandlerError> {
        let SyscallRequest::EmitEvent(request) =
            self._read_and_validate_syscall_request("emit_event", vm, syscall_ptr)?;

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

    fn send_message_to_l1(&self, _vm: VirtualMachine, _syscall_ptr: Relocatable) {
        todo!()
    }

    fn _get_tx_info_ptr(&self, _vm: VirtualMachine) {
        todo!()
    }
    fn _deploy(&self, _vm: VirtualMachine, _syscall_ptr: Relocatable) -> i32 {
        todo!()
    }

    fn _read_and_validate_syscall_request(
        &self,
        syscall_name: &str,
        vm: &VirtualMachine,
        syscall_ptr: Relocatable,
    ) -> Result<SyscallRequest, SyscallHandlerError> {
        self.read_syscall_request(syscall_name, vm, syscall_ptr)
    }

    fn _call_contract(
        &self,
        _syscall_name: &str,
        _vm: VirtualMachine,
        _syscall_ptr: Relocatable,
    ) -> Vec<i32> {
        todo!()
    }
    fn _get_caller_address(&self, _vm: VirtualMachine, _syscall_ptr: Relocatable) -> i32 {
        todo!()
    }
    fn _get_contract_address(&self, _vm: VirtualMachine, _syscall_ptr: Relocatable) -> i32 {
        todo!()
    }
    fn _storage_read(&self, _address: i32) -> i32 {
        todo!()
    }
    fn _storage_write(&self, _address: i32, _value: i32) {
        todo!()
    }
    fn _allocate_segment(&self, _vm: VirtualMachine, _data: Vec<MaybeRelocatable>) -> Relocatable {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use crate::business_logic::execution::objects::OrderedEvent;
    use crate::core::syscalls::hint_code::{DEPLOY_SYSCALL_CODE, EMIT_EVENT_CODE};
    use crate::core::syscalls::syscall_handler::*;
    use crate::utils::test_utils::*;
    use cairo_rs::hint_processor::builtin_hint_processor::builtin_hint_processor_definition::{
        BuiltinHintProcessor, HintProcessorData,
    };
    use cairo_rs::hint_processor::hint_processor_definition::HintProcessor;
    use cairo_rs::types::exec_scope::ExecutionScopes;
    use cairo_rs::types::relocatable::{MaybeRelocatable, Relocatable};
    use cairo_rs::vm::errors::memory_errors::MemoryError;
    use cairo_rs::vm::errors::vm_errors::VirtualMachineError;
    use cairo_rs::vm::errors::vm_errors::VirtualMachineError::UnknownHint;
    use cairo_rs::vm::vm_core::VirtualMachine;
    use num_bigint::{BigInt, Sign};
    use std::any::Any;
    use std::collections::HashMap;
    use std::str::FromStr;

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

        // insert selector of syscall
        let selector = BigInt::from_str("1280709301550335749748").unwrap();

        // keys_len
        let keys_len = BigInt::from_str("2").unwrap();
        // data_len
        let data_len = BigInt::from_str("2").unwrap();

        // insert keys and data to generate the event
        // keys points to (2,0)
        let key1 = BigInt::from_str("1").unwrap();
        let key2 = BigInt::from_str("1").unwrap();

        // data points to (2,3)
        let data1 = BigInt::from_str("1").unwrap();
        let data2 = BigInt::from_str("1").unwrap();

        memory_insert!(
            vm,
            [
                ((1, 0), (2, 0)),
                ((2, 0), selector),
                ((2, 1), (keys_len)),
                ((2, 2), (3, 0)),
                ((2, 3), data_len),
                ((2, 4), (3, 3)),
                ((3, 0), key1),
                ((3, 1), key2),
                ((3, 3), data1),
                ((3, 4), data2)
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
                Vec::from([
                    BigInt::from_str("1").unwrap(),
                    BigInt::from_str("1").unwrap()
                ]),
                Vec::from([
                    BigInt::from_str("1").unwrap(),
                    BigInt::from_str("1").unwrap()
                ])
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
}
