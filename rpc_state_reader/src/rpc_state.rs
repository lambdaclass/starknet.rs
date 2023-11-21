use cairo_vm::vm::runners::cairo_runner::ExecutionResources as VmExecutionResources;
use core::fmt;
use dotenv::dotenv;
use serde::{Deserialize, Deserializer};
use serde_json::json;
use starknet::core::types::ContractClass as SNContractClass;
use starknet_api::{
    block::{BlockNumber, BlockTimestamp},
    core::{ChainId, ClassHash, ContractAddress},
    hash::{StarkFelt, StarkHash},
    state::StorageKey,
    transaction::{Transaction as SNTransaction, TransactionHash},
};
use starknet_in_rust::definitions::block_context::StarknetChainId;
use std::{collections::HashMap, env, fmt::Display};

use crate::{rpc_state_errors::RpcStateError, utils};

/// Starknet chains supported in Infura.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum RpcChain {
    MainNet,
    TestNet,
    TestNet2,
}

impl From<RpcChain> for StarknetChainId {
    fn from(network: RpcChain) -> StarknetChainId {
        match network {
            RpcChain::MainNet => StarknetChainId::MainNet,
            RpcChain::TestNet => StarknetChainId::TestNet,
            RpcChain::TestNet2 => StarknetChainId::TestNet2,
        }
    }
}

impl fmt::Display for RpcChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcChain::MainNet => write!(f, "starknet-mainnet"),
            RpcChain::TestNet => write!(f, "starknet-goerli"),
            RpcChain::TestNet2 => write!(f, "starknet-goerli2"),
        }
    }
}

impl From<RpcChain> for ChainId {
    fn from(value: RpcChain) -> Self {
        ChainId(match value {
            RpcChain::MainNet => "alpha-mainnet".to_string(),
            RpcChain::TestNet => "alpha4".to_string(),
            RpcChain::TestNet2 => "alpha4-2".to_string(),
        })
    }
}

/// A [StateReader] that holds all the data in memory.
///
/// This implementation is uses HTTP requests to call the RPC endpoint,
/// using Infura.
/// In order to use it an Infura API key is necessary.
#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct RpcState {
    /// Enum with one of the supported Infura chains/
    pub chain: RpcChain,
    /// RPC Endpoint URL.
    rpc_endpoint: String,
    /// The url to the starknet feeder.
    feeder_url: String,
    /// Struct that holds information on the block where we are going to use to read the state.
    pub block: BlockValue,
}

/// Represents the tag of a block value.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BlockTag {
    Latest,
    Pending,
}

impl Display for BlockTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let string = match self {
            BlockTag::Latest => "latest",
            BlockTag::Pending => "pending",
        };
        write!(f, "{}", string)
    }
}

/// [`BlockValue`] is an Enum that represent which block we are going to use to retrieve information.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum BlockValue {
    /// String one of: ["latest", "pending"]
    Tag(BlockTag),
    /// Integer
    Number(BlockNumber),
    /// String with format: 0x{felt252}
    Hash(StarkHash),
}

impl From<BlockTag> for BlockValue {
    fn from(value: BlockTag) -> Self {
        BlockValue::Tag(value)
    }
}

impl From<BlockNumber> for BlockValue {
    fn from(value: BlockNumber) -> Self {
        BlockValue::Number(value)
    }
}

impl From<StarkHash> for BlockValue {
    fn from(value: StarkHash) -> Self {
        BlockValue::Hash(value)
    }
}

impl BlockValue {
    fn to_value(self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(match self {
            BlockValue::Tag(block_tag) => block_tag.to_string().into(),
            BlockValue::Number(block_number) => json!({ "block_number": block_number }),
            BlockValue::Hash(block_hash) => json!({ "block_hash": block_hash }),
        })
    }
}

/// The RPC block info.
#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct RpcBlockInfo {
    /// The sequence number of the last block created.
    pub block_number: BlockNumber,
    /// Timestamp of the beginning of the last block creation attempt.
    pub block_timestamp: BlockTimestamp,
    /// The sequencer address of this block.
    pub sequencer_address: ContractAddress,
    /// The transactions of this block.
    pub transactions: Vec<SNTransaction>,
}

