use super::{
    error::{FeeCheckError, TransactionError},
    VersionSpecificAccountTxFields,
};
use crate::{
    definitions::{
        block_context::{BlockContext, FeeType},
        constants::{INITIAL_GAS_COST, TRANSFER_ENTRY_POINT_SELECTOR},
    },
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        gas_usage::get_onchain_data_segment_length,
        os_usage::{
            ESTIMATED_DECLARE_STEPS, ESTIMATED_DEPLOY_ACCOUNT_STEPS,
            ESTIMATED_INVOKE_FUNCTION_STEPS,
        },
        CallInfo, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::deprecated_contract_class::EntryPointType,
    state::{
        cached_state::CachedState,
        contract_class_cache::ContractClassCache,
        state_api::{State, StateChangesCount, StateReader},
        ExecutionResourcesManager,
    },
    transaction::Address,
};
use cairo_vm::Felt252;
use num_traits::{ToPrimitive, Zero};
use std::collections::HashMap;

#[cfg(feature = "cairo-native")]
use {
    crate::transaction::ClassHash,
    cairo_native::cache::ProgramCache,
    std::{cell::RefCell, rc::Rc},
};

// second element is the actual fee that the transaction uses
pub type FeeInfo = (Option<CallInfo>, u128);

/// Transfers the amount actual_fee from the caller account to the sequencer.
/// Returns the resulting CallInfo of the transfer call.
pub(crate) fn execute_fee_transfer<S: StateReader, C: ContractClassCache>(
    state: &mut CachedState<S, C>,
    block_context: &BlockContext,
    tx_execution_context: &mut TransactionExecutionContext,
    actual_fee: u128,
    #[cfg(feature = "cairo-native")] program_cache: Option<
        Rc<RefCell<ProgramCache<'_, ClassHash>>>,
    >,
) -> Result<CallInfo, TransactionError> {
    let fee_token_address = block_context
        .get_fee_token_address_by_fee_type(&tx_execution_context.account_tx_fields.fee_type())
        .clone();

    let fee_transfer_call = ExecutionEntryPoint::new(
        fee_token_address,
        vec![
            block_context.block_info.sequencer_address.0, // Recipient
            Felt252::from(actual_fee),                    // Fee.low  (U256)
            0.into(),                                     // Fee.high (U256)
        ],
        *TRANSFER_ENTRY_POINT_SELECTOR,
        tx_execution_context.account_contract_address.clone(),
        EntryPointType::External,
        Some(CallType::Call),
        None,
        INITIAL_GAS_COST,
    );

    let ExecutionResult { call_info, .. } = fee_transfer_call
        .execute(
            state,
            block_context,
            &mut ExecutionResourcesManager::default(),
            tx_execution_context,
            false,
            block_context.invoke_tx_max_n_steps,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )
        .map_err(|e| TransactionError::FeeTransferError(Box::new(e)))?;

    call_info.ok_or(TransactionError::CallInfoIsNone)
}

/// Calculates the fee that should be charged, given execution resources.
pub fn calculate_tx_fee(
    resources: &HashMap<String, usize>,
    block_context: &BlockContext,
    fee_type: &FeeType,
) -> Result<u128, TransactionError> {
    let l1_gas_usage = calculate_tx_l1_gas_usage(resources, block_context)?;
    Ok(l1_gas_usage * block_context.get_gas_price_by_fee_type(fee_type))
}

/// Computes and returns the total L1 gas consumption.
/// We add the l1_gas_usage (which may include, for example, the direct cost of L2-to-L1 messages)
/// to the gas consumed by Cairo VM resource.
pub fn calculate_tx_l1_gas_usage(
    resources: &HashMap<String, usize>,
    block_context: &BlockContext,
) -> Result<u128, TransactionError> {
    let gas_usage = resources
        .get(&"l1_gas_usage".to_string())
        .ok_or_else(|| TransactionError::FeeError("Invalid fee value".to_string()))?
        .to_owned();

    let l1_gas_by_cairo_usage = calculate_l1_gas_by_cairo_usage(block_context, resources)?;
    let total_l1_gas_usage = gas_usage.to_f64().unwrap() + l1_gas_by_cairo_usage;

    Ok(total_l1_gas_usage.ceil() as u128)
}

