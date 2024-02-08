use crate::definitions::constants::*;
use crate::execution::L2toL1MessageInfo;
use crate::services::eth_definitions::eth_gas_constants::*;
use crate::state::state_api::StateChangesCount;

/// Estimates L1 gas usage by Starknet's update state and the verifier
///
/// For information about the fee calculation visit the [starknet documentation](https://docs.starknet.io/documentation/architecture_and_concepts/Fees/fee-mechanism/).
///
/// # Parameters:
/// - `l2_to_l1_messages`: A vector of [`L2toL1MessageInfo`] objects representing the messages from L2 to L1.
/// - `n_modified_contracts`: The number of contracts modified by the transaction.
/// - `n_storage_changes`: The number of storage changes made by the transaction.
/// - `l1_handler_payload_size`: The payload size of the L1 to L2 message if and only if the gas usage is being
/// calculated for an InvokeFunction of type L1 handler. Otherwise, it should be `None`.
/// - `n_deployments`: The number of contracts deployed by the transaction.
///
/// # Returns:
///
/// The estimation of L1 gas usage as a `usize` value.
pub fn calculate_tx_gas_usage(
    l2_to_l1_messages: Vec<L2toL1MessageInfo>,
    state_changes: &StateChangesCount,
    l1_handler_payload_size: Option<usize>,
) -> usize {
    let residual_message_segment_length =
        get_message_segment_lenght(&l2_to_l1_messages, l1_handler_payload_size);

    let residual_onchain_data_segment_length = get_onchain_data_segment_length(state_changes);

    let n_l2_to_l1_messages = l2_to_l1_messages.len();
    let n_l1_to_l2_messages = match l1_handler_payload_size {
        Some(_size) => 1,
        None => 0,
    };

    let starknet_gas_usage = starknet_gas_usage(
        residual_message_segment_length,
        n_l2_to_l1_messages,
        n_l1_to_l2_messages,
        l1_handler_payload_size,
        &l2_to_l1_messages,
    );

    let sharp_gas_usage = (residual_message_segment_length * SHARP_GAS_PER_MEMORY_WORD)
        + (residual_onchain_data_segment_length * SHARP_GAS_PER_MEMORY_WORD);

    starknet_gas_usage + sharp_gas_usage
}

// ~~~~~~~~~~~~~~~~
// Helper function
// ~~~~~~~~~~~~~~~~

fn starknet_gas_usage(
    residual_msg_lenght: usize,
    l2_to_l1_msg_len: usize,
    l1_to_l2_msg_len: usize,
    l1_handler: Option<usize>,
    l2_to_l1_messages: &[L2toL1MessageInfo],
) -> usize {
    let l2_emissions_cost = get_consumed_message_to_l2_emissions_cost(l1_handler);
    let l1_log_emissions_cost = get_log_message_to_l1_emissions_cost(l2_to_l1_messages);

    (residual_msg_lenght * GAS_PER_MEMORY_WORD)
        + (l2_to_l1_msg_len * GAS_PER_ZERO_TO_NONZERO_STORAGE_SET)
        + (l1_to_l2_msg_len * GAS_PER_COUNTER_DECREASE)
        + l2_emissions_cost
        + l1_log_emissions_cost
}

/// Calculates the amount of `felt252` added to the output message's segment by the given messages.
///
/// # Parameters:
/// - `l2_to_l1_messages`: A slice of [`L2toL1MessageInfo`] objects representing the messages from L2 to L1.
/// - `l1_handler_payload_size`: The payload size of the L1 to L2 message if and only if the gas usage is being
/// calculated for an `InvokeFunction` of type L1 handler. Otherwise, it should be `None`.
///
/// # Returns:
/// The length of the message segment.
pub fn get_message_segment_lenght(
    l2_to_l1_messages: &[L2toL1MessageInfo],
    l1_handler_payload_size: Option<usize>,
) -> usize {
    let message_segment_length = l2_to_l1_messages.iter().fold(0, |acc, msg| {
        acc + L2_TO_L1_MSG_HEADER_SIZE + msg.payload.len()
    });

    match l1_handler_payload_size {
        Some(size) => message_segment_length + L1_TO_L2_MSG_HEADER_SIZE + size,
        None => message_segment_length,
    }
}

