use super::error::TransactionError;
use crate::{
    definitions::{
        block_context::{BlockContext, FeeType},
        constants::{FEE_FACTOR, INITIAL_GAS_COST, TRANSFER_ENTRY_POINT_SELECTOR},
    },
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        CallInfo, CallType, TransactionExecutionContext,
    },
    services::api::contract_classes::deprecated_contract_class::EntryPointType,
    state::{
        cached_state::CachedState, contract_class_cache::ContractClassCache,
        state_api::StateReader, ExecutionResourcesManager,
    },
};
use cairo_vm::Felt252;
use num_traits::{ToPrimitive, Zero};
use std::collections::HashMap;

#[cfg(feature = "cairo-native")]
use {
    crate::utils::ClassHash,
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
    if actual_fee > tx_execution_context.account_tx_fields.max_fee() {
        return Err(TransactionError::ActualFeeExceedsMaxFee(
            actual_fee,
            tx_execution_context.account_tx_fields.max_fee(),
        ));
    }

    let fee_token_address = block_context.starknet_os_config.fee_token_address.clone();

    let calldata = [
        block_context.block_info.sequencer_address.0,
        Felt252::from(actual_fee), // U256.low
        0.into(),                  // U256.high
    ]
    .to_vec();

    let fee_transfer_call = ExecutionEntryPoint::new(
        fee_token_address.get_by_fee_type(&FeeType::Eth).clone(),
        calldata,
        *TRANSFER_ENTRY_POINT_SELECTOR,
        tx_execution_context.account_contract_address.clone(),
        EntryPointType::External,
        Some(CallType::Call),
        None,
        INITIAL_GAS_COST,
    );

    let mut resources_manager = ExecutionResourcesManager::default();
    let ExecutionResult { call_info, .. } = fee_transfer_call
        .execute(
            state,
            block_context,
            &mut resources_manager,
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
    Ok(l1_gas_usage
        * block_context
            .starknet_os_config()
            .gas_price()
            .get_by_fee_type(fee_type))
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
    resources: &HashMap<String, usize>,
    block_context: &BlockContext,
    max_fee: u128,
    tx_execution_context: &mut TransactionExecutionContext,
    skip_fee_transfer: bool,
    #[cfg(feature = "cairo-native")] program_cache: Option<
        Rc<RefCell<ProgramCache<'_, ClassHash>>>,
    >,
) -> Result<FeeInfo, TransactionError> {
    if max_fee.is_zero() {
        return Ok((None, 0));
    }

    let actual_fee = calculate_tx_fee(resources, block_context, &FeeType::Eth)?;

    let actual_fee = {
        let version_0 = tx_execution_context.version.is_zero();
        let fee_exceeded_max = actual_fee > max_fee;

        if version_0 && fee_exceeded_max {
            0
        } else if version_0 && !fee_exceeded_max {
            actual_fee
        } else {
            actual_fee.min(max_fee) * FEE_FACTOR
        }
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

#[cfg(test)]
mod tests {
    use crate::{
        definitions::block_context::{BlockContext, GasPrices},
        execution::TransactionExecutionContext,
        state::{
            cached_state::CachedState, contract_class_cache::PermanentContractClassCache,
            in_memory_state_reader::InMemoryStateReader,
        },
        transaction::fee::charge_fee,
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
        block_context.starknet_os_config.gas_price = GasPrices::new(1, 0);
        let resources = HashMap::from([
            ("l1_gas_usage".to_string(), 200_usize),
            ("pedersen_builtin".to_string(), 10000_usize),
        ]);
        let max_fee = 100;
        let skip_fee_transfer = true;

        let result = charge_fee(
            &mut state,
            &resources,
            &block_context,
            max_fee,
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
        block_context.starknet_os_config.gas_price = GasPrices::new(1, 0);
        let resources = HashMap::from([
            ("l1_gas_usage".to_string(), 200_usize),
            ("pedersen_builtin".to_string(), 10000_usize),
        ]);
        let max_fee = 100;
        let skip_fee_transfer = true;

        let result = charge_fee(
            &mut state,
            &resources,
            &block_context,
            max_fee,
            &mut tx_execution_context,
            skip_fee_transfer,
            #[cfg(feature = "cairo-native")]
            None,
        )
        .unwrap();

        assert_eq!(result.1, max_fee);
    }
}
