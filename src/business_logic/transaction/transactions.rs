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
    objects::{internal_deploy::InternalDeploy, internal_invoke_function::InternalInvokeFunction},
};

pub(crate) enum Transaction {
    Deploy(InternalDeploy),
    InvokeFunction(InternalInvokeFunction),
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
        }
    }

    pub fn apply_state_updates<S: Default + State + StateReader + Clone>(
        &self,
        state: &mut S,
        general_config: &StarknetGeneralConfig,
    ) -> Result<TransactionExecutionInfo, TransactionError> {
        match self {
            Transaction::Deploy(tx) => tx.apply_state_updates(state, general_config),
            Transaction::InvokeFunction(_) => todo!(),
        }
    }
}
