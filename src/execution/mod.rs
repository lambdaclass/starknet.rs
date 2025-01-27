pub mod execution_entry_point;
pub mod gas_usage;
pub mod os_usage;
use crate::services::api::contract_classes::deprecated_contract_class::EntryPointType;
use crate::transaction::VersionSpecificAccountTxFields;
use crate::utils::parse_felt_array;
use crate::{
    definitions::{constants::CONSTRUCTOR_ENTRY_POINT_SELECTOR, transaction_type::TransactionType},
    state::state_cache::StorageEntry,
    syscalls::syscall_handler_errors::SyscallHandlerError,
    transaction::{error::TransactionError, Address, ClassHash},
    utils::{get_big_int, get_integer, get_relocatable},
};
use cairo_vm::Felt252;
use cairo_vm::{
    types::relocatable::{MaybeRelocatable, Relocatable},
    vm::{runners::cairo_runner::ExecutionResources, vm_core::VirtualMachine},
};
use getset::Getters;
use num_traits::ToPrimitive;
use serde::{Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallType {
    Call,
    Delegate,
}

// --------------------
// CallInfo structure
// --------------------

#[derive(Debug, Clone, PartialEq)]
pub struct CallInfo {
    pub caller_address: Address,
    pub call_type: Option<CallType>,
    pub contract_address: Address,
    pub code_address: Option<Address>,
    pub class_hash: Option<ClassHash>,
    pub entry_point_selector: Option<Felt252>,
    pub entry_point_type: Option<EntryPointType>,
    pub calldata: Vec<Felt252>,
    pub retdata: Vec<Felt252>,
    pub execution_resources: Option<ExecutionResources>,
    pub events: Vec<OrderedEvent>,
    pub l2_to_l1_messages: Vec<OrderedL2ToL1Message>,
    pub storage_read_values: Vec<Felt252>,
    pub accessed_storage_keys: HashSet<ClassHash>,
    pub internal_calls: Vec<CallInfo>,
    pub gas_consumed: u128,
    pub failure_flag: bool,
}

impl CallInfo {
    pub fn empty(
        contract_address: Address,
        caller_address: Address,
        class_hash: Option<ClassHash>,
        call_type: Option<CallType>,
        entry_point_type: Option<EntryPointType>,
        entry_point_selector: Option<Felt252>,
        code_address: Option<Address>,
    ) -> Self {
        CallInfo {
            caller_address,
            call_type,
            contract_address,
            class_hash,
            entry_point_selector,
            code_address,
            entry_point_type,
            calldata: Vec::new(),
            retdata: Vec::new(),
            execution_resources: Some(ExecutionResources {
                n_steps: 0,
                builtin_instance_counter: HashMap::new(),
                n_memory_holes: 0,
            }),
            events: Vec::new(),
            l2_to_l1_messages: Vec::new(),
            storage_read_values: Vec::new(),
            accessed_storage_keys: HashSet::new(),
            internal_calls: Vec::new(),
            gas_consumed: 0,
            failure_flag: false,
        }
    }

    pub fn empty_constructor_call(
        contract_address: Address,
        caller_address: Address,
        class_hash: Option<ClassHash>,
    ) -> Self {
        CallInfo::empty(
            contract_address,
            caller_address,
            class_hash,
            Some(CallType::Call),
            Some(EntryPointType::Constructor),
            Some(*CONSTRUCTOR_ENTRY_POINT_SELECTOR),
            None,
        )
    }

    /// Returns the contract calls in DFS (preorder).
    pub fn gen_call_topology(&self) -> Vec<CallInfo> {
        let mut calls = Vec::new();
        // add the current call
        calls.push(self.clone());

        // if it has internal calls we need to add them too.
        if !self.internal_calls.is_empty() {
            for inner_call in self.internal_calls.clone() {
                calls.extend(inner_call.gen_call_topology());
            }
        }

        calls
    }

    /// Returns a list of [`Event`] objects collected during the execution, sorted by the order
    /// in which they were emitted.
    pub fn get_sorted_events(&self) -> Result<Vec<Event>, TransactionError> {
        // collect a vector of the full call topology (all the internal
        // calls performed during the current call)
        let calls = self.gen_call_topology();
        let mut collected_events = Vec::new();

        // for each call, collect its ordered events
        for c in calls {
            collected_events.extend(
                c.events
                    .iter()
                    .map(|oe| (oe.clone(), c.contract_address.clone())),
            );
        }
        // sort the collected events using the ordering given by the order
        collected_events.sort_by_key(|(oe, _)| oe.order);

        // check that there is no holes.
        // since it is already sorted, we only need to check for continuity
        let mut i = 0;
        for (oe, _) in collected_events.iter() {
            if i == oe.order {
                continue;
            }
            i += 1;
            if i != oe.order {
                return Err(TransactionError::UnexpectedHolesInEventOrder);
            }
        }

        // now that it is ordered and without holes, we can discard the order and
        // convert each [`OrderedEvent`] to the underlying [`Event`].
        let collected_events = collected_events
            .into_iter()
            .map(|(oe, ca)| Event::new(oe, ca))
            .collect();
        Ok(collected_events)
    }

    /// Returns a list of L2ToL1MessageInfo objects collected during the execution, sorted
    /// by the order in which they were sent.
    pub fn get_sorted_l2_to_l1_messages(&self) -> Result<Vec<L2toL1MessageInfo>, TransactionError> {
        let calls = self.gen_call_topology();
        let n_msgs = calls
            .iter()
            .fold(0, |acc, c| acc + c.l2_to_l1_messages.len());

        let mut starknet_events: Vec<Option<L2toL1MessageInfo>> =
            (0..n_msgs).map(|_| None).collect();

        for call in calls {
            for ordered_msg in call.l2_to_l1_messages {
                let l2tol1msg =
                    L2toL1MessageInfo::new(ordered_msg.clone(), call.contract_address.clone());
                starknet_events.remove(ordered_msg.order);
                starknet_events.insert(ordered_msg.order, Some(l2tol1msg));
            }
        }

        let are_all_some = starknet_events.iter().all(|e| e.is_some());

        if !are_all_some {
            return Err(TransactionError::UnexpectedHolesL2toL1Messages);
        }
        Ok(starknet_events.into_iter().flatten().collect())
    }

    pub fn get_visited_storage_entries(self) -> HashSet<StorageEntry> {
        let storage_entries = self
            .accessed_storage_keys
            .into_iter()
            .map(|key| (self.contract_address.clone(), key.0))
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

    pub fn result(&self) -> CallResult {
        CallResult {
            gas_consumed: self.gas_consumed,
            is_success: !self.failure_flag,
            retdata: self.retdata.iter().map(|f| f.into()).collect(),
        }
    }
}

impl Default for CallInfo {
    fn default() -> Self {
        Self {
            caller_address: Address(0.into()),
            call_type: None,
            contract_address: Address(0.into()),
            code_address: None,
            class_hash: Some(ClassHash::default()),
            internal_calls: Vec::new(),
            entry_point_type: Some(EntryPointType::Constructor),
            storage_read_values: Vec::new(),
            retdata: Vec::new(),
            entry_point_selector: None,
            l2_to_l1_messages: Vec::new(),
            accessed_storage_keys: HashSet::new(),
            calldata: Vec::new(),
            execution_resources: Some(ExecutionResources {
                n_steps: 0,
                n_memory_holes: 0,
                builtin_instance_counter: HashMap::new(),
            }),
            events: Vec::new(),
            gas_consumed: 0,
            failure_flag: false,
        }
    }
}

impl<'de> Deserialize<'de> for CallInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: serde_json::Value = Deserialize::deserialize(deserializer)?;

        // Parse execution_resources
        let execution_resources_value = value["execution_resources"].clone();

        let execution_resources = ExecutionResources {
            n_steps: serde_json::from_value(execution_resources_value["n_steps"].clone())
                .map_err(serde::de::Error::custom)?,
            n_memory_holes: serde_json::from_value(
                execution_resources_value["n_memory_holes"].clone(),
            )
            .map_err(serde::de::Error::custom)?,
            builtin_instance_counter: serde_json::from_value(
                execution_resources_value["builtin_instance_counter"].clone(),
            )
            .map_err(serde::de::Error::custom)?,
        };

        // Parse retdata
        let retdata_value = value["result"].clone();
        let retdata = parse_felt_array(retdata_value.as_array().unwrap());

        // Parse calldata
        let calldata_value = value["calldata"].clone();
        let calldata = parse_felt_array(calldata_value.as_array().unwrap());

        // Parse internal calls
        let internal_calls_value = value["internal_calls"].clone();
        let mut internal_calls = vec![];

        for call in internal_calls_value.as_array().unwrap() {
            internal_calls
                .push(serde_json::from_value(call.clone()).map_err(serde::de::Error::custom)?);
        }

        Ok(CallInfo {
            execution_resources: Some(execution_resources),
            retdata,
            calldata,
            internal_calls,
            ..Default::default()
        })
    }
}

