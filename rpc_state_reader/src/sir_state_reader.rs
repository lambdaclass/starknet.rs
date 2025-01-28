use std::sync::Arc;

use cairo_vm::Felt252;
use starknet_api::{
    block::BlockNumber,
    core::{ClassHash as SNClassHash, ContractAddress, PatriciaKey},
    hash::{StarkFelt, StarkHash},
    state::StorageKey,
    transaction::{Transaction as SNTransaction, TransactionHash},
};
use starknet_in_rust::{
    core::errors::state_errors::StateError,
    definitions::{
        block_context::{BlockContext, FeeTokenAddresses, StarknetChainId, StarknetOsConfig},
        constants::{
            DEFAULT_CAIRO_RESOURCE_FEE_WEIGHTS, DEFAULT_CONTRACT_STORAGE_COMMITMENT_TREE_HEIGHT,
            DEFAULT_GLOBAL_STATE_COMMITMENT_TREE_HEIGHT, DEFAULT_INVOKE_TX_MAX_N_STEPS,
            DEFAULT_VALIDATE_MAX_N_STEPS,
        },
    },
    execution::TransactionExecutionInfo,
    services::api::contract_classes::compiled_class::CompiledClass,
    state::{
        cached_state::CachedState,
        contract_class_cache::{ContractClassCache, PermanentContractClassCache},
        state_api::StateReader,
        state_cache::StorageEntry,
        BlockInfo,
    },
    transaction::{
        declare_tx_from_sn_api_transaction, error::TransactionError, Address, ClassHash,
        DeployAccount, InvokeFunction, L1Handler,
    },
};

use crate::{
    rpc_state::{
        BlockValue, RpcBlockInfo, RpcChain, RpcState, RpcTransactionReceipt, TransactionTrace,
    },
    rpc_state_errors::RpcStateError,
};

#[derive(Debug)]
pub struct RpcStateReader(pub RpcState);

impl RpcStateReader {
    pub fn new(state: RpcState) -> Self {
        Self(state)
    }
}

impl StateReader for RpcStateReader {
    fn get_contract_class(&self, class_hash: &ClassHash) -> Result<CompiledClass, StateError> {
        let hash = SNClassHash(StarkHash::new(class_hash.0).unwrap());
        let contract_class = self
            .0
            .get_contract_class(&hash)
            .ok_or(StateError::MissingCasmClass(*class_hash))?;
        Ok(CompiledClass::from(contract_class))
    }

    fn get_class_hash_at(&self, contract_address: &Address) -> Result<ClassHash, StateError> {
        let address = ContractAddress(
            PatriciaKey::try_from(
                StarkHash::new(contract_address.clone().0.to_bytes_be()).unwrap(),
            )
            .unwrap(),
        );
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(self.0.get_class_hash_at(&address).0.bytes());
        Ok(ClassHash(bytes))
    }

    fn get_nonce_at(&self, contract_address: &Address) -> Result<Felt252, StateError> {
        let address = ContractAddress(
            PatriciaKey::try_from(
                StarkHash::new(contract_address.clone().0.to_bytes_be()).unwrap(),
            )
            .unwrap(),
        );
        let nonce = self.0.get_nonce_at(&address);
        Ok(Felt252::from_bytes_be_slice(nonce.bytes()))
    }

    fn get_storage_at(&self, storage_entry: &StorageEntry) -> Result<Felt252, StateError> {
        let (contract_address, key) = storage_entry;
        let address = ContractAddress(
            PatriciaKey::try_from(
                StarkHash::new(contract_address.clone().0.to_bytes_be()).unwrap(),
            )
            .unwrap(),
        );
        let key = StorageKey(PatriciaKey::try_from(StarkHash::new(*key).unwrap()).unwrap());
        let value = self.0.get_storage_at(&address, &key);
        Ok(Felt252::from_bytes_be_slice(value.bytes()))
    }

    fn get_compiled_class_hash(&self, class_hash: &ClassHash) -> Result<ClassHash, StateError> {
        Ok(*class_hash)
    }
}

pub fn execute_tx_configurable(
    tx_hash: &str,
    network: RpcChain,
    block_number: BlockNumber,
    skip_validate: bool,
    skip_nonce_check: bool,
) -> Result<
    (
        TransactionExecutionInfo,
        TransactionTrace,
        RpcTransactionReceipt,
    ),
    TransactionError,
> {
    let rpc_reader = RpcStateReader(RpcState::new_rpc(network, block_number.into()).unwrap());
    let class_cache = PermanentContractClassCache::default();
    let mut state = CachedState::new(Arc::new(rpc_reader), Arc::new(class_cache));
    let tx_hash =
        TransactionHash(StarkFelt::try_from(tx_hash.strip_prefix("0x").unwrap()).unwrap());
    let tx = state.state_reader.0.get_transaction(&tx_hash).unwrap();
    let gas_price = state.state_reader.0.get_gas_price(block_number.0).unwrap();
    let RpcBlockInfo {
        block_timestamp,
        sequencer_address,
        ..
    } = state.state_reader.0.get_block_info().unwrap();
    let sequencer_address = Address(Felt252::from_bytes_be_slice(
        sequencer_address.0.key().bytes(),
    ));
    let block_info = BlockInfo {
        block_number: block_number.0,
        block_timestamp: block_timestamp.0,
        gas_price,
        sequencer_address,
    };
    let sir_exec_info = execute_tx_configurable_with_state(
        &tx_hash,
        tx,
        network,
        block_info,
        skip_validate,
        skip_nonce_check,
        &mut state,
    )?;
    let trace = state
        .state_reader
        .0
        .get_transaction_trace(&tx_hash)
        .unwrap();
    let receipt = state
        .state_reader
        .0
        .get_transaction_receipt(&tx_hash)
        .unwrap();
    Ok((sir_exec_info, trace, receipt))
}

