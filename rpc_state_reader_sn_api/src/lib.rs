use blockifier::execution::contract_class::{
    ContractClass as BlockifierContractClass, ContractClassV0, ContractClassV0Inner,
};
use cairo_lang_starknet::casm_contract_class::CasmContractClass;
use cairo_lang_starknet::contract_class::{
    ContractClass as SierraContractClass, ContractEntryPoints,
};
use cairo_vm::types::program::Program;
use cairo_vm::vm::runners::cairo_runner::ExecutionResources as VmExecutionResources;
use core::fmt;
use dotenv::dotenv;
use serde::{Deserialize, Deserializer};
use serde_json::json;
use serde_with::{serde_as, DeserializeAs};
use starknet::core::types::ContractClass as SNContractClass;
use starknet_api::block::{BlockNumber, BlockTimestamp};
use starknet_api::core::{ChainId, ClassHash, EntryPointSelector};
use starknet_api::deprecated_contract_class::EntryPointOffset;
use starknet_api::hash::StarkFelt;
use starknet_api::serde_utils::PrefixedBytesAsHex;
use starknet_api::transaction::{InvokeTransaction, Transaction, TransactionHash};
use starknet_api::{core::ContractAddress, hash::StarkHash, state::StorageKey};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use thiserror::Error;

/// Starknet chains supported in Infura.
#[derive(Debug, Clone, Copy)]
pub enum RpcChain {
    MainNet,
    TestNet,
    TestNet2,
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

/// A [StateReader] that holds all the data in memory.
///
/// This implementation is uses HTTP requests to call the RPC endpoint,
/// using Infura.
/// In order to use it an Infura API key is necessary.
pub struct RpcState {
    /// Enum with one of the supported Infura chains/
    chain: RpcChain,
    /// Infura API key.
    api_key: String,
    /// Struct that holds information on the block where we are going to use to read the state.
    block: BlockValue,
}

#[derive(Debug, Error)]
enum RpcError {
    #[error("RPC call failed with error: {0}")]
    RpcCall(String),
    #[error("Request failed with error: {0}")]
    Request(String),
    #[error("Failed to cast from: {0} to: {1} with error: {2}")]
    Cast(String, String, String),
}

/// [`BlockValue`] is an Enum that represent which block we are going to use to retrieve information.
#[allow(dead_code)]
pub enum BlockValue {
    /// String one of: ["latest", "pending"]
    Tag(serde_json::Value),
    /// Integer
    Number(serde_json::Value),
    /// String with format: 0x{felt252}
    Hash(serde_json::Value),
}

impl BlockValue {
    fn to_value(&self) -> serde_json::Value {
        match self {
            BlockValue::Tag(block_tag) => block_tag.clone(),
            BlockValue::Number(block_number) => json!({ "block_number": block_number }),
            BlockValue::Hash(block_hash) => json!({ "block_hash": block_hash }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RpcResponseContractClass {
    result: SNContractClass,
}

// We use this new struct to cast the string that contains a [`StarkFelt`] in hex to a [`StarkFelt`]
struct StarkFeltHex;

impl<'de> DeserializeAs<'de, StarkFelt> for StarkFeltHex {
    fn deserialize_as<D>(deserializer: D) -> Result<StarkFelt, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: PrefixedBytesAsHex<32> = PrefixedBytesAsHex::deserialize(deserializer)?;
        Ok(bytes.try_into().unwrap())
    }
}

#[serde_as]
#[derive(Debug, Deserialize)]
struct RpcResponseFelt {
    #[serde_as(as = "StarkFeltHex")]
    result: StarkFelt,
}

pub struct RpcBlockInfo {
    /// The sequence number of the last block created.
    pub block_number: BlockNumber,
    /// Timestamp of the beginning of the last block creation attempt.
    pub block_timestamp: BlockTimestamp,
    /// The sequencer address of this block.
    pub sequencer_address: ContractAddress,
}

type RpcResponse = ureq::Response;

impl RpcState {
    pub fn new(chain: RpcChain, block: BlockValue) -> Self {
        if env::var("INFURA_API_KEY").is_err() {
            dotenv().expect("Missing .env file");
        }
        Self {
            chain,
            api_key: env::var("INFURA_API_KEY")
                .expect("Missing API Key in environment: INFURA_API_KEY"),
            block,
        }
    }

    fn rpc_call<T: for<'a> Deserialize<'a>>(
        &self,
        params: &serde_json::Value,
    ) -> Result<T, RpcError> {
        Self::deserialize_call(self.rpc_call_no_deserialize(params)?)
    }

    fn rpc_call_no_deserialize(&self, params: &serde_json::Value) -> Result<RpcResponse, RpcError> {
        ureq::post(&format!(
            "https://{}.infura.io/v3/{}",
            self.chain, self.api_key
        ))
        .set("Content-Type", "application/json")
        .set("accept", "application/json")
        .send_json(params)
        .map_err(|err| RpcError::Request(err.to_string()))
    }

    fn deserialize_call<T: for<'a> Deserialize<'a>>(response: RpcResponse) -> Result<T, RpcError> {
        let response = response.into_string().map_err(|err| {
            RpcError::Cast("Response".to_owned(), "String".to_owned(), err.to_string())
        })?;
        serde_json::from_str(&response).map_err(|err| RpcError::RpcCall(err.to_string()))
    }
}

#[derive(Debug)]
pub struct TransactionTrace {
    pub validate_invocation: RpcCallInfo,
    pub function_invocation: RpcCallInfo,
    pub fee_transfer_invocation: RpcCallInfo,
    pub signature: Vec<StarkFelt>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct RpcExecutionResources {
    pub n_steps: usize,
    pub n_memory_holes: usize,
    pub builtin_instance_counter: HashMap<String, usize>,
}

#[derive(Debug)]
pub struct RpcCallInfo {
    pub execution_resources: VmExecutionResources,
    pub retdata: Option<Vec<StarkFelt>>,
    pub calldata: Option<Vec<StarkFelt>>,
    pub internal_calls: Vec<RpcCallInfo>,
}

impl<'de> Deserialize<'de> for RpcCallInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: serde_json::Value = Deserialize::deserialize(deserializer)?;

        // Parse execution_resources
        let execution_resources_value = value["execution_resources"].clone();

        let execution_resources = VmExecutionResources {
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
        let retdata = serde_json::from_value(retdata_value).unwrap();

        // Parse calldata
        let calldata_value = value["calldata"].clone();
        let calldata = serde_json::from_value(calldata_value).unwrap();

        // Parse internal calls
        let internal_calls_value = value["internal_calls"].clone();
        let mut internal_calls = vec![];

        for call in internal_calls_value.as_array().unwrap() {
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

impl<'de> Deserialize<'de> for TransactionTrace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: serde_json::Value = Deserialize::deserialize(deserializer)?;

        let validate_invocation = value["validate_invocation"].clone();
        let function_invocation = value["function_invocation"].clone();
        let fee_transfer_invocation = value["fee_transfer_invocation"].clone();
        let signature_value = value["signature"].clone();

        Ok(TransactionTrace {
            validate_invocation: serde_json::from_value(validate_invocation)
                .map_err(serde::de::Error::custom)?,
            function_invocation: serde_json::from_value(function_invocation)
                .map_err(serde::de::Error::custom)?,
            fee_transfer_invocation: serde_json::from_value(fee_transfer_invocation)
                .map_err(serde::de::Error::custom)?,
            signature: serde_json::from_value(signature_value).map_err(serde::de::Error::custom)?,
        })
    }
}

impl RpcState {
    /// Requests the transaction trace to the Feeder Gateway API.
    /// It's useful for testing the transaction outputs like:
    /// - execution resources
    /// - actual fee
    /// - events
    /// - return data
    pub fn get_transaction_trace(&self, hash: TransactionHash) -> TransactionTrace {
        let chain_name = self.get_chain_name();
        let response = ureq::get(&format!(
            "https://{}.starknet.io/feeder_gateway/get_transaction_trace",
            chain_name
        ))
        .query("transactionHash", &hash.0.to_string())
        .call()
        .unwrap();

        serde_json::from_str(&response.into_string().unwrap()).unwrap()
    }

    /// Requests the given transaction to the Feeder Gateway API.
    pub fn get_transaction(&self, hash: &TransactionHash) -> Transaction {
        let params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getTransactionByHash",
            "params": [hash.to_string()],
            "id": 1
        });
        let result = self.rpc_call::<serde_json::Value>(&params).unwrap()["result"].clone();

        match result["type"].as_str().unwrap() {
            "INVOKE" => match result["version"].as_str().unwrap() {
                "0x0" => Transaction::Invoke(InvokeTransaction::V0(
                    serde_json::from_value(result).unwrap(),
                )),
                "0x1" => Transaction::Invoke(InvokeTransaction::V1(
                    serde_json::from_value(result).unwrap(),
                )),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    pub fn get_chain_name(&self) -> ChainId {
        ChainId(match self.chain {
            RpcChain::MainNet => "alpha-mainnet".to_string(),
            RpcChain::TestNet => "alpha4".to_string(),
            RpcChain::TestNet2 => "alpha4-2".to_string(),
        })
    }

    pub fn get_block_info(&self) -> RpcBlockInfo {
        let get_block_info_params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getBlockWithTxHashes",
            "params": [self.block.to_value()],
            "id": 1
        });

        let block_info: serde_json::Value = self.rpc_call(&get_block_info_params).unwrap();
        let sequencer_address: StarkFelt =
            serde_json::from_value(block_info["result"]["sequencer_address"].clone()).unwrap();

        RpcBlockInfo {
            block_number: BlockNumber(
                block_info["result"]["block_number"]
                    .to_string()
                    .parse::<u64>()
                    .unwrap(),
            ),
            block_timestamp: BlockTimestamp(
                block_info["result"]["timestamp"]
                    .to_string()
                    .parse::<u64>()
                    .unwrap(),
            ),
            sequencer_address: ContractAddress(sequencer_address.try_into().unwrap()),
        }
    }

    pub fn get_contract_class(
        &self,
        class_hash: &starknet_api::core::ClassHash,
    ) -> BlockifierContractClass {
        let params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getClass",
            "params": [self.block.to_value(), class_hash.0.to_string()],
            "id": 1
        });

        let response = self
            .rpc_call::<RpcResponseContractClass>(&params)
            .unwrap()
            .result;

        match response {
            SNContractClass::Legacy(compressed_legacy_cc) => {
                let as_str = utils::decode_reader(compressed_legacy_cc.program).unwrap();
                let program = Program::from_bytes(as_str.as_bytes(), None).unwrap();
                let entry_points_by_type = utils::map_entry_points_by_type_legacy(
                    compressed_legacy_cc.entry_points_by_type,
                );
                let inner = Arc::new(ContractClassV0Inner {
                    program,
                    entry_points_by_type,
                });
                BlockifierContractClass::V0(ContractClassV0(inner))
            }
            SNContractClass::Sierra(flattened_sierra_cc) => {
                let middle_sierra: utils::MiddleSierraContractClass = {
                    let v = serde_json::to_value(flattened_sierra_cc).unwrap();
                    serde_json::from_value(v).unwrap()
                };
                let sierra_cc = SierraContractClass {
                    sierra_program: middle_sierra.sierra_program,
                    contract_class_version: middle_sierra.contract_class_version,
                    entry_points_by_type: middle_sierra.entry_points_by_type,
                    sierra_program_debug_info: None,
                    abi: None,
                };
                let casm_cc = CasmContractClass::from_contract_class(sierra_cc, false).unwrap();
                BlockifierContractClass::V1(casm_cc.try_into().unwrap())
            }
        }
    }

    pub fn get_class_hash_at(&self, contract_address: &ContractAddress) -> ClassHash {
        let params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getClassHashAt",
            "params": [self.block.to_value(), contract_address.0.key().clone().to_string()],
            "id": 1
        });