// ----------------------
// CallResult structure
// ----------------------

#[derive(Debug)]
pub struct CallResult {
    pub gas_consumed: u128,
    pub is_success: bool,
    pub retdata: Vec<MaybeRelocatable>,
}

impl From<CallInfo> for CallResult {
    fn from(info: CallInfo) -> Self {
        Self {
            gas_consumed: 0,
            is_success: true,
            retdata: info.retdata.into_iter().map(Into::into).collect(),
        }
    }
}

// -------------------------
//  Events Structures
// -------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrderedEvent {
    pub order: u64,
    pub keys: Vec<Felt252>,
    pub data: Vec<Felt252>,
}

impl OrderedEvent {
    pub fn new(order: u64, keys: Vec<Felt252>, data: Vec<Felt252>) -> Self {
        OrderedEvent { order, keys, data }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub from_address: Address,
    pub keys: Vec<Felt252>,
    pub data: Vec<Felt252>,
}

impl Event {
    pub fn new(event_content: OrderedEvent, emitting_contract_address: Address) -> Self {
        Event {
            from_address: emitting_contract_address,
            keys: event_content.keys,
            data: event_content.data,
        }
    }
}

// -------------------------
//  Transaction Structures
// -------------------------

#[derive(Clone, Debug, Default, Getters)]
pub struct TransactionExecutionContext {
    pub(crate) n_emitted_events: u64,
    pub(crate) version: Felt252,
    pub(crate) account_contract_address: Address,
    pub(crate) account_tx_fields: VersionSpecificAccountTxFields,
    pub(crate) transaction_hash: Felt252,
    pub(crate) signature: Vec<Felt252>,
    #[get = "pub"]
    pub(crate) nonce: Felt252,
    pub(crate) n_sent_messages: usize,
    pub(crate) _n_steps: u64,
    // pub(crate) use_cairo_native: bool,
}

impl TransactionExecutionContext {
    pub fn new(
        account_contract_address: Address,
        transaction_hash: Felt252,
        signature: Vec<Felt252>,
        account_tx_fields: VersionSpecificAccountTxFields,
        nonce: Felt252,
        _n_steps: u64,
        version: Felt252,
    ) -> Self {
        let nonce = if version == 0.into() {
            Felt252::ZERO
        } else {
            nonce
        };

        TransactionExecutionContext {
            n_emitted_events: 0,
            account_contract_address,
            account_tx_fields,
            nonce,
            signature,
            transaction_hash,
            version,
            n_sent_messages: 0,
            _n_steps,
        }
    }