pub fn execute_tx_configurable_with_state(
    tx_hash: &TransactionHash,
    tx: SNTransaction,
    network: RpcChain,
    block_info: BlockInfo,
    skip_validate: bool,
    skip_nonce_check: bool,
    state: &mut CachedState<RpcStateReader, PermanentContractClassCache>,
) -> Result<TransactionExecutionInfo, TransactionError> {
    let fee_token_address = FeeTokenAddresses::new(
        Address(
            Felt252::from_hex("049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7")
                .unwrap(),
        ),
        Address::default(),
    );

    // Get values for block context before giving ownership of the reader
    let chain_id = match state.state_reader.0.chain {
        RpcChain::MainNet => StarknetChainId::MainNet,
        RpcChain::TestNet => StarknetChainId::TestNet,
        RpcChain::TestNet2 => StarknetChainId::TestNet2,
    };
    let starknet_os_config = StarknetOsConfig::new(chain_id.to_felt(), fee_token_address);

    // Get transaction before giving ownership of the reader
    let tx = match tx {
        SNTransaction::Invoke(tx) => InvokeFunction::from_invoke_transaction(
            tx,
            Felt252::from_bytes_be_slice(tx_hash.0.bytes()),
        )
        .unwrap()
        .create_for_simulation(skip_validate, false, false, false, skip_nonce_check),
        SNTransaction::DeployAccount(tx) => DeployAccount::from_sn_api_transaction(
            tx,
            Felt252::from_bytes_be_slice(tx_hash.0.bytes()),
        )
        .unwrap()
        .create_for_simulation(skip_validate, false, false, false, skip_nonce_check),
        SNTransaction::Declare(tx) => {
            // Try to fetch contract class from cache
            let class_hash = ClassHash(tx.class_hash().0.bytes().try_into().unwrap());
            let contract_class = if let Ok(contract_class) = state.get_contract_class(&class_hash) {
                contract_class
            } else {
                // Fetch the contract_class from the next block (as we don't have it in the previous one)
                let next_block_state_reader = RpcStateReader(
                    RpcState::new_rpc(network, BlockNumber(block_info.block_number).next().into())
                        .unwrap(),
                );

                let contract_class = next_block_state_reader
                    .get_contract_class(&class_hash)
                    .unwrap();

                // Manually add the contract class to the cache so we don't need to fetch it when benchmarking (replay crate)
                state
                    .contract_class_cache_mut()
                    .set_contract_class(class_hash, contract_class.clone());
                contract_class
            };

            let declare = declare_tx_from_sn_api_transaction(
                tx,
                Felt252::from_bytes_be_slice(tx_hash.0.bytes()),
                contract_class,
            )?;
            declare.create_for_simulation(skip_validate, false, false, false, skip_nonce_check)
        }
        SNTransaction::L1Handler(tx) => L1Handler::from_sn_api_tx(
            tx,
            Felt252::from_bytes_be_slice(tx_hash.0.bytes()),
            Some(Felt252::from(u128::MAX)),
        )
        .unwrap()
        .create_for_simulation(skip_validate, false),
        SNTransaction::Deploy(_) => unimplemented!(),
    };

    let block_context = BlockContext::new(
        starknet_os_config,
        DEFAULT_CONTRACT_STORAGE_COMMITMENT_TREE_HEIGHT,
        DEFAULT_GLOBAL_STATE_COMMITMENT_TREE_HEIGHT,
        DEFAULT_CAIRO_RESOURCE_FEE_WEIGHTS.clone(),
        DEFAULT_INVOKE_TX_MAX_N_STEPS,
        DEFAULT_VALIDATE_MAX_N_STEPS,
        block_info,
        Default::default(),
        true,
    );

    #[cfg(not(feature = "cairo-native"))]
    let sir_execution = tx.execute(state, &block_context, u128::MAX)?;
    #[cfg(feature = "cairo-native")]
    let sir_execution = tx.execute(state, &block_context, u128::MAX, None)?;

    Ok(sir_execution)
}

pub fn execute_tx(
    tx_hash: &str,
    network: RpcChain,
    block_number: BlockNumber,
) -> Result<
    (
        TransactionExecutionInfo,
        TransactionTrace,
        RpcTransactionReceipt,
    ),
    TransactionError,
> {
    execute_tx_configurable(tx_hash, network, block_number, false, false)
}

pub fn execute_tx_without_validate(
    tx_hash: &str,
    network: RpcChain,
    block_number: BlockNumber,
) -> Result<
    (
        TransactionExecutionInfo,
        TransactionTrace,
        RpcTransactionReceipt,
    ),
    TransactionError,
> {
    execute_tx_configurable(tx_hash, network, block_number, true, true)
}

pub fn get_transaction_hashes(
    block_number: BlockNumber,
    network: RpcChain,
) -> Result<Vec<String>, RpcStateError> {
    let rpc_state = RpcState::new_rpc(network, BlockValue::Number(block_number))?;
    rpc_state.get_transaction_hashes()
}
