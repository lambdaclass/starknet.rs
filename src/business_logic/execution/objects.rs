use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
};

use cairo_rs::{
    types::relocatable::{MaybeRelocatable, Relocatable},
    vm::{runners::cairo_runner::ExecutionResources, vm_core::VirtualMachine},
};
use felt::{Felt, NewFelt};
use num_traits::{ToPrimitive, Zero};

use super::execution_errors::ExecutionError;
use crate::{
    business_logic::state::state_cache::StorageEntry,
    core::{
        errors::syscall_handler_errors::SyscallHandlerError, syscalls::syscall_request::FromPtr,
    },
    definitions::{general_config::StarknetChainId, transaction_type::TransactionType},
    utils::{get_big_int, get_integer, get_relocatable, Address},
};
use crate::{services::api::contract_class::EntryPointType, starknet_storage::storage::Storage};

type ResourcesMapping = HashMap<String, Felt>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CallType {
    Call,
    Delegate,
}

// --------------------
// CallInfo structure
// --------------------

#[derive(Debug, Clone, PartialEq)]
pub struct CallInfo {
    pub(crate) caller_address: Address,
    pub(crate) call_type: Option<CallType>,
    pub(crate) contract_address: Address,
    pub(crate) class_hash: Option<Felt>,
    pub(crate) entry_point_selector: Option<usize>,
    pub(crate) entry_point_type: Option<EntryPointType>,
    pub(crate) calldata: VecDeque<Felt>,
    pub(crate) retdata: VecDeque<u64>,
    pub(crate) execution_resources: ExecutionResources,
    pub(crate) events: VecDeque<OrderedEvent>,
    pub(crate) l2_to_l1_messages: VecDeque<OrderedL2ToL1Message>,
    pub(crate) storage_read_values: VecDeque<u64>,
    pub(crate) accesed_storage_keys: VecDeque<[u8; 32]>,
    pub(crate) internal_calls: Vec<CallInfo>,
}

impl CallInfo {
    ///Yields the contract calls in DFS (preorder).
    pub fn gen_call_topology(&self) -> Vec<CallInfo> {
        let mut calls = Vec::new();
        if self.internal_calls.is_empty() {
            calls.push(self.clone())
        } else {
            calls.push(self.clone());
            for call_info in self.internal_calls.clone() {
                calls.extend(call_info.gen_call_topology());
            }
        }
        calls
    }

    /// Returns a list of StarkNet Event objects collected during the execution, sorted by the order
    /// in which they were emitted.
    pub fn get_sorted_events(&self) -> Result<Vec<Event>, ExecutionError> {
        let calls = self.gen_call_topology();
        let n_events = calls.iter().fold(0, |acc, c| acc + c.events.len());

        let mut starknet_events: Vec<Option<Event>> = (0..n_events).map(|_| None).collect();

        for call in calls {
            for ordered_event in call.events {
                let event = Event::new(ordered_event.clone(), call.contract_address.clone());
                starknet_events.remove(ordered_event.order as usize - 1);
                starknet_events.insert(ordered_event.order as usize - 1, Some(event));
            }
        }

        let are_all_some = starknet_events.iter().all(|e| e.is_some());

        if !are_all_some {
            return Err(ExecutionError::UnexpectedHolesInEventOrder);
        }
        Ok(starknet_events.into_iter().flatten().collect())
    }