/// A RPC response.
#[derive(Debug, Deserialize, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct RpcResponse<T> {
    result: T,
}

#[derive(Debug, Deserialize, Clone, Eq, PartialEq)]
pub struct TransactionTrace {
    pub validate_invocation: Option<RpcCallInfo>,
    pub function_invocation: Option<RpcCallInfo>,
    pub fee_transfer_invocation: Option<RpcCallInfo>,
    pub signature: Vec<StarkFelt>,
    pub revert_error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct RpcExecutionResources {
    pub n_steps: usize,
    pub n_memory_holes: usize,
    pub builtin_instance_counter: HashMap<String, usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RpcCallInfo {
    pub execution_resources: VmExecutionResources,
    pub retdata: Option<Vec<StarkFelt>>,
    pub calldata: Option<Vec<StarkFelt>>,
    pub internal_calls: Vec<RpcCallInfo>,
}

#[allow(unused)]
#[derive(Debug, Deserialize)]
pub struct RpcTransactionReceipt {
    #[serde(deserialize_with = "actual_fee_deser")]
    pub actual_fee: u128,
    pub block_hash: StarkHash,
    pub block_number: u64,
    pub execution_status: String,
    #[serde(rename = "type")]
    pub tx_type: String,
}

fn actual_fee_deser<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let hex: String = Deserialize::deserialize(deserializer)?;
    u128::from_str_radix(&hex[2..], 16).map_err(serde::de::Error::custom)
}

impl<'de> Deserialize<'de> for RpcCallInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: serde_json::Value = Deserialize::deserialize(deserializer)?;

        // Parse execution_resources
        let execution_resources_value = value
            .get("execution_resources")
            .ok_or(serde::de::Error::custom(
                "Missing field execution_resources",
            ))?
            .clone();

        let execution_resources = VmExecutionResources {
            n_steps: serde_json::from_value(
                execution_resources_value
                    .get("n_steps")
                    .ok_or(serde::de::Error::custom(
                        "Missing field execution_resources.n_steps",
                    ))?
                    .clone(),
            )
            .map_err(serde::de::Error::custom)?,
            n_memory_holes: serde_json::from_value(
                execution_resources_value
                    .get("n_memory_holes")
                    .ok_or(serde::de::Error::custom(
                        "Missing field execution_resources.n_memory_holes",
                    ))?
                    .clone(),
            )
            .map_err(serde::de::Error::custom)?,
            builtin_instance_counter: serde_json::from_value(
                execution_resources_value
                    .get("builtin_instance_counter")
                    .ok_or(serde::de::Error::custom(
                        "Missing field execution_resources.builtin_instance_counter",
                    ))?
                    .clone(),
            )
            .map_err(serde::de::Error::custom)?,
        };

        // Parse retdata
        let retdata_value = value
            .get("result")
            .ok_or(serde::de::Error::custom("Missing field result"))?
            .clone();
        let retdata = serde_json::from_value(retdata_value)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        // Parse calldata
        let calldata_value = value
            .get("calldata")
            .ok_or(serde::de::Error::custom("Missing field calldata"))?
            .clone();
        let calldata = serde_json::from_value(calldata_value)
            .map_err(|e| serde::de::Error::custom(e.to_string()))?;

        // Parse internal calls
        let internal_calls_value = value
            .get("internal_calls")
            .ok_or(serde::de::Error::custom("Missing field internal_calls"))?
            .clone();
        let mut internal_calls = vec![];

        for call in internal_calls_value
            .as_array()
            .ok_or(serde::de::Error::custom(
                "Wrong type for field internal_calls",
            ))?
        {
            internal_calls
                .push(serde_json::from_value(call.clone()).map_err(serde::de::Error::custom)?);
        }

        Ok(RpcCallInfo {
            execution_resources,
            retdata,
            calldata,
            internal_calls,
        })
    }
}

impl RpcState {
    pub fn new(chain: RpcChain, block: BlockValue, rpc_endpoint: &str, feeder_url: &str) -> Self {
        Self {
            chain,
            rpc_endpoint: rpc_endpoint.to_string(),
            feeder_url: feeder_url.to_string(),
            block,
        }
    }