        let response: RpcResponseFelt = self.rpc_call(&params).unwrap();

        ClassHash(response.result)
    }

    pub fn get_nonce_at(&self, contract_address: &ContractAddress) -> StarkFelt {
        let params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getNonce",
            "params": [self.block.to_value(), contract_address.0.key().clone().to_string()],
            "id": 1
        });

        let resp: RpcResponseFelt = self.rpc_call(&params).unwrap();

        resp.result
    }

    fn get_storage_at(&self, contract_address: &ContractAddress, key: &StorageKey) -> StarkFelt {
        let contract_address = contract_address.0.key();
        let key = key.0.key();
        let params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getStorageAt",
            "params": [contract_address.to_string(),
            key.to_string(), self.block.to_value()],
            "id": 1
        });

        let resp: RpcResponseFelt = self.rpc_call(&params).unwrap();

        resp.result
    }

    /// Requests the given transaction to the Feeder Gateway API.
    pub fn get_transaction_receipt(&self, hash: &TransactionHash) -> Transaction {
        let params = ureq::json!({
            "jsonrpc": "2.0",
            "method": "starknet_getTransactionReceipt",
            "params": [hash.to_string()],
            "id": 1
        });
        let result = self.rpc_call::<serde_json::Value>(&params).unwrap()["result"].clone();

        match result["type"].as_str().unwrap() {
            "INVOKE" => match result["version"].as_str().unwrap() {
                "0x0" => Transaction::Invoke(InvokeTransaction::V0(
                    serde_json::from_value(result).unwrap(),
                )),
                "0x1" => Transaction::Invoke(InvokeTransaction::V1(
                    serde_json::from_value(result).unwrap(),
                )),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }
}

mod utils {
    use std::io::{self, Read};

    use cairo_lang_utils::bigint::BigUintAsHex;
    use starknet::core::types::{LegacyContractEntryPoint, LegacyEntryPointsByType};
    use starknet_api::deprecated_contract_class::{EntryPoint, EntryPointType};

    use super::*;

    #[derive(Debug, Deserialize)]
    pub struct MiddleSierraContractClass {
        pub sierra_program: Vec<BigUintAsHex>,
        pub contract_class_version: String,
        pub entry_points_by_type: ContractEntryPoints,
    }

    pub(crate) fn map_entry_points_by_type_legacy(
        entry_points_by_type: LegacyEntryPointsByType,
    ) -> HashMap<EntryPointType, Vec<EntryPoint>> {
        let entry_types_to_points = HashMap::from([
            (
                EntryPointType::Constructor,
                entry_points_by_type.constructor,
            ),
            (EntryPointType::External, entry_points_by_type.external),
            (EntryPointType::L1Handler, entry_points_by_type.l1_handler),
        ]);

        let to_contract_entry_point = |entrypoint: &LegacyContractEntryPoint| -> EntryPoint {
            let felt: StarkFelt = StarkHash::new(entrypoint.selector.to_bytes_be()).unwrap();
            EntryPoint {
                offset: EntryPointOffset(entrypoint.offset as usize),
                selector: EntryPointSelector(felt),
            }
        };

        let mut entry_points_by_type_map = HashMap::new();
        for (entry_point_type, entry_points) in entry_types_to_points.into_iter() {
            let values = entry_points
                .iter()
                .map(to_contract_entry_point)
                .collect::<Vec<_>>();
            entry_points_by_type_map.insert(entry_point_type, values);
        }

        entry_points_by_type_map
    }

    use flate2::bufread;
    // Uncompresses a Gz Encoded vector of bytes and returns a string or error
    // Here &[u8] implements BufRead
    pub(crate) fn decode_reader(bytes: Vec<u8>) -> io::Result<String> {
        let mut gz = bufread::GzDecoder::new(&bytes[..]);
        let mut s = String::new();
        gz.read_to_string(&mut s)?;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use starknet_api::{
        core::{ClassHash, PatriciaKey},
        hash::StarkFelt,
        stark_felt,
    };

    use super::*;

    /// A utility macro to create a [`PatriciaKey`] from a hex string / unsigned integer representation.
    /// Imported from starknet_api
    macro_rules! patricia_key {
        ($s:expr) => {
            PatriciaKey::try_from(StarkHash::try_from($s).unwrap()).unwrap()
        };
    }

    /// A utility macro to create a [`ClassHash`] from a hex string / unsigned integer representation.
    /// Imported from starknet_api
    macro_rules! class_hash {
        ($s:expr) => {
            ClassHash(StarkHash::try_from($s).unwrap())
        };
    }

    /// A utility macro to create a [`ContractAddress`] from a hex string / unsigned integer
    /// representation.
    /// Imported from starknet_api
    macro_rules! contract_address {
        ($s:expr) => {
            ContractAddress(patricia_key!($s))
        };
    }

    #[test]
    fn test_get_contract_class_cairo1() {
        let rpc_state = RpcState::new(
            RpcChain::MainNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );

        let class_hash =
            class_hash!("0298e56befa6d1446b86ed5b900a9ba51fd2faa683cd6f50e8f833c0fb847216");
        // This belongs to
        // https://starkscan.co/class/0x0298e56befa6d1446b86ed5b900a9ba51fd2faa683cd6f50e8f833c0fb847216
        // which is cairo1.0

        rpc_state.get_contract_class(&class_hash);
    }

    #[test]
    fn test_get_contract_class_cairo0() {
        let rpc_state = RpcState::new(
            RpcChain::MainNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );

        let class_hash =
            class_hash!("025ec026985a3bf9d0cc1fe17326b245dfdc3ff89b8fde106542a3ea56c5a918");
        rpc_state.get_contract_class(&class_hash);
    }

    #[test]
    fn test_get_class_hash_at() {
        let rpc_state = RpcState::new(
            RpcChain::MainNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );
        let address =
            contract_address!("00b081f7ba1efc6fe98770b09a827ae373ef2baa6116b3d2a0bf5154136573a9");

        assert_eq!(
            rpc_state.get_class_hash_at(&address),
            class_hash!("025ec026985a3bf9d0cc1fe17326b245dfdc3ff89b8fde106542a3ea56c5a918")
        );
    }

    #[test]
    fn test_get_nonce_at() {
        let rpc_state = RpcState::new(
            RpcChain::TestNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );
        // Contract deployed by xqft which will not be used again, so nonce changes will not break
        // this test.
        let address =
            contract_address!("07185f2a350edcc7ea072888edb4507247de23e710cbd56084c356d265626bea");
        assert_eq!(rpc_state.get_nonce_at(&address), stark_felt!("0x0"));
    }

    #[test]
    fn test_get_storage_at() {
        let rpc_state = RpcState::new(
            RpcChain::MainNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );
        let address =
            contract_address!("00b081f7ba1efc6fe98770b09a827ae373ef2baa6116b3d2a0bf5154136573a9");
        let key = StorageKey(patricia_key!(0u128));

        assert_eq!(rpc_state.get_storage_at(&address, &key), stark_felt!("0x0"));
    }

    #[test]
    fn test_get_transaction() {
        let rpc_state = RpcState::new(
            RpcChain::MainNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );
        let tx_hash = TransactionHash(stark_felt!(
            "06da92cfbdceac5e5e94a1f40772d6c79d34f011815606742658559ec77b6955"
        ));

        rpc_state.get_transaction(&tx_hash);
    }

    #[test]
    fn test_get_block_info() {
        let rpc_state = RpcState::new(
            RpcChain::MainNet,
            BlockValue::Tag(serde_json::to_value("latest").unwrap()),
        );

        rpc_state.get_block_info();
    }

    // Tested with the following query to the Feeder Gateway API:
    // https://alpha4-2.starknet.io/feeder_gateway/get_transaction_trace?transactionHash=0x019feb888a2d53ffddb7a1750264640afab8e9c23119e648b5259f1b5e7d51bc
    #[test]
    fn test_get_transaction_trace() {
        let rpc_state = RpcState::new(
            RpcChain::TestNet2,
            BlockValue::Number(serde_json::to_value(838683).unwrap()),
        );

        let tx_hash = TransactionHash(stark_felt!(
            "19feb888a2d53ffddb7a1750264640afab8e9c23119e648b5259f1b5e7d51bc"
        ));

        let tx_trace = rpc_state.get_transaction_trace(tx_hash);

        assert_eq!(
            tx_trace.signature,
            vec![
                stark_felt!("ffab1c47d8d5e5b76bdcc4af79e98205716c36b440f20244c69599a91ace58"),
                stark_felt!("6aa48a0906c9c1f7381c1a040c043b649eeac1eea08f24a9d07813f6b1d05fe"),
            ]
        );

        assert_eq!(
            tx_trace.validate_invocation.calldata,
            Some(vec![
                stark_felt!("1"),
                stark_felt!("690c876e61beda61e994543af68038edac4e1cb1990ab06e52a2d27e56a1232"),
                stark_felt!("1f24f689ced5802b706d7a2e28743fe45c7bfa37431c97b1c766e9622b65573"),
                stark_felt!("0"),
                stark_felt!("9"),
                stark_felt!("9"),
                stark_felt!("4"),
                stark_felt!("4254432d55534443"),
                stark_felt!("f02e7324ecbd65ce267"),
                stark_felt!("5754492d55534443"),
                stark_felt!("8e13050d06d8f514c"),
                stark_felt!("4554482d55534443"),
                stark_felt!("f0e4a142c3551c149d"),
                stark_felt!("4a50592d55534443"),
                stark_felt!("38bd34c31a0a5c"),
            ])
        );
        assert_eq!(tx_trace.validate_invocation.retdata, Some(vec![]));
        assert_eq!(
            tx_trace.validate_invocation.execution_resources,
            VmExecutionResources {
                n_steps: 790,
                n_memory_holes: 51,
                builtin_instance_counter: HashMap::from([
                    ("range_check_builtin".to_string(), 20),
                    ("ecdsa_builtin".to_string(), 1),
                    ("pedersen_builtin".to_string(), 2),
                ]),
            }
        );
        assert_eq!(tx_trace.validate_invocation.internal_calls.len(), 1);

        assert_eq!(
            tx_trace.function_invocation.calldata,
            Some(vec![
                stark_felt!("1"),
                stark_felt!("690c876e61beda61e994543af68038edac4e1cb1990ab06e52a2d27e56a1232"),
                stark_felt!("1f24f689ced5802b706d7a2e28743fe45c7bfa37431c97b1c766e9622b65573"),
                stark_felt!("0"),
                stark_felt!("9"),
                stark_felt!("9"),
                stark_felt!("4"),
                stark_felt!("4254432d55534443"),
                stark_felt!("f02e7324ecbd65ce267"),
                stark_felt!("5754492d55534443"),
                stark_felt!("8e13050d06d8f514c"),
                stark_felt!("4554482d55534443"),
                stark_felt!("f0e4a142c3551c149d"),
                stark_felt!("4a50592d55534443"),
                stark_felt!("38bd34c31a0a5c"),
            ])
        );
        assert_eq!(
            tx_trace.function_invocation.retdata,
            Some(vec![0u128.into()])
        );
        assert_eq!(
            tx_trace.function_invocation.execution_resources,
            VmExecutionResources {
                n_steps: 2808,
                n_memory_holes: 136,
                builtin_instance_counter: HashMap::from([
                    ("range_check_builtin".to_string(), 49),
                    ("pedersen_builtin".to_string(), 14),
                ]),
            }
        );
        assert_eq!(tx_trace.function_invocation.internal_calls.len(), 1);
        assert_eq!(
            tx_trace.function_invocation.internal_calls[0]
                .internal_calls
                .len(),
            1
        );
        assert_eq!(
            tx_trace.function_invocation.internal_calls[0].internal_calls[0]
                .internal_calls
                .len(),
            7
        );

        assert_eq!(
            tx_trace.fee_transfer_invocation.calldata,
            Some(vec![
                stark_felt!("1176a1bd84444c89232ec27754698e5d2e7e1a7f1539f12027f28b23ec9f3d8"),
                stark_felt!("2b0322a23ba4"),
                stark_felt!("0"),
            ])
        );
        assert_eq!(
            tx_trace.fee_transfer_invocation.retdata,
            Some(vec![1u128.into()])
        );
        assert_eq!(
            tx_trace.fee_transfer_invocation.execution_resources,
            VmExecutionResources {
                n_steps: 586,
                n_memory_holes: 42,
                builtin_instance_counter: HashMap::from([
                    ("range_check_builtin".to_string(), 21),
                    ("pedersen_builtin".to_string(), 4),
                ]),
            }
        );
        assert_eq!(tx_trace.fee_transfer_invocation.internal_calls.len(), 1);
    }
}

mod blockifier_transaction_tests {
    use blockifier::{
        block_context::BlockContext,
        execution::contract_class::ContractClass,
        state::{
            cached_state::{CachedState, GlobalContractCache},
            state_api::{StateReader, StateResult},
        },
        transaction::{
            account_transaction::AccountTransaction,
            objects::TransactionExecutionInfo,
            transactions::{ExecutableTransaction, InvokeTransaction},
        },
    };
    use starknet_api::{
        contract_address,
        core::{CompiledClassHash, Nonce, PatriciaKey},
        patricia_key, stark_felt,
        transaction::TransactionHash,
    };

    use super::*;

    pub struct RpcStateReader(RpcState);

    impl StateReader for RpcStateReader {
        fn get_storage_at(
            &mut self,
            contract_address: ContractAddress,
            key: StorageKey,
        ) -> StateResult<StarkFelt> {
            Ok(self.0.get_storage_at(&contract_address, &key))
        }

        fn get_nonce_at(&mut self, contract_address: ContractAddress) -> StateResult<Nonce> {
            Ok(Nonce(self.0.get_nonce_at(&contract_address)))
        }

        fn get_class_hash_at(
            &mut self,
            contract_address: ContractAddress,
        ) -> StateResult<ClassHash> {
            Ok(self.0.get_class_hash_at(&contract_address))
        }

        /// Returns the contract class of the given class hash.
        fn get_compiled_contract_class(
            &mut self,
            class_hash: &ClassHash,
        ) -> StateResult<ContractClass> {
            Ok(self.0.get_contract_class(class_hash))
        }

        /// Returns the compiled class hash of the given class hash.
        fn get_compiled_class_hash(
            &mut self,
            class_hash: ClassHash,
        ) -> StateResult<CompiledClassHash> {
            Ok(CompiledClassHash(
                self.0
                    .get_class_hash_at(&ContractAddress(class_hash.0.try_into().unwrap()))
                    .0,
            ))
        }
    }

    #[allow(unused)]
    pub fn execute_tx(
        tx_hash: &str,
        network: RpcChain,
        block_number: u64,
        gas_price: u128,
    ) -> (TransactionExecutionInfo, TransactionTrace) {
        let tx_hash = tx_hash.strip_prefix("0x").unwrap();

        // Instantiate the RPC StateReader and the CachedState
        let block = BlockValue::Number(serde_json::to_value(block_number).unwrap());
        let rpc_reader = RpcStateReader(RpcState::new(network, block));

        // Get values for block context before giving ownership of the reader
        let chain_id = rpc_reader.0.get_chain_name();
        let RpcBlockInfo {
            block_number,
            block_timestamp,
            sequencer_address,
        } = rpc_reader.0.get_block_info();

        // Get transaction before giving ownership of the reader
        let tx_hash = TransactionHash(stark_felt!(tx_hash));
        let sn_api_tx = rpc_reader.0.get_transaction(&tx_hash);

        let trace = rpc_reader.0.get_transaction_trace(tx_hash);

        // Create state from RPC reader
        let global_cache = GlobalContractCache::default();
        let mut state = CachedState::new(rpc_reader, global_cache);

        let fee_token_address =
            contract_address!("049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7");

        const N_STEPS_FEE_WEIGHT: f64 = 0.01;
        let vm_resource_fee_cost = Arc::new(HashMap::from([
            ("n_steps".to_string(), N_STEPS_FEE_WEIGHT),
            ("output_builtin".to_string(), 0.0),
            ("pedersen_builtin".to_string(), N_STEPS_FEE_WEIGHT * 32.0),
            ("range_check_builtin".to_string(), N_STEPS_FEE_WEIGHT * 16.0),
            ("ecdsa_builtin".to_string(), N_STEPS_FEE_WEIGHT * 2048.0),
            ("bitwise_builtin".to_string(), N_STEPS_FEE_WEIGHT * 64.0),
            ("ec_op_builtin".to_string(), N_STEPS_FEE_WEIGHT * 1024.0),
            ("poseidon_builtin".to_string(), N_STEPS_FEE_WEIGHT * 32.0),
            (
                "segment_arena_builtin".to_string(),
                N_STEPS_FEE_WEIGHT * 10.0,
            ),
            ("keccak_builtin".to_string(), N_STEPS_FEE_WEIGHT * 2048.0), // 2**11
        ]));

        let block_context = BlockContext {
            chain_id,
            block_number,
            block_timestamp,
            sequencer_address,
            fee_token_address,
            vm_resource_fee_cost,
            gas_price,
            invoke_tx_max_n_steps: 1_000_000,
            validate_max_n_steps: 1_000_000,
            max_recursion_depth: 500,
        };

        // Map starknet_api transaction to blockifier's
        let blockifier_tx = match sn_api_tx {
            Transaction::Invoke(tx) => {
                let invoke = InvokeTransaction { tx, tx_hash };
                AccountTransaction::Invoke(invoke)
            }
            _ => unimplemented!(),
        };

        (
            blockifier_tx
                .execute(&mut state, &block_context, true, true)
                .unwrap(),
            trace,
        )
    }

    #[cfg(test)]
    mod test {
        use blockifier::execution::entry_point::CallInfo;

        use super::*;

        #[test]
        #[ignore = "working on fixes"]
        fn test_recent_tx() {
            let (tx_info, trace) = execute_tx(
                "0x05d200ef175ba15d676a68b36f7a7b72c17c17604eda4c1efc2ed5e4973e2c91",
                RpcChain::MainNet,
                169928,
                17110275391107,
            );

            let TransactionExecutionInfo {
                execute_call_info,
                actual_fee,
                ..
            } = tx_info;

            let CallInfo {
                vm_resources,
                inner_calls,
                ..
            } = execute_call_info.unwrap();

            assert_eq!(vm_resources, trace.function_invocation.execution_resources);
            assert_eq!(
                inner_calls.len(),
                trace.function_invocation.internal_calls.len()
            );

            assert_eq!(actual_fee.0, 5728510166928);
        }
    }
}