    /// Returns a list of StarkNet L2ToL1MessageInfo objects collected during the execution, sorted
    /// by the order in which they were sent.
    pub fn get_sorted_l2_to_l1_messages(&self) -> Result<Vec<L2toL1MessageInfo>, ExecutionError> {
        let calls = self.gen_call_topology();
        let n_msgs = calls
            .iter()
            .fold(0, |acc, c| acc + c.l2_to_l1_messages.len());

        let mut starknet_events: Vec<Option<L2toL1MessageInfo>> =
            (0..n_msgs).map(|_| None).collect();

        for call in calls {
            for ordered_msg in call.l2_to_l1_messages {
                let l2tol1msg =
                    L2toL1MessageInfo::new(ordered_msg.clone(), call.caller_address.clone());
                starknet_events.remove(ordered_msg.order as usize - 1);
                starknet_events.insert(ordered_msg.order as usize - 1, Some(l2tol1msg));
            }
        }

        let are_all_some = starknet_events.iter().all(|e| e.is_some());

        if !are_all_some {
            return Err(ExecutionError::UnexpectedHolesL2toL1Messages);
        }
        Ok(starknet_events.into_iter().flatten().collect())
    }

    pub fn get_visited_storage_entries(self) -> HashSet<StorageEntry> {
        let mut storage_entries = self
            .accesed_storage_keys
            .into_iter()
            .map(|key| (self.contract_address.clone(), key))
            .collect::<HashSet<(Address, [u8; 32])>>();

        let internal_visited_storage_entries =
            CallInfo::get_visited_storage_entries_of_many(self.internal_calls);

        storage_entries
            .union(&internal_visited_storage_entries)
            .cloned()
            .collect()
    }

    pub fn get_visited_storage_entries_of_many(calls_info: Vec<CallInfo>) -> HashSet<StorageEntry> {
        calls_info.into_iter().fold(HashSet::new(), |acc, c| {
            acc.union(&c.get_visited_storage_entries())
                .cloned()
                .collect()
        })
    }
}

impl Default for CallInfo {
    fn default() -> Self {
        Self {
            caller_address: Address(0.into()),
            call_type: None,
            contract_address: Address(0.into()),
            class_hash: None,
            internal_calls: Vec::new(),
            entry_point_type: Some(EntryPointType::Constructor),
            storage_read_values: VecDeque::new(),
            retdata: VecDeque::new(),
            entry_point_selector: None,
            l2_to_l1_messages: VecDeque::new(),
            accesed_storage_keys: VecDeque::new(),
            calldata: VecDeque::new(),
            execution_resources: ExecutionResources {
                n_steps: 0,
                n_memory_holes: 0,
                builtin_instance_counter: HashMap::new(),
            },
            events: VecDeque::new(),
        }
    }
}

// -------------------------
//  Events Structures
// -------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrderedEvent {
    order: u64,
    keys: Vec<Felt>,
    data: Vec<Felt>,
}