/// Calculates the amount of `felt252` added to the output message's segment by the given operations.
pub const fn get_onchain_data_segment_length(state_changes: &StateChangesCount) -> usize {
    // For each newly modified contract:
    // contract address (1 word).
    // + 1 word with the following info: A flag indicating whether the class hash was updated, the
    // number of entry updates, and the new nonce.
    state_changes.n_modified_contracts * 2
    // For each class updated (through a deploy or a class replacement).
        + state_changes.n_class_hash_updates * CLASS_UPDATE_SIZE
        // For each modified storage cell: key, new value.
        + state_changes.n_storage_updates * 2
         // For each compiled class updated (through declare): class_hash, compiled_class_hash
        + state_changes.n_compiled_class_hash_updates * 2
}

pub fn get_onchain_data_cost(state_changes: &StateChangesCount) -> usize {
    let onchain_data_segment_length = get_onchain_data_segment_length(state_changes);
    let naive_cost = onchain_data_segment_length * SHARP_GAS_PER_DA_WORD;

    let mut discount = state_changes.n_modified_contracts * MODIFIED_CONTRACT_DISCOUNT;
    discount += GAS_PER_MEMORY_WORD - FEE_BALANCE_VALUE_COST;
    naive_cost.saturating_sub(discount)
}

/// Calculates the cost of ConsumedMessageToL2 event emissions caused by an L1 handler with the given
/// payload size.
///
/// # Parameters:
/// - `l1_handler_payload_size`: The payload size of the L1 to L2 message if and only if the gas usage is being
/// calculated for an InvokeFunction of type L1 handler. Otherwise, it should be `None`.
///
/// # Returns:
///
/// The cost of ConsumedMessageToL2 event emissions.
pub fn get_consumed_message_to_l2_emissions_cost(l1_handler: Option<usize>) -> usize {
    match l1_handler {
        None => 0,
        Some(size) => get_event_emission_cost(
            CONSUMED_MSG_TO_L2_N_TOPICS,
            size + CONSUMED_MSG_TO_L2_ENCODED_DATA_SIZE,
        ),
    }
}

/// Calculates the cost of LogMessageToL1 event emissions caused by the given messages.
///
/// # Parameters:
/// - `l2_to_l1_messages`: A slice of [`L2toL1MessageInfo`] objects representing the messages from L2 to L1.
///
/// # Returns:
///
/// The cost of LogMessageToL1 event emissions.
pub fn get_log_message_to_l1_emissions_cost(l2_to_l1_messages: &[L2toL1MessageInfo]) -> usize {
    l2_to_l1_messages.iter().fold(0, |acc, msg| {
        acc + get_event_emission_cost(
            LOG_MSG_TO_L1_N_TOPICS,
            LOG_MSG_TO_L1_ENCODED_DATA_SIZE + msg.payload.len(),
        )
    })
}

/// Calculates the cost of event emissions.
///
/// # Parameters:
/// - `topics`: The number of topics in the event.
/// - `l1_handler_payload_size`: The payload size of the L1 to L2 message.
///
/// # Returns:
///
/// The cost of event emissions.
pub const fn get_event_emission_cost(topics: usize, l1_handler_payload_size: usize) -> usize {
    GAS_PER_LOG
        + (topics + N_DEFAULT_TOPICS) * GAS_PER_LOG_TOPIC
        + l1_handler_payload_size * GAS_PER_LOG_DATA_WORD
}

#[cfg(test)]
mod test {
    use super::get_event_emission_cost;
    use super::*;
    use crate::{execution::OrderedL2ToL1Message, transaction::Address};
    use coverage_helper::test;

    #[test]
    fn test_event_emission_cost() {
        let topics = 40;
        let l1_handler_payload_size = 9;
        // GAS_PER_LOG = 375
        // N_DEFAULT_TOPICS = 1
        // GAS_PER_LOG_TOPIC = 375
        // GAS_PER_LOG_DATA_WORD = 8 * 32
        assert_eq!(
            18054,
            get_event_emission_cost(topics, l1_handler_payload_size)
        );
    }

