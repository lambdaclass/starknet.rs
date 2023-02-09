use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader},
    path::PathBuf,
};

use cairo_rs::{
    serde::deserialize_program::{
        deserialize_array_of_bigint_hex, deserialize_felt_hex, Attribute, HintParams, Identifier,
        ReferenceManager,
    },
    types::{
        errors::program_errors::ProgramError, program::Program, relocatable::MaybeRelocatable,
    },
    utils::is_subsequence,
};
use felt::{Felt, PRIME_STR};
use starknet_api::state::{ContractClassAbiEntry, EntryPoint};

use crate::{
    business_logic::execution::execution_errors::ExecutionError,
    core::errors::state_errors::StateError, public::abi::AbiType,
};
use serde::{Deserialize, Serialize};

use super::contract_class_errors::ContractClassError;

pub(crate) const SUPPORTED_BUILTINS: [&str; 5] =
    ["pedersen", "range_check", "ecdsa", "bitwise", "ec_op"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum EntryPointType {
    External,
    L1Handler,
    Constructor,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Default, PartialEq)]
pub struct ContractEntryPoint {
    pub(crate) selector: Felt,
    pub(crate) offset: Felt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractClass {
    pub(crate) program: Program,
    pub(crate) entry_points_by_type: HashMap<EntryPointType, Vec<ContractEntryPoint>>,
    pub(crate) abi: Option<AbiType>,
}

impl From<&ContractEntryPoint> for Vec<MaybeRelocatable> {
    fn from(entry_point: &ContractEntryPoint) -> Self {
        vec![
            MaybeRelocatable::from(entry_point.selector.clone()),
            MaybeRelocatable::from(entry_point.offset.clone()),
        ]
    }
}

impl From<starknet_api::state::ContractClass> for ContractClass {
    fn from(contract_class: starknet_api::state::ContractClass) -> Self {
        let program = to_cairo_runner_program(&contract_class.program).unwrap();
        let entry_points_by_type = convert_entry_points(contract_class.entry_points_by_type);

        ContractClass {
            program,
            entry_points_by_type,
            abi: None,
        }
    }
}

impl ContractClass {
    pub(crate) fn new(
        program: Program,
        entry_points_by_type: HashMap<EntryPointType, Vec<ContractEntryPoint>>,
        abi: Option<AbiType>,
    ) -> Result<Self, ContractClassError> {
        for entry_points in entry_points_by_type.values() {
            let mut index = 1;
            while let Some(entry_point) = entry_points.get(index) {
                if entry_point.selector > entry_points[index - 1].selector {
                    return Err(ContractClassError::EntrypointError(entry_points.clone()));
                }
                index += 1;
            }
        }

        Ok(ContractClass {
            program,
            entry_points_by_type,
            abi,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), ContractClassError> {
        let supported_builtins: Vec<String> = SUPPORTED_BUILTINS
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        if !is_subsequence(&self.program.builtins, &supported_builtins) {
            return Err(ContractClassError::DisorderedBuiltins);
        };

        if self.program.prime != *PRIME_STR {
            return Err(ContractClassError::InvalidPrime(
                self.program.prime.clone(),
                PRIME_STR.to_string(),
            ));
        };
        Ok(())
    }
}

// -------------------
//  Helper Functions
// -------------------

impl TryFrom<PathBuf> for ContractClass {
    type Error = io::Error;

    fn try_from(path: PathBuf) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let raw_contract_class: starknet_api::state::ContractClass =
            serde_json::from_reader(reader)?;

        let contract_class = ContractClass::from(raw_contract_class);
        Ok(contract_class)
    }
}

fn convert_entry_points(
    entry_points: HashMap<starknet_api::state::EntryPointType, Vec<EntryPoint>>,
) -> HashMap<EntryPointType, Vec<ContractEntryPoint>> {
    let mut converted_entries: HashMap<EntryPointType, Vec<ContractEntryPoint>> = HashMap::new();
    for (entry_type, vec) in entry_points {
        let en_type = match entry_type {
            starknet_api::state::EntryPointType::Constructor => {
                crate::services::api::contract_class::EntryPointType::Constructor
            }
            starknet_api::state::EntryPointType::External => {
                crate::services::api::contract_class::EntryPointType::External
            }
            starknet_api::state::EntryPointType::L1Handler => {
                crate::services::api::contract_class::EntryPointType::L1Handler
            }
        };

        let contracts_entry_points = vec
            .into_iter()
            .map(|e| {
                let selector = Felt::from_bytes_be(&e.selector.0.bytes());
                let offset = e.offset.0.into();
                ContractEntryPoint { selector, offset }
            })
            .collect::<Vec<ContractEntryPoint>>();

        converted_entries.insert(en_type, contracts_entry_points);
    }

    converted_entries
}

fn to_cairo_runner_program(
    program: &starknet_api::state::Program,
) -> Result<Program, ProgramError> {
    let program = program.clone();
    let identifiers = serde_json::from_value::<HashMap<String, Identifier>>(program.identifiers)?;

    let start = match identifiers.get("__main__.__start__") {
        Some(identifier) => identifier.pc,
        None => None,
    };
    let end = match identifiers.get("__main__.__end__") {
        Some(identifier) => identifier.pc,
        None => None,
    };

    Ok(Program {
        builtins: serde_json::from_value::<Vec<String>>(program.builtins)?,
        prime: deserialize_felt_hex(program.prime)?.to_string(),
        data: deserialize_array_of_bigint_hex(program.data)?,
        constants: {
            let mut constants = HashMap::new();
            for (key, value) in identifiers.iter() {
                if value.type_.as_deref() == Some("const") {
                    let value = value
                        .value
                        .clone()
                        .ok_or_else(|| ProgramError::ConstWithoutValue(key.to_owned()))?;
                    constants.insert(key.to_owned(), value);
                }
            }

            constants
        },
        main: None,
        start,
        end,
        hints: serde_json::from_value::<HashMap<usize, Vec<HintParams>>>(program.hints)?,
        reference_manager: serde_json::from_value::<ReferenceManager>(program.reference_manager)?,
        identifiers,
        error_message_attributes: serde_json::from_value::<Vec<Attribute>>(program.attributes)?
            .into_iter()
            .filter(|attr| attr.name == "error_message")
            .collect(),
        instruction_locations: None,
    })
}