    pub fn new_infura(chain: RpcChain, block: BlockValue) -> Result<Self, RpcStateError> {
        if env::var("INFURA_API_KEY").is_err() {
            dotenv().map_err(|_| RpcStateError::MissingEnvFile)?;
        }

        let rpc_endpoint = format!(
            "https://{}.infura.io/v3/{}",
            chain,
            env::var("INFURA_API_KEY").map_err(|_| RpcStateError::MissingInfuraApiKey)?
        );

        let chain_id: ChainId = chain.into();
        let feeder_url = format!("https://{}.starknet.io/feeder_gateway", chain_id);

        Ok(Self::new(chain, block, &rpc_endpoint, &feeder_url))
    }

    fn rpc_call_result<T: for<'a> Deserialize<'a>>(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<T, RpcStateError> {
        Ok(self.rpc_call::<RpcResponse<T>>(method, params)?.result)
    }

    fn rpc_call<T: for<'a> Deserialize<'a>>(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<T, RpcStateError> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });
        let response = self.rpc_call_no_deserialize(&payload)?.into_json()?;
        Self::deserialize_call(response)
    }

    fn rpc_call_no_deserialize(
        &self,
        params: &serde_json::Value,
    ) -> Result<ureq::Response, RpcStateError> {
        ureq::post(&self.rpc_endpoint)
            .set("Content-Type", "application/json")
            .set("accept", "application/json")
            .send_json(params)
            .map_err(|err| RpcStateError::Request(err.to_string()))
    }

    fn deserialize_call<T: for<'a> Deserialize<'a>>(
        response: serde_json::Value,
    ) -> Result<T, RpcStateError> {
        serde_json::from_value(response).map_err(|err| RpcStateError::RpcCall(err.to_string()))
    }

    /// Gets the url of the feeder endpoint
    fn get_feeder_endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.feeder_url, path)
    }

    /// Requests the transaction trace to the Feeder Gateway API.
    /// It's useful for testing the transaction outputs like:
    /// - execution resources
    /// - actual fee
    /// - events
    /// - return data
    pub fn get_transaction_trace(
        &self,
        hash: &TransactionHash,
    ) -> Result<TransactionTrace, RpcStateError> {
        let response = ureq::get(&self.get_feeder_endpoint("get_transaction_trace"))
            .query("transactionHash", &hash.0.to_string())
            .call()
            .map_err(|e| RpcStateError::Request(e.to_string()))?;

        serde_json::from_value(response.into_json().map_err(RpcStateError::Io)?)
            .map_err(|e| RpcStateError::Request(e.to_string()))
    }

    /// Requests the given transaction to the Feeder Gateway API.
    pub fn get_transaction(&self, hash: &TransactionHash) -> Result<SNTransaction, RpcStateError> {
        let result = self
            .rpc_call::<serde_json::Value>(
                "starknet_getTransactionByHash",
                &json!([hash.to_string()]),
            )?
            .get("result")
            .ok_or(RpcStateError::RpcCall(
                "Response has no field result".into(),
            ))?
            .clone();
        utils::deserialize_transaction_json(result).map_err(RpcStateError::SerdeJson)
    }

    /// Gets the gas price of a given block.
    pub fn get_gas_price(&self, block_number: u64) -> Result<u128, RpcStateError> {
        let response = ureq::get(&self.get_feeder_endpoint("get_block"))
            .query("blockNumber", &block_number.to_string())
            .call()
            .map_err(|e| RpcStateError::Request(e.to_string()))?;

        let res: serde_json::Value = response.into_json().map_err(RpcStateError::Io)?;

        let gas_price_hex =
            res.get("gas_price")
                .and_then(|gp| gp.as_str())
                .ok_or(RpcStateError::Request(
                    "Response has no field gas_price".to_string(),
                ))?;
        let gas_price =
            u128::from_str_radix(gas_price_hex.trim_start_matches("0x"), 16).map_err(|_| {
                RpcStateError::Request("Response field gas_price has wrong type".to_string())
            })?;
        Ok(gas_price)
    }

    pub fn get_chain_name(&self) -> ChainId {
        self.chain.into()
    }

    pub fn get_block_info(&self) -> Result<RpcBlockInfo, RpcStateError> {
        let block_info: serde_json::Value = self
            .rpc_call("starknet_getBlockWithTxs", &json!([self.block.to_value()?]))
            .map_err(|e| RpcStateError::RpcCall(e.to_string()))?;

        let sequencer_address: StarkFelt = block_info
            .get("result")
            .and_then(|result| result.get("sequencer_address"))
            .and_then(|sa| serde_json::from_value(sa.clone()).ok())
            .ok_or_else(|| {
                RpcStateError::RpcObjectHasNoField("block_info".into(), "sequencer_address".into())
            })?;

        let transactions: Vec<_> = block_info
            .get("result")
            .and_then(|result| result.get("transactions"))
            .and_then(|txs| txs.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|result| utils::deserialize_transaction_json(result.clone()).ok())
                    .collect()
            })
            .ok_or_else(|| {
                RpcStateError::RpcObjectHasNoField("block_info".into(), "transactions".into())
            })?;

        Ok(RpcBlockInfo {
            block_number: BlockNumber(
                block_info
                    .get("result")
                    .and_then(|result| result.get("block_number"))
                    .and_then(|v| v.to_string().parse::<u64>().ok())
                    .ok_or_else(|| {
                        RpcStateError::RpcObjectHasNoField(
                            "block_info".into(),
                            "block_number".into(),
                        )
                    })?,
            ),
            block_timestamp: BlockTimestamp(
                block_info
                    .get("result")
                    .and_then(|result| result.get("timestamp"))
                    .and_then(|v| v.to_string().parse::<u64>().ok())
                    .ok_or_else(|| {
                        RpcStateError::RpcObjectHasNoField("block_info".into(), "timestamp".into())
                    })?,
            ),
            sequencer_address: ContractAddress(
                sequencer_address
                    .try_into()
                    .map_err(|_| RpcStateError::StarkFeltToParticiaKeyConversion)?,
            ),
            transactions,
        })
    }

    pub fn get_contract_class(&self, class_hash: &ClassHash) -> Option<SNContractClass> {
        self.block.to_value().ok().and_then(|block| {
            self.rpc_call_result(
                "starknet_getClass",
                &json!([block, class_hash.0.to_string()]),
            )
            .ok()
        })
    }

    pub fn get_class_hash_at(&self, contract_address: &ContractAddress) -> ClassHash {
        let hash = self
            .block
            .to_value()
            .ok()
            .and_then(|block| {
                self.rpc_call_result(
                    "starknet_getClassHashAt",
                    &json!([block, contract_address.0.key().clone().to_string()]),
                )
                .ok()
            })
            .unwrap_or_default();

        ClassHash(hash)
    }

    pub fn get_nonce_at(&self, contract_address: &ContractAddress) -> StarkFelt {
        self.block
            .to_value()
            .ok()
            .and_then(|block| {
                self.rpc_call_result(
                    "starknet_getNonce",
                    &json!([block, contract_address.0.key().clone().to_string()]),
                )
                .ok()
            })
            // When running deploy_account transactions, the nonce doesn't exist on the previous block so we return 0
            .unwrap_or_default()
    }

    pub fn get_storage_at(
        &self,
        contract_address: &ContractAddress,
        key: &StorageKey,
    ) -> StarkFelt {
        let contract_address = contract_address.0.key();
        let key = key.0.key();
        self.block
            .to_value()
            .ok()
            .and_then(|block| {
                self.rpc_call_result(
                    "starknet_getStorageAt",
                    &json!([contract_address.to_string(), key.to_string(), block]),
                )
                .ok()
            })
            .unwrap_or_default()
    }

    /// Requests the given transaction to the Feeder Gateway API.
    pub fn get_transaction_receipt(
        &self,
        hash: &TransactionHash,
    ) -> Result<RpcTransactionReceipt, RpcStateError> {
        self.rpc_call_result("starknet_getTransactionReceipt", &json!([hash.to_string()]))
            .map_err(|e| RpcStateError::RpcCall(e.to_string()))
    }
}