/// Calculates the L1 gas consumed when submitting the underlying Cairo program to SHARP.
/// I.e., returns the heaviest Cairo resource weight (in terms of L1 gas), as the size of
/// a proof is determined similarly - by the (normalized) largest segment.
pub(crate) fn calculate_l1_gas_by_cairo_usage(
    block_context: &BlockContext,
    cairo_resource_usage: &HashMap<String, usize>,
) -> Result<f64, TransactionError> {
    if !cairo_resource_usage
        .keys()
        .all(|k| k == "l1_gas_usage" || block_context.cairo_resource_fee_weights.contains_key(k))
    {
        return Err(TransactionError::ResourcesError);
    }

    // Convert Cairo usage to L1 gas usage.
    Ok(max_of_keys(
        cairo_resource_usage,
        &block_context.cairo_resource_fee_weights,
    ))
}

/// Calculates the maximum weighted value from a given resource usage mapping.
fn max_of_keys(cairo_rsc: &HashMap<String, usize>, weights: &HashMap<String, f64>) -> f64 {
    let mut max = 0.0_f64;
    for (k, v) in weights {
        let val = cairo_rsc.get(k).unwrap_or(&0).to_f64().unwrap_or(0.0_f64);
        max = f64::max(max, val * v);
    }
    max
}

/// Calculates and charges the actual fee.
///
/// # Parameters:
/// - `state`: A [CachedState].
/// - `resources`: The resources that are in use by the contract
/// - `block_context`: The block's execution context.
/// - `max_fee`: The maximum fee that the transaction is allowed to charge.
/// - `tx_execution_context`: The transaction's execution context.
/// - `skip_fee_transfer`: Whether to skip the fee transfer.
///
/// # Errors
/// - [TransactionError::ActualFeeExceedsMaxFee] - If the actual fee is bigger than the maximal fee.
///
/// # Returns
/// The [FeeInfo] with the given actual fee.
pub fn charge_fee<S: StateReader, C: ContractClassCache>(
    state: &mut CachedState<S, C>,
    calculated_fee: u128,
    block_context: &BlockContext,
    tx_execution_context: &mut TransactionExecutionContext,
    skip_fee_transfer: bool,
    #[cfg(feature = "cairo-native")] program_cache: Option<
        Rc<RefCell<ProgramCache<'_, ClassHash>>>,
    >,
) -> Result<FeeInfo, TransactionError> {
    let max_fee = tx_execution_context.account_tx_fields.max_fee();
    if max_fee.is_zero() {
        return Ok((None, 0));
    }

    let actual_fee = if tx_execution_context.version.is_zero() && calculated_fee > max_fee {
        0
    } else {
        calculated_fee.min(max_fee)
    };

    let fee_transfer_info = if skip_fee_transfer {
        None
    } else {
        Some(execute_fee_transfer(
            state,
            block_context,
            tx_execution_context,
            actual_fee,
            #[cfg(feature = "cairo-native")]
            program_cache,
        )?)
    };

    Ok((fee_transfer_info, actual_fee))
}
// Minimal Fee estimation

pub(crate) enum AccountTxType {
    Declare,
    Invoke,
    DeployAccount,
}