    pub fn create_for_testing(
        account_contract_address: Address,
        nonce: Felt252,
        _n_steps: u64,
        version: Felt252,
    ) -> Self {
        TransactionExecutionContext {
            version,
            account_contract_address,
            nonce,
            _n_steps,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TxInfoStruct {
    pub(crate) version: Felt252,
    pub(crate) account_contract_address: Address,
    pub(crate) max_fee: u128,
    pub(crate) signature_len: usize,
    pub(crate) signature: Relocatable,
    pub(crate) transaction_hash: Felt252,
    pub(crate) chain_id: Felt252,
    pub(crate) nonce: Felt252,
}

impl TxInfoStruct {
    pub(crate) fn new(
        tx: TransactionExecutionContext,
        signature: Relocatable,
        chain_id: Felt252,
    ) -> TxInfoStruct {
        TxInfoStruct {
            version: tx.version,
            account_contract_address: tx.account_contract_address,
            max_fee: tx.account_tx_fields.max_fee(),
            signature_len: tx.signature.len(),
            signature,
            transaction_hash: tx.transaction_hash,
            chain_id,
            nonce: tx.nonce,
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<MaybeRelocatable> {
        vec![
            MaybeRelocatable::from(&self.version),
            MaybeRelocatable::from(&self.account_contract_address.0),
            MaybeRelocatable::from(Felt252::from(self.max_fee)),
            MaybeRelocatable::from(Felt252::from(self.signature_len)),
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
        let version = get_big_int(vm, tx_info_ptr)?;

        let account_contract_address = Address(get_big_int(vm, (tx_info_ptr + 1)?)?);
        let max_fee = get_big_int(vm, (tx_info_ptr + 2)?)?.to_u128().ok_or(
            SyscallHandlerError::Conversion("Felt252".to_string(), "u128".to_string()),
        )?;
        let signature_len = get_integer(vm, (tx_info_ptr + 3)?)?;
        let signature = get_relocatable(vm, (tx_info_ptr + 4)?)?;
        let transaction_hash = get_big_int(vm, (tx_info_ptr + 5)?)?;
        let chain_id = get_big_int(vm, (tx_info_ptr + 6)?)?;
        let nonce = get_big_int(vm, (tx_info_ptr + 7)?)?;

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransactionExecutionInfo {
    pub validate_info: Option<CallInfo>,
    pub call_info: Option<CallInfo>,
    pub revert_error: Option<String>,
    pub fee_transfer_info: Option<CallInfo>,
    pub actual_fee: u128,
    pub actual_resources: HashMap<String, usize>,
    pub tx_type: Option<TransactionType>,
}

impl TransactionExecutionInfo {
    pub const fn new(
        validate_info: Option<CallInfo>,
        call_info: Option<CallInfo>,
        revert_error: Option<String>,
        fee_transfer_info: Option<CallInfo>,
        actual_fee: u128,
        actual_resources: HashMap<String, usize>,
        tx_type: Option<TransactionType>,
    ) -> Self {
        TransactionExecutionInfo {
            validate_info,
            call_info,
            revert_error,
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
            revert_error: None,
            fee_transfer_info,
            actual_fee: 0,
            actual_resources: HashMap::new(),
            tx_type,
        }
    }

    pub const fn new_without_fee_info(
        validate_info: Option<CallInfo>,
        call_info: Option<CallInfo>,
        revert_error: Option<String>,
        actual_resources: HashMap<String, usize>,
        tx_type: Option<TransactionType>,
    ) -> Self {
        TransactionExecutionInfo {
            validate_info,
            call_info,
            revert_error,
            fee_transfer_info: None,
            actual_fee: 0,
            actual_resources,
            tx_type,
        }
    }

    pub fn set_fee_info(&mut self, actual_fee: u128, fee_transfer_call_info: Option<CallInfo>) {
        self.actual_fee = actual_fee;
        self.fee_transfer_info = fee_transfer_call_info;
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

    /// Returns an ordered vector with all the event emitted during the transaction.
    /// Including the ones emitted by internal calls.
    pub fn get_sorted_events(&self) -> Result<Vec<Event>, TransactionError> {
        let calls = self.non_optional_calls();
        let mut sorted_events: Vec<Event> = Vec::new();

        for call in calls {
            let events = call.get_sorted_events()?;
            sorted_events.extend(events);
        }

        Ok(sorted_events)
    }

    pub fn get_sorted_l2_to_l1_messages(&self) -> Result<Vec<L2toL1MessageInfo>, TransactionError> {
        let calls = self.non_optional_calls();
        let mut sorted_messages: Vec<L2toL1MessageInfo> = Vec::new();

        for call in calls {
            let messages = call.get_sorted_l2_to_l1_messages()?;
            sorted_messages.extend(messages);
        }

        Ok(sorted_messages)
    }

    pub fn to_revert_error(self, revert_error: &str) -> Self {
        TransactionExecutionInfo {
            validate_info: None,
            call_info: None,
            revert_error: Some(revert_error.to_string()),
            fee_transfer_info: None,
            ..self
        }
    }
}

// --------------------
// Messages Structures
// --------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderedL2ToL1Message {
    pub order: usize,
    pub to_address: Address,
    pub payload: Vec<Felt252>,
}

impl OrderedL2ToL1Message {
    pub fn new(order: usize, to_address: Address, payload: Vec<Felt252>) -> Self {
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
    pub from_address: Address,
    pub to_address: Address,
    pub payload: Vec<Felt252>,
}

impl L2toL1MessageInfo {
    pub fn new(message_content: OrderedL2ToL1Message, sending_contract_address: Address) -> Self {
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
    use super::*;
    use crate::utils::string_to_hash;

    #[test]
    fn test_get_sorted_single_event() {
        let address = Address(Felt252::ZERO);
        let ordered_event = OrderedEvent::new(0, vec![], vec![]);
        let event = Event::new(ordered_event.clone(), address.clone());
        let internal_calls = vec![CallInfo {
            events: vec![ordered_event],
            ..Default::default()
        }];
        let call_info = CallInfo {
            contract_address: address,
            internal_calls,
            ..Default::default()
        };

        let sorted_events = call_info.get_sorted_events().unwrap();

        assert_eq!(sorted_events, vec![event]);
    }

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
        child1.events = Vec::from([ord_event3.clone(), ord_event4.clone()]);
        child2.events = Vec::from([ord_event1.clone(), ord_event2.clone()]);

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
        child1.events = Vec::from([ord_event3, ord_event4]);
        child2.events = Vec::from([ord_event1, ord_event2]);

        call_root.internal_calls = [child1, child2].to_vec();

        assert!(call_root.get_sorted_events().is_ok())
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
        ord_msg1.order = 0;
        ord_msg2.order = 1;
        ord_msg3.order = 2;
        ord_msg4.order = 3;

        // store events
        child1.l2_to_l1_messages = Vec::from([ord_msg3.clone(), ord_msg4.clone()]);
        child2.l2_to_l1_messages = Vec::from([ord_msg1.clone(), ord_msg2.clone()]);

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
        ord_msg1.order = 0;
        ord_msg2.order = 1;
        ord_msg3.order = 2;
        ord_msg4.order = 2;

        // store events
        child1.l2_to_l1_messages = Vec::from([ord_msg3, ord_msg4]);
        child2.l2_to_l1_messages = Vec::from([ord_msg1, ord_msg2]);

        call_root.internal_calls = [child1, child2].to_vec();

        assert!(call_root.get_sorted_l2_to_l1_messages().is_err())
    }

    #[test]
    fn callinfo_get_visited_storage_entries_test() {
        // root
        let mut call_root = CallInfo::default();
        let addr1 = Address(1.into());
        let addr2 = Address(2.into());

        // level 1 children
        let mut child1 = CallInfo {
            contract_address: addr1.clone(),
            ..Default::default()
        };
        let mut child2 = CallInfo {
            contract_address: addr2.clone(),
            ..Default::default()
        };

        let hash1 = ClassHash::default();
        let hash2 = ClassHash::default();
        let hash3 = ClassHash::default();
        let hash4 = string_to_hash("0x3");

        child1.accessed_storage_keys = HashSet::new();
        child1.accessed_storage_keys.insert(hash1);
        child1.accessed_storage_keys.insert(hash2);
        child2.accessed_storage_keys = HashSet::new();
        child2.accessed_storage_keys.insert(hash3);
        child2.accessed_storage_keys.insert(hash4);

        call_root.internal_calls = [child1.clone(), child2.clone()].to_vec();

        assert_eq!(
            call_root.get_visited_storage_entries(),
            HashSet::from([(addr1, hash1.0), (addr2.clone(), hash3.0), (addr2, hash4.0)])
        )
    }

    #[test]
    fn txexecinfo_from_calls_info() {
        let callinfo = CallInfo {
            contract_address: Address(2.into()),
            ..Default::default()
        };

        let txexecinfo =
            TransactionExecutionInfo::from_calls_info(Some(callinfo), None, None, None);
        assert_eq!(
            txexecinfo.call_info.unwrap().contract_address,
            Address(2.into())
        );
    }

    #[test]
    fn txexecinfo_get_visited_storage_entries_test() {
        let mut call_info = CallInfo::default();
        let addr1 = Address(1.into());
        let addr2 = Address(2.into());

        // level 1 children
        let mut validate_info = CallInfo {
            contract_address: addr1.clone(),
            ..Default::default()
        };
        let mut fee_transfer_info = CallInfo {
            contract_address: addr2.clone(),
            ..Default::default()
        };

        let hash1 = string_to_hash("0x0");
        let hash2 = string_to_hash("0x1");
        let hash3 = string_to_hash("0x2");
        let hash4 = string_to_hash("0x3");

        validate_info.accessed_storage_keys = HashSet::new();
        validate_info.accessed_storage_keys.insert(hash1);
        validate_info.accessed_storage_keys.insert(hash2);
        fee_transfer_info.accessed_storage_keys = HashSet::new();
        fee_transfer_info.accessed_storage_keys.insert(hash3);
        fee_transfer_info.accessed_storage_keys.insert(hash4);

        let hash5 = string_to_hash("0x5");
        call_info.accessed_storage_keys.insert(hash5);

        let txexecinfo = TransactionExecutionInfo::from_calls_info(
            Some(call_info),
            Some(TransactionType::Deploy),
            Some(validate_info),
            Some(fee_transfer_info),
        );

        assert_eq!(
            txexecinfo.get_visited_storage_entries(),
            HashSet::from([
                (addr1.clone(), hash1.0),
                (addr1, hash2.0),
                (addr2.clone(), hash3.0),
                (addr2, hash4.0),
                (Address(0.into()), hash5.0)
            ])
        )
    }
}