    #[test]
    fn log_messages_cost_test() {
        let ord_ev1 = OrderedL2ToL1Message::new(1, Address(1235.into()), vec![4.into()]);
        let ord_ev2 = OrderedL2ToL1Message::new(2, Address(35.into()), vec![5.into(), 6.into()]);
        let message1 = L2toL1MessageInfo::new(ord_ev1, Address(1234.into()));
        let message2 = L2toL1MessageInfo::new(ord_ev2, Address(1235.into()));

        // LOG_MSG_TO_L1_N_TOPICS = 2
        // LOG_MSG_TO_L1_ENCODED_DATA_SIZE = 2
        assert_eq!(
            get_log_message_to_l1_emissions_cost(&[message1, message2]),
            4792
        )
    }

    #[test]
    fn l2_emission_cost() {
        let l1_handler_1 = Some(10);
        let l1_handler_2 = None;

        // CONSUMED_MSG_TO_L2_N_TOPICS = 3
        // CONSUMED_MSG_TO_L2_ENCODED_DATA_SIZE = 3
        // result = emission_cost(3, 3+10)
        assert_eq!(
            get_consumed_message_to_l2_emissions_cost(l1_handler_1),
            5203
        );
        assert_eq!(get_consumed_message_to_l2_emissions_cost(l1_handler_2), 0);
    }

    #[test]
    fn message_segment_len() {
        let ord_ev1 = OrderedL2ToL1Message::new(1, Address(1235.into()), vec![4.into()]);
        let ord_ev2 = OrderedL2ToL1Message::new(2, Address(35.into()), vec![5.into(), 6.into()]);
        let message1 = L2toL1MessageInfo::new(ord_ev1, Address(1234.into()));
        let message2 = L2toL1MessageInfo::new(ord_ev2, Address(1235.into()));

        let ord_ev3 = OrderedL2ToL1Message::new(1, Address(1235.into()), vec![5.into(), 6.into()]);
        let ord_ev4 = OrderedL2ToL1Message::new(2, Address(35.into()), vec![4.into()]);
        let message3 = L2toL1MessageInfo::new(ord_ev3, Address(1234.into()));
        let message4 = L2toL1MessageInfo::new(ord_ev4, Address(1235.into()));

        let l1_handler_1 = Some(10);
        let l1_handler_2 = None;

        // L2_TO_L1_MSG_HEADER_SIZE = 3
        // iterations
        // initial value: acc = 0
        // first iteration: acc + 3 + 1 -> acc = 4
        // second iteration: acc + 3 + 2 = 9
        // 9 + 5 + size
        assert_eq!(
            get_message_segment_lenght(&[message1, message2], l1_handler_1),
            24
        );

        // iterations
        // initial value: acc = 0
        // first iteration: acc + 3 + 2 -> acc = 5
        // second iteration: acc + 3 + 1 = 9
        // 9
        assert_eq!(
            get_message_segment_lenght(&[message3, message4], l1_handler_2),
            9
        );
    }

    #[test]
    fn transaction_gas_usage_test() {
        let ord_ev1 = OrderedL2ToL1Message::new(1, Address(1235.into()), vec![4.into()]);
        let ord_ev2 = OrderedL2ToL1Message::new(2, Address(35.into()), vec![5.into(), 6.into()]);
        let message1 = L2toL1MessageInfo::new(ord_ev1, Address(1234.into()));
        let message2 = L2toL1MessageInfo::new(ord_ev2, Address(1235.into()));

        assert_eq!(
            calculate_tx_gas_usage(
                vec![message1, message2],
                &StateChangesCount {
                    n_storage_updates: 2,
                    n_class_hash_updates: 1,
                    n_compiled_class_hash_updates: 0,
                    n_modified_contracts: 2
                },
                Some(2)
            ),
            76439
        )
    }

    #[test]
    fn test_get_onchain_data_cost() {
        // Input values and expected output taken from blockifier test `test_onchain_data_discount`
        let state_changes = StateChangesCount {
            n_storage_updates: 1,
            n_class_hash_updates: 0,
            n_compiled_class_hash_updates: 0,
            n_modified_contracts: 7,
        };

        let onchain_data_cost = get_onchain_data_cost(&state_changes);
        assert_eq!(onchain_data_cost, 6392)
    }
}