pub(crate) fn check_fee_bounds(
    account_tx_fields: &VersionSpecificAccountTxFields,
    block_context: &BlockContext,
    tx_type: AccountTxType,
) -> Result<(), TransactionError> {
    let minimal_l1_gas_amount = estimate_minimal_l1_gas(block_context, tx_type)?;
    match account_tx_fields {
        VersionSpecificAccountTxFields::Deprecated(max_fee) => {
            let minimal_fee = minimal_l1_gas_amount
                * block_context.get_gas_price_by_fee_type(&account_tx_fields.fee_type());
            // Check max fee is at least the estimated constant overhead.
            if *max_fee < minimal_fee {
                return Err(TransactionError::MaxFeeTooLow(*max_fee, minimal_fee));
            }
        }
        VersionSpecificAccountTxFields::Current(fields) => {
            // Check l1_gas amount
            if (fields.l1_resource_bounds.max_amount as u128) < minimal_l1_gas_amount {
                Err(TransactionError::MaxL1GasAmountTooLow(
                    fields.l1_resource_bounds.max_amount,
                    minimal_l1_gas_amount,
                ))?;
            }
            // Check l1_gas price
            let actual_gas_price =
                block_context.get_gas_price_by_fee_type(&account_tx_fields.fee_type());
            if fields.l1_resource_bounds.max_price_per_unit < actual_gas_price {
                Err(TransactionError::MaxL1GasPriceTooLow(
                    fields.l1_resource_bounds.max_price_per_unit,
                    actual_gas_price,
                ))?;
            }
        }
    }
    Ok(())
}

pub(crate) fn run_post_execution_fee_checks<S: StateReader, C: ContractClassCache>(
    state: &mut CachedState<S, C>,
    account_tx_fields: &VersionSpecificAccountTxFields,
    block_context: &BlockContext,
    actual_fee: u128,
    actual_resources: &HashMap<String, usize>,
    sender_address: &Address,
    skip_fee_transfer: bool,
) -> Result<(), TransactionError> {
    if account_tx_fields.max_fee().is_zero() {
        return Ok(());
    }
    check_actual_cost_within_bounds(
        block_context,
        account_tx_fields,
        actual_fee,
        actual_resources,
    )?;
    check_can_pay_fee(
        block_context,
        state,
        sender_address,
        actual_fee,
        &account_tx_fields.fee_type(),
        skip_fee_transfer,
    )
}

// Checks that the cost of the transaction (Measured by l1_gas in V3 Txs, and by fee in lower Tx versions) is within the bounds of the transaction
fn check_actual_cost_within_bounds(
    block_context: &BlockContext,
    account_tx_fields: &VersionSpecificAccountTxFields,
    actual_fee: u128,
    actual_resources: &HashMap<String, usize>,
) -> Result<(), TransactionError> {
    match account_tx_fields {
        VersionSpecificAccountTxFields::Current(fields) => {
            let actual_used_l1_gas = calculate_tx_l1_gas_usage(actual_resources, block_context)?;
            if actual_used_l1_gas > fields.l1_resource_bounds.max_amount as u128 {
                return Err(FeeCheckError::L1GasAmountExceedsMax(
                    actual_used_l1_gas,
                    fields.l1_resource_bounds.max_amount,
                )
                .into());
            }
        }
        VersionSpecificAccountTxFields::Deprecated(max_fee) => {
            if actual_fee > *max_fee {
                return Err(FeeCheckError::FeeExceedsMax(actual_fee, *max_fee).into());
            }
        }
    }
    Ok(())
}

// Checks that the account's fee token balance is high enough to cover the actual_fee
fn check_can_pay_fee<S: StateReader, C: ContractClassCache>(
    block_context: &BlockContext,
    state: &mut CachedState<S, C>,
    sender_address: &Address,
    actual_fee: u128,
    fee_type: &FeeType,
    skip_fee_transfer: bool,
) -> Result<(), TransactionError> {
    // Check if as a result of tx execution the sender's fee token balance is not enough to pay the actual_fee.
    let (balance_low, balance_high) =
        state.get_fee_token_balance(block_context, sender_address, fee_type)?;
    // The fee is at most 128 bits, while balance is 256 bits (split into two 128 bit words).
    if balance_high.is_zero() && balance_low < Felt252::from(actual_fee) && !skip_fee_transfer {
        Err(FeeCheckError::InsufficientFeeTokenBalance.into())
    } else {
        Ok(())
    }
}