impl OrderedEvent {
    pub fn new(order: u64, keys: Vec<Felt>, data: Vec<Felt>) -> Self {
        OrderedEvent { order, keys, data }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub from_addres: Address,
    pub keys: Vec<Felt>,
    pub data: Vec<Felt>,
}

impl Event {
    pub fn new(event_content: OrderedEvent, emitting_contract_address: Address) -> Self {
        Event {
            from_addres: emitting_contract_address,
            keys: event_content.keys,
            data: event_content.data,
        }
    }
}

// -------------------------
//  Transaction Structures
// -------------------------

#[derive(Clone)]
pub(crate) struct TransactionExecutionContext {
    pub(crate) n_emitted_events: u64,
    pub(crate) version: usize,
    pub(crate) account_contract_address: Address,
    pub(crate) max_fee: u64,
    pub(crate) transaction_hash: Felt,
    pub(crate) signature: Vec<Felt>,
    pub(crate) nonce: Felt,
    pub(crate) n_sent_messages: usize,
}

impl TransactionExecutionContext {
    pub fn new() -> Self {
        TransactionExecutionContext {
            n_emitted_events: 0,
            account_contract_address: Address(Felt::zero()),
            max_fee: 0,
            nonce: Felt::zero(),
            signature: Vec::new(),
            transaction_hash: Felt::zero(),
            version: 0,
            n_sent_messages: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TxInfoStruct {
    pub(crate) version: usize,
    pub(crate) account_contract_address: Address,
    pub(crate) max_fee: u64,
    pub(crate) signature_len: usize,
    pub(crate) signature: Relocatable,
    pub(crate) transaction_hash: Felt,
    pub(crate) chain_id: Felt,
    pub(crate) nonce: Felt,
}

impl TxInfoStruct {
    pub(crate) fn new(
        tx: TransactionExecutionContext,
        signature: Relocatable,
        chain_id: StarknetChainId,
    ) -> TxInfoStruct {
        TxInfoStruct {
            version: tx.version,
            account_contract_address: tx.account_contract_address,
            max_fee: tx.max_fee,
            signature_len: tx.signature.len(),
            signature,
            transaction_hash: tx.transaction_hash,
            chain_id: chain_id.to_felt(),
            nonce: tx.nonce,
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<MaybeRelocatable> {
        vec![
            MaybeRelocatable::from(Felt::new(self.version)),
            MaybeRelocatable::from(&self.account_contract_address.0),
            MaybeRelocatable::from(Felt::new(self.max_fee)),
            MaybeRelocatable::from(Felt::new(self.signature_len)),
            MaybeRelocatable::from(&self.signature),
            MaybeRelocatable::from(&self.transaction_hash),
            MaybeRelocatable::from(&self.chain_id),
            MaybeRelocatable::from(&self.nonce),
        ]
    }

    pub(crate) fn from_ptr(
        vm: &VirtualMachine,
        tx_info_ptr: Relocatable,
    ) -> Result<TxInfoStruct, SyscallHandlerError> {
        let version = get_integer(vm, &tx_info_ptr)?;

        let account_contract_address = Address(get_big_int(vm, &(&tx_info_ptr + 1))?);
        let max_fee = get_big_int(vm, &(&tx_info_ptr + 2))?
            .to_u64()
            .ok_or(SyscallHandlerError::FeltToU64Fail)?;
        let signature_len = get_integer(vm, &(&tx_info_ptr + 3))?;
        let signature = get_relocatable(vm, &(&tx_info_ptr + 4))?;
        let transaction_hash = get_big_int(vm, &(&tx_info_ptr + 5))?;
        let chain_id = get_big_int(vm, &(&tx_info_ptr + 6))?;
        let nonce = get_big_int(vm, &(&tx_info_ptr + 7))?;

        Ok(TxInfoStruct {
            version,
            account_contract_address,
            max_fee,
            signature_len,
            signature,
            transaction_hash,
            chain_id,
            nonce,
        })
    }
}

#[derive(Debug, PartialEq, Default)]
pub struct TransactionExecutionInfo {
    pub(crate) validate_info: Option<CallInfo>,
    pub(crate) call_info: Option<CallInfo>,
    pub(crate) fee_transfer_info: Option<CallInfo>,
    pub(crate) actual_fee: u64,
    pub(crate) actual_resources: ResourcesMapping,
    pub(crate) tx_type: Option<TransactionType>,
}

impl TransactionExecutionInfo {
    pub fn new(
        validate_info: Option<CallInfo>,
        call_info: Option<CallInfo>,
        fee_transfer_info: Option<CallInfo>,
        actual_fee: u64,
        actual_resources: ResourcesMapping,
        tx_type: Option<TransactionType>,
    ) -> Self {
        TransactionExecutionInfo {
            validate_info,
            call_info,
            fee_transfer_info,
            actual_fee,
            actual_resources,
            tx_type,
        }
    }

    // In deploy account tx, validation will take place after execution of the constructor.
    pub fn non_optional_calls(&self) -> Vec<CallInfo> {
        let calls = match self.tx_type {
            Some(TransactionType::Deploy) => [
                self.call_info.clone(),
                self.validate_info.clone(),
                self.fee_transfer_info.clone(),
            ],
            _ => [
                self.validate_info.clone(),
                self.call_info.clone(),
                self.fee_transfer_info.clone(),
            ],
        };

        calls.into_iter().flatten().collect()
    }

    pub fn get_visited_storage_entries(&self) -> HashSet<StorageEntry> {
        CallInfo::get_visited_storage_entries_of_many(self.non_optional_calls())
    }

    pub fn from_calls_info(
        execute_call_info: Option<CallInfo>,
        tx_type: Option<TransactionType>,
        validate_info: Option<CallInfo>,
        fee_transfer_info: Option<CallInfo>,
    ) -> Self {
        TransactionExecutionInfo {
            validate_info,
            call_info: execute_call_info,
            fee_transfer_info,
            actual_fee: 0,
            actual_resources: HashMap::new(),
            tx_type,
        }
    }

    pub fn empty() -> Self {
        TransactionExecutionInfo {
            validate_info: None,
            call_info: None,
            fee_transfer_info: None,
            actual_fee: 0,
            actual_resources: HashMap::new(),
            tx_type: None,
        }
    }

    pub fn create_concurrent_stage_execution_info(
        validate_info: Option<CallInfo>,
        call_info: Option<CallInfo>,
        actual_resources: ResourcesMapping,
        tx_type: Option<TransactionType>,
    ) -> Self {
        TransactionExecutionInfo {
            validate_info,
            call_info,
            fee_transfer_info: None,
            actual_fee: 0,
            actual_resources,
            tx_type,
        }
    }

    pub fn from_concurrent_state_execution_info(
        concurrent_execution_info: TransactionExecutionInfo,
        actual_fee: u64,
        fee_transfer_info: Option<CallInfo>,
    ) -> Self {
        TransactionExecutionInfo {
            actual_fee,
            fee_transfer_info,
            ..concurrent_execution_info
        }
    }

    pub fn get_visited_storage_entries_of_many(
        execution_infos: Vec<TransactionExecutionInfo>,
    ) -> HashSet<StorageEntry> {
        execution_infos.into_iter().fold(HashSet::new(), |acc, e| {
            acc.union(&e.get_visited_storage_entries())
                .cloned()
                .collect()
        })
    }

    pub fn get_sorted_events(&self) -> Result<Vec<Event>, ExecutionError> {
        let calls = self.non_optional_calls();
        let mut sorted_events: Vec<Event> = Vec::new();

        for call in calls {
            let events = call.get_sorted_events()?;
            sorted_events.extend(events);
        }

        Ok(sorted_events)
    }

    pub fn get_sorted_l2_to_l1_messages(&self) -> Result<Vec<L2toL1MessageInfo>, ExecutionError> {
        let calls = self.non_optional_calls();
        let mut sorted_messages: Vec<L2toL1MessageInfo> = Vec::new();

        for call in calls {
            let messages = call.get_sorted_l2_to_l1_messages()?;
            sorted_messages.extend(messages);
        }

        Ok(sorted_messages)
    }
}

// --------------------
// Messages Structures
// --------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OrderedL2ToL1Message {
    pub(crate) order: usize,
    pub(crate) to_address: Address,
    pub(crate) payload: Vec<Felt>,
}

impl OrderedL2ToL1Message {
    pub fn new(order: usize, to_address: Address, payload: Vec<Felt>) -> Self {
        OrderedL2ToL1Message {
            order,
            to_address,
            payload,
        }
    }
}

impl Default for OrderedL2ToL1Message {
    fn default() -> Self {
        OrderedL2ToL1Message {
            order: 0,
            to_address: Address(0.into()),
            payload: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct L2toL1MessageInfo {
    pub(crate) from_address: Address,
    pub(crate) to_address: Address,
    pub(crate) payload: Vec<Felt>,
}

impl L2toL1MessageInfo {
    pub(crate) fn new(
        message_content: OrderedL2ToL1Message,
        sending_contract_address: Address,
    ) -> Self {
        L2toL1MessageInfo {
            from_address: sending_contract_address,
            to_address: message_content.to_address,
            payload: message_content.payload,
        }
    }
}

// ---------------
//     Tests
// ---------------

#[cfg(test)]
mod tests {

    use std::{collections::VecDeque, ops::Add};

    use crate::{business_logic::execution::objects::CallInfo, utils::Address};

    use super::{
        Event, L2toL1MessageInfo, OrderedEvent, OrderedL2ToL1Message, TransactionExecutionInfo,
    };

    #[test]
    fn non_optional_calls_test() {
        let mut tx_info = TransactionExecutionInfo {
            ..Default::default()
        };
        tx_info.call_info = Some(CallInfo {
            ..Default::default()
        });
        tx_info.validate_info = Some(CallInfo {
            ..Default::default()
        });
        tx_info.fee_transfer_info = None;

        let res = tx_info.non_optional_calls();
        assert_eq!(
            res,
            [
                CallInfo {
                    ..Default::default()
                },
                CallInfo {
                    ..Default::default()
                }
            ]
        );

        tx_info.call_info = None;
        tx_info.validate_info = None;
        tx_info.fee_transfer_info = None;

        let res = tx_info.non_optional_calls();
        assert_eq!(res, [])
    }

    #[test]
    fn gen_call_topology_test() {
        // dfs root
        let mut call_root = CallInfo::default();

        // level 1 children
        let mut child1 = CallInfo::default();
        let mut child2 = CallInfo::default();

        // level 2 children
        let mut child3 = CallInfo::default();
        let mut child4 = CallInfo::default();
        let mut child5 = CallInfo::default();
        let mut child6 = CallInfo::default();

        // Set a contract address to identified them
        call_root.contract_address = Address(0.into());
        child1.contract_address = Address(1.into());
        child2.contract_address = Address(2.into());
        child3.contract_address = Address(3.into());
        child4.contract_address = Address(4.into());
        child5.contract_address = Address(5.into());
        child6.contract_address = Address(6.into());

        // set children
        child1.internal_calls = [child3.clone(), child4.clone()].to_vec();
        child2.internal_calls = [child5.clone(), child6.clone()].to_vec();
        call_root.internal_calls = [child1.clone(), child2.clone()].to_vec();

        // DFS recursibly stores from the root to the leftmost child,
        // then goes to the right child and repeats the procedure.
        // expected result of DFS (pre-order) = [call_root, child1, child3, child4, child2, child5, child6]

        assert_eq!(
            call_root.gen_call_topology(),
            [call_root, child1, child3, child4, child2, child5, child6]
        )
    }

    #[test]
    fn get_ordered_event_test() {
        // root
        let mut call_root = CallInfo::default();

        // level 1 children
        let mut child1 = CallInfo::default();
        let mut child2 = CallInfo::default();

        // orderdered events
        let mut ord_event1 = OrderedEvent::default();
        let mut ord_event2 = OrderedEvent::default();
        let mut ord_event3 = OrderedEvent::default();
        let mut ord_event4 = OrderedEvent::default();

        // set orders
        ord_event1.order = 1;
        ord_event2.order = 2;
        ord_event3.order = 3;
        ord_event4.order = 4;

        // store events
        child1.events = VecDeque::from([ord_event3.clone(), ord_event4.clone()]);
        child2.events = VecDeque::from([ord_event1.clone(), ord_event2.clone()]);

        call_root.internal_calls = [child1.clone(), child2.clone()].to_vec();

        // events

        let event1 = Event::new(ord_event1, child2.caller_address.clone());
        let event2 = Event::new(ord_event2, child2.caller_address);
        let event3 = Event::new(ord_event3, child1.caller_address.clone());
        let event4 = Event::new(ord_event4, child1.caller_address);

        assert_eq!(
            call_root.get_sorted_events().unwrap(),
            [event1, event2, event3, event4]
        )
    }

    #[test]
    fn get_ordered_event_test_fail() {
        // root
        let mut call_root = CallInfo::default();

        // level 1 children
        let mut child1 = CallInfo::default();
        let mut child2 = CallInfo::default();

        // orderdered events
        let mut ord_event1 = OrderedEvent::default();
        let mut ord_event2 = OrderedEvent::default();
        let mut ord_event3 = OrderedEvent::default();
        let mut ord_event4 = OrderedEvent::default();

        // set orders
        ord_event1.order = 1;
        ord_event2.order = 2;
        ord_event3.order = 3;
        ord_event4.order = 3;

        // store events
        child1.events = VecDeque::from([ord_event3.clone(), ord_event4.clone()]);
        child2.events = VecDeque::from([ord_event1.clone(), ord_event2.clone()]);

        call_root.internal_calls = [child1.clone(), child2.clone()].to_vec();

        // events

        let event1 = Event::new(ord_event1, child2.caller_address.clone());
        let event2 = Event::new(ord_event2, child2.caller_address);
        let event3 = Event::new(ord_event3, child1.caller_address.clone());
        let event4 = Event::new(ord_event4, child1.caller_address);

        assert!(call_root.get_sorted_events().is_err())
    }

    #[test]
    fn get_ordered_messages_test() {
        // root
        let mut call_root = CallInfo::default();

        // level 1 children
        let mut child1 = CallInfo::default();
        let mut child2 = CallInfo::default();

        // orderdered events
        let mut ord_msg1 = OrderedL2ToL1Message::default();
        let mut ord_msg2 = OrderedL2ToL1Message::default();
        let mut ord_msg3 = OrderedL2ToL1Message::default();
        let mut ord_msg4 = OrderedL2ToL1Message::default();

        // set orders
        ord_msg1.order = 1;
        ord_msg2.order = 2;
        ord_msg3.order = 3;
        ord_msg4.order = 4;

        // store events
        child1.l2_to_l1_messages = VecDeque::from([ord_msg3.clone(), ord_msg4.clone()]);
        child2.l2_to_l1_messages = VecDeque::from([ord_msg1.clone(), ord_msg2.clone()]);

        call_root.internal_calls = [child1.clone(), child2.clone()].to_vec();

        // events

        let msg1 = L2toL1MessageInfo::new(ord_msg1, child2.caller_address.clone());
        let msg2 = L2toL1MessageInfo::new(ord_msg2, child2.caller_address);
        let msg3 = L2toL1MessageInfo::new(ord_msg3, child1.caller_address.clone());
        let msg4 = L2toL1MessageInfo::new(ord_msg4, child1.caller_address);

        assert_eq!(
            call_root.get_sorted_l2_to_l1_messages().unwrap(),
            [msg1, msg2, msg3, msg4]
        )
    }

    #[test]
    fn get_ordered_messages_test_fail() {
        // root
        let mut call_root = CallInfo::default();

        // level 1 children
        let mut child1 = CallInfo::default();
        let mut child2 = CallInfo::default();

        // orderdered events
        let mut ord_msg1 = OrderedL2ToL1Message::default();
        let mut ord_msg2 = OrderedL2ToL1Message::default();
        let mut ord_msg3 = OrderedL2ToL1Message::default();
        let mut ord_msg4 = OrderedL2ToL1Message::default();

        // set orders
        ord_msg1.order = 1;
        ord_msg2.order = 2;
        ord_msg3.order = 3;
        ord_msg4.order = 3;

        // store events
        child1.l2_to_l1_messages = VecDeque::from([ord_msg3.clone(), ord_msg4.clone()]);
        child2.l2_to_l1_messages = VecDeque::from([ord_msg1.clone(), ord_msg2.clone()]);

        call_root.internal_calls = [child1.clone(), child2.clone()].to_vec();

        // events

        let msg1 = L2toL1MessageInfo::new(ord_msg1, child2.caller_address.clone());
        let msg2 = L2toL1MessageInfo::new(ord_msg2, child2.caller_address);
        let msg3 = L2toL1MessageInfo::new(ord_msg3, child1.caller_address.clone());
        let msg4 = L2toL1MessageInfo::new(ord_msg4, child1.caller_address);

        assert!(call_root.get_sorted_l2_to_l1_messages().is_err())
    }
}
