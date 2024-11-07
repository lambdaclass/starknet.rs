use std::collections::HashMap;
use std::io::{self, Read};
use std::sync::Arc;

use crate::core::contract_address::{compute_hinted_class_hash, CairoProgramToHash};
use crate::services::api::contract_class_errors::ContractClassError;
use crate::services::api::contract_classes::deprecated_contract_class::AbiType;
use crate::{ContractEntryPoint, EntryPointType};

use super::deprecated_contract_class::ContractClass;
use cairo_lang_sierra::program::Program as SierraProgram;
use cairo_lang_starknet_classes::{
    abi::Contract,
    casm_contract_class::CasmContractClass,
    contract_class::{ContractClass as SierraContractClass, ContractEntryPoints},
};
use cairo_lang_utils::bigint::BigUintAsHex;
use cairo_vm::types::program::Program;
use cairo_vm::Felt252;
use serde::{Deserialize, Serialize};
use starknet::core::types::ContractClass as StarknetRsContractClass;
use starknet::core::types::ContractClass::{Legacy, Sierra};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CompiledClass {
    Deprecated(Arc<ContractClass>),
    Casm {
        casm: Arc<CasmContractClass>,
        sierra: Option<Arc<(SierraProgram, ContractEntryPoints)>>,
    },
}

impl TryInto<CasmContractClass> for CompiledClass {
    type Error = ContractClassError;

    fn try_into(self) -> Result<CasmContractClass, ContractClassError> {
        match self {
            CompiledClass::Casm { casm, .. } => Ok((*casm).clone()),
            _ => Err(ContractClassError::NotACasmContractClass),
        }
    }
}

impl TryInto<ContractClass> for CompiledClass {
    type Error = ContractClassError;

    fn try_into(self) -> Result<ContractClass, ContractClassError> {
        match self {
            CompiledClass::Deprecated(arc) => Ok((*arc).clone()),
            _ => Err(ContractClassError::NotADeprecatedContractClass),
        }
    }
}

extern crate serde;
extern crate serde_json;

#[derive(Debug, Deserialize)]
struct MiddleSierraContractClass {
    sierra_program: Vec<BigUintAsHex>,
    sierra_program_debug_info: Option<cairo_lang_sierra::debug_info::DebugInfo>,
    contract_class_version: String,
    entry_points_by_type: ContractEntryPoints,
    #[allow(dead_code)]
    abi: serde_json::Value,
}

impl From<StarknetRsContractClass> for CompiledClass {
    fn from(starknet_rs_contract_class: StarknetRsContractClass) -> Self {
        match starknet_rs_contract_class {
            Sierra(flattened_sierra_contract_class) => {
                let abi: Option<Contract> =
                    serde_json::from_str(&flattened_sierra_contract_class.abi).unwrap();

                let v = serde_json::to_value(&flattened_sierra_contract_class).unwrap();
                let middle_sierra: MiddleSierraContractClass = serde_json::from_value(v).unwrap();

                let sierra_cc = SierraContractClass {
                    sierra_program: middle_sierra.sierra_program,
                    sierra_program_debug_info: middle_sierra.sierra_program_debug_info,
                    contract_class_version: middle_sierra.contract_class_version,
                    entry_points_by_type: middle_sierra.entry_points_by_type,
                    abi,
                };

                let sierra_arc = Arc::new((
                    sierra_cc.extract_sierra_program().unwrap(),
                    sierra_cc.entry_points_by_type.clone(),
                ));

                // TODO: from where this value must be passed? Constant?
                let max_bytecode_size = 4_089_446;
                let casm_cc =
                    CasmContractClass::from_contract_class(sierra_cc, true, max_bytecode_size)
                        .unwrap();

                CompiledClass::Casm {
                    casm: Arc::new(casm_cc),
                    sierra: Some(sierra_arc),
                }
            }
            Legacy(_deprecated_contract_class) => {
                let as_str = decode_reader(_deprecated_contract_class.program).unwrap();

                let program = Program::from_bytes(as_str.as_bytes(), None).unwrap();

                // let hinted_class_hash = compute_hinted_class_hash();
                let mut entry_points_by_type: HashMap<EntryPointType, Vec<ContractEntryPoint>> =
                    HashMap::new();

                let constructor_entries = _deprecated_contract_class
                    .entry_points_by_type
                    .clone()
                    .constructor
                    .into_iter()
                    .map(|entrypoint| {
                        ContractEntryPoint::new(
                            Felt252::from_bytes_be(&entrypoint.selector.to_bytes_be()),
                            entrypoint.offset as usize,
                        )
                    })
                    .collect::<Vec<ContractEntryPoint>>();
                entry_points_by_type.insert(EntryPointType::Constructor, constructor_entries);

                let external_entries = _deprecated_contract_class
                    .entry_points_by_type
                    .clone()
                    .external
                    .into_iter()
                    .map(|entrypoint| {
                        ContractEntryPoint::new(
                            Felt252::from_bytes_be(&entrypoint.selector.to_bytes_be()),
                            entrypoint.offset as usize,
                        )
                    })
                    .collect::<Vec<ContractEntryPoint>>();
                entry_points_by_type.insert(EntryPointType::External, external_entries);

                let l1_handler_entries = _deprecated_contract_class
                    .entry_points_by_type
                    .clone()
                    .l1_handler
                    .into_iter()
                    .map(|entrypoint| {
                        ContractEntryPoint::new(
                            Felt252::from_bytes_be(&entrypoint.selector.to_bytes_be()),
                            entrypoint.offset as usize,
                        )
                    })
                    .collect::<Vec<ContractEntryPoint>>();
                entry_points_by_type.insert(EntryPointType::L1Handler, l1_handler_entries);

                let v = serde_json::to_value(&_deprecated_contract_class.abi).unwrap();
                let abi: Option<AbiType> = serde_json::from_value(v).unwrap();

                let cairo_program_to_hash: CairoProgramToHash =
                    serde_json::from_str(as_str.as_str()).unwrap();

                let serialized_cc = SerializedContractClass {
                    program: cairo_program_to_hash,
                    entry_points_by_type: serde_json::to_value(
                        &_deprecated_contract_class.entry_points_by_type,
                    )
                    .unwrap(),
                    abi: serde_json::to_value(&_deprecated_contract_class.abi).unwrap(),
                };

                let v = serde_json::to_value(serialized_cc).unwrap();
                let hinted_class_hash = compute_hinted_class_hash(&v).unwrap();

                CompiledClass::Deprecated(Arc::new(ContractClass {
                    program,
                    entry_points_by_type,
                    abi,
                    hinted_class_hash,
                }))
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SerializedContractClass<'a> {
    /// Main program definition.
    #[serde(borrow)]
    program: CairoProgramToHash<'a>,
    entry_points_by_type: serde_json::Value,
    abi: serde_json::Value,
}

use flate2::bufread;
// Uncompresses a Gz Encoded vector of bytes and returns a string or error
// Here &[u8] implements BufRead
fn decode_reader(bytes: Vec<u8>) -> io::Result<String> {
    let mut gz = bufread::GzDecoder::new(&bytes[..]);
    let mut s = String::new();
    gz.read_to_string(&mut s)?;
    Ok(s)
}