pub(crate) fn estimate_minimal_l1_gas(
    block_context: &BlockContext,
    tx_type: AccountTxType,
) -> Result<u128, TransactionError> {
    let n_estimated_steps = match tx_type {
        AccountTxType::Declare => ESTIMATED_DECLARE_STEPS,
        AccountTxType::Invoke => ESTIMATED_INVOKE_FUNCTION_STEPS,
        AccountTxType::DeployAccount => ESTIMATED_DEPLOY_ACCOUNT_STEPS,
    };
    let state_changes = match tx_type {
        AccountTxType::Declare => StateChangesCount {
            n_storage_updates: 1,
            n_class_hash_updates: 0,
            n_compiled_class_hash_updates: 0,
            n_modified_contracts: 1,
        },
        AccountTxType::Invoke => StateChangesCount {
            n_storage_updates: 1,
            n_class_hash_updates: 0,
            n_compiled_class_hash_updates: 0,
            n_modified_contracts: 1,
        },
        AccountTxType::DeployAccount => StateChangesCount {
            n_storage_updates: 1,
            n_class_hash_updates: 1,
            n_compiled_class_hash_updates: 0,
            n_modified_contracts: 1,
        },
    };
    let gas_cost = get_onchain_data_segment_length(&state_changes);
    let resources = HashMap::from([
        ("l1_gas_usage".to_string(), gas_cost),
        ("n_steps".to_string(), n_estimated_steps),
    ]);
    calculate_tx_l1_gas_usage(&resources, block_context)
}

#[cfg(test)]
mod tests {
    use crate::{
        definitions::block_context::{BlockContext, GasPrices},
        execution::TransactionExecutionContext,
        state::{
            cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
            in_memory_state_reader::InMemoryStateReader,
        },
        transaction::{
            fee::{calculate_tx_fee, charge_fee},
            VersionSpecificAccountTxFields,
        },
    };
    use std::{collections::HashMap, sync::Arc};

    /// Tests the behavior of the charge_fee function when the actual fee exceeds the maximum fee
    /// for version 0. It expects to return an ActualFeeExceedsMaxFee error.
    #[test]
    fn charge_fee_v0_max_fee_exceeded_should_charge_nothing() {
        let mut state = CachedState::new(
            Arc::new(InMemoryStateReader::default()),
            Arc::new(PermanentContractClassCache::default()),
        );
        let mut tx_execution_context = TransactionExecutionContext::default();
        let mut block_context = BlockContext::default();
        block_context.block_info.gas_price = GasPrices::new(1, 0);
        let resources = HashMap::from([
            ("l1_gas_usage".to_string(), 200_usize),
            ("pedersen_builtin".to_string(), 10000_usize),
        ]);
        tx_execution_context.account_tx_fields = VersionSpecificAccountTxFields::Deprecated(100);
        let skip_fee_transfer = true;

        let calculated_fee = calculate_tx_fee(
            &resources,
            &block_context,
            &tx_execution_context.account_tx_fields.fee_type(),
        )
        .unwrap();

        let result = charge_fee(
            &mut state,
            calculated_fee,
            &block_context,
            &mut tx_execution_context,
            skip_fee_transfer,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

        assert_eq!(result.1, 0);
    }

    /// Tests the behavior of the charge_fee function when the actual fee exceeds the maximum fee
    /// for version 1. It expects the function to return the maximum fee.
    #[test]
    fn charge_fee_v1_max_fee_exceeded_should_charge_max_fee() {
        let mut state = CachedState::new(
            Arc::new(InMemoryStateReader::default()),
            Arc::new(PermanentContractClassCache::default()),
        );
        let mut tx_execution_context = TransactionExecutionContext {
            version: 1.into(),
            ..Default::default()
        };
        let mut block_context = BlockContext::default();
        block_context.block_info.gas_price = GasPrices::new(1, 0);
        let resources = HashMap::from([
            ("l1_gas_usage".to_string(), 200_usize),
            ("pedersen_builtin".to_string(), 10000_usize),
        ]);
        tx_execution_context.account_tx_fields = VersionSpecificAccountTxFields::Deprecated(100);
        let skip_fee_transfer = true;

        let calculated_fee = calculate_tx_fee(
            &resources,
            &block_context,
            &tx_execution_context.account_tx_fields.fee_type(),
        )
        .unwrap();

        let result = charge_fee(
            &mut state,
            calculated_fee,
            &block_context,
            &mut tx_execution_context,
            skip_fee_transfer,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

        assert_eq!(result.1, 100);
    }
}
