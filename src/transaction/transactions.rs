use crate::{
    definitions::general_config::TransactionContext,
    execution::TransactionExecutionInfo,
    state::state_api::{State, StateReader},
    utils::{Address, ClassHash},
};

use super::{error::TransactionError, Deploy, InvokeFunction};

pub enum Transaction {
    Deploy(Deploy),
    InvokeFunction(InvokeFunction),
}

impl Transaction {
    pub fn contract_hash(&self) -> ClassHash {
        match self {
            Transaction::Deploy(tx) => tx.contract_hash,
            _ => [0; 32],
        }
    }

    pub fn contract_address(&self) -> Address {
        match self {
            Transaction::Deploy(tx) => tx.contract_address.clone(),
            Transaction::InvokeFunction(tx) => tx.contract_address().clone(),
        }
    }

    pub fn execute<S: State + StateReader>(
        &self,
        state: &mut S,
        general_config: &TransactionContext,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        match self {
            Transaction::Deploy(tx) => tx.execute(state, general_config),
            Transaction::InvokeFunction(tx) => tx.execute(state, general_config),
        }
    }
}