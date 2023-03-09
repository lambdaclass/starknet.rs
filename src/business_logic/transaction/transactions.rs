use crate::{
    business_logic::{
        execution::objects::TransactionExecutionInfo,
        state::state_api::{State, StateReader},
    },
    definitions::general_config::StarknetGeneralConfig,
    utils::Address,
};

use super::{
    error::TransactionError,
    objects::{
        internal_declare::InternalDeclare, internal_deploy::InternalDeploy,
        internal_invoke_function::InternalInvokeFunction,
    },
};

pub enum Transaction {
    Deploy(InternalDeploy),
    InvokeFunction(InternalInvokeFunction),
    Declare(InternalDeclare),
}

impl Transaction {
    pub fn contract_hash(&self) -> [u8; 32] {
        match self {
            Transaction::Deploy(tx) => tx.contract_hash,
            _ => [0; 32],
        }
    }

    pub fn contract_address(&self) -> Address {
        match self {
            Transaction::Deploy(tx) => tx.contract_address.clone(),
            Transaction::InvokeFunction(tx) => tx.contract_address.clone(),
            Transaction::Declare(tx) => tx.account_contract_address(),
        }
    }

    pub fn execute<S: Default + State + StateReader + Clone>(
        &self,
        state: &mut S,
        general_config: &StarknetGeneralConfig,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        match self {
            Transaction::Deploy(tx) => tx.execute(state, general_config),
            Transaction::Declare(tx) => tx.execute(state, general_config),
            Transaction::InvokeFunction(tx) => tx
                .execute(state, general_config)
                .map_err(|e| TransactionError::InvokeExecutionError(e.to_string())),
        }
    }
}
