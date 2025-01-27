use crate::core::contract_address::compute_hinted_class_hash;
use crate::services::api::contract_class_errors::ContractClassError;
use cairo_vm::serde::deserialize_program::{
    deserialize_array_of_bigint_hex, Attribute, HintParams, Identifier, ReferenceManager,
};
use cairo_vm::types::builtin_name::BuiltinName;
use cairo_vm::types::relocatable::MaybeRelocatable;
use cairo_vm::types::{errors::program_errors::ProgramError, program::Program};
use cairo_vm::utils::PRIME_STR;
use cairo_vm::Felt252;
use core::str::FromStr;
use getset::{CopyGetters, Getters};
use serde_json::Value;
use starknet_api::deprecated_contract_class::{ContractClassAbiEntry, EntryPoint};
use std::collections::HashMap;
use std::path::Path;

pub type AbiType = Vec<ContractClassAbiEntry>;

// -------------------------------
//       Entry Point types
// -------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryPointType {
    External,
    L1Handler,
    Constructor,
}

#[derive(Clone, CopyGetters, Debug, Default, Eq, Getters, Hash, PartialEq)]
pub struct ContractEntryPoint {
    #[getset(get = "pub")]
    selector: Felt252,
    #[getset(get_copy = "pub")]
    offset: usize,
}

impl ContractEntryPoint {
    pub const fn new(selector: Felt252, offset: usize) -> ContractEntryPoint {
        ContractEntryPoint { selector, offset }
    }
}

// -------------------------------
//         From traits
// -------------------------------

impl From<&ContractEntryPoint> for Vec<MaybeRelocatable> {
    fn from(entry_point: &ContractEntryPoint) -> Self {
        vec![
            MaybeRelocatable::from(entry_point.selector),
            MaybeRelocatable::from(entry_point.offset),
        ]
    }
}

impl From<starknet_api::deprecated_contract_class::EntryPointType> for EntryPointType {
    fn from(entry_type: starknet_api::deprecated_contract_class::EntryPointType) -> Self {
        type ApiEPT = starknet_api::deprecated_contract_class::EntryPointType;
        type StarknetEPT = EntryPointType;

        match entry_type {
            ApiEPT::Constructor => StarknetEPT::Constructor,
            ApiEPT::External => StarknetEPT::External,
            ApiEPT::L1Handler => StarknetEPT::L1Handler,
        }
    }
}

// -------------------------------
//         Contract Class
// -------------------------------

#[derive(Clone, Debug, Eq, Getters, PartialEq)]
pub struct ContractClass {
    #[getset(get = "pub")]
    pub(crate) program: Program,
    #[getset(get = "pub")]
    pub(crate) hinted_class_hash: Felt252,
    #[getset(get = "pub")]
    pub(crate) entry_points_by_type: HashMap<EntryPointType, Vec<ContractEntryPoint>>,
    #[getset(get = "pub")]
    pub(crate) abi: Option<AbiType>,
}

impl ContractClass {
    pub fn new(
        program_json: Value,
        program: Program,
        entry_points_by_type: HashMap<EntryPointType, Vec<ContractEntryPoint>>,
        abi: Option<AbiType>,
    ) -> Result<Self, ContractClassError> {
        for entry_points in entry_points_by_type.values() {
            for i in 1..entry_points.len() {
                if entry_points[i - 1].selector() > entry_points[i].selector() {
                    return Err(ContractClassError::EntrypointError(entry_points.clone()));
                }
            }
        }
        let hinted_class_hash = compute_hinted_class_hash(&program_json).unwrap();
        Ok(ContractClass {
            hinted_class_hash,
            program,
            entry_points_by_type,
            abi,
        })
    }

    pub fn new_with_hinted_class_hash(
        hinted_class_hash: Felt252,
        program: Program,
        entry_points_by_type: HashMap<EntryPointType, Vec<ContractEntryPoint>>,
        abi: Option<AbiType>,
    ) -> Result<Self, ContractClassError> {
        for entry_points in entry_points_by_type.values() {
            for i in 1..entry_points.len() {
                if entry_points[i - 1].selector() > entry_points[i].selector() {
                    return Err(ContractClassError::EntrypointError(entry_points.clone()));
                }
            }
        }

        Ok(ContractClass {
            hinted_class_hash,
            program,
            entry_points_by_type,
            abi,
        })
    }

    /// Parses a [`ContractClass`] from a compiled Cairo 0 program's JSON and a class hash.
    ///
    /// This constructor avoids the need to recompute the class hash from the program JSON,
    /// but it does not verify that the given class hash is correct.
    ///
    /// This could return a wrong [`ContractClass`] if the given class hash is wrong.
    ///
    /// # Parameters
    /// - `program_json`: The JSON of the compiled Cairo 0 program.
    /// - `hinted_class_hash`: The class hash of the program.
    ///
    /// # Returns
    /// A [`ContractClass`] parsed from the given JSON and class hash.
    pub fn from_program_json_and_class_hash(
        program_json: &str,
        hinted_class_hash: Felt252,
    ) -> Result<Self, ContractClassError> {
        let contract_class: starknet_api::deprecated_contract_class::ContractClass =
            serde_json::from_str(program_json).map_err(|_| ContractClassError::ParseError)?;
        let program = to_cairo_runner_program(contract_class.program)
            .map_err(|e| ContractClassError::ProgramError(e.to_string()))?;
        let entry_points_by_type = convert_entry_points(contract_class.entry_points_by_type);
        Ok(ContractClass {
            hinted_class_hash,
            program,
            entry_points_by_type,
            abi: contract_class.abi,
        })
    }

    /// Parses a [`ContractClass`] from a compiled Cairo 0 program's JSON
    /// at the given file path.
    pub fn from_path<F>(path: F) -> Result<Self, ProgramError>
    where
        F: AsRef<Path>,
    {
        Self::from_str(std::fs::read_to_string(path)?.as_str())
    }

    /// Parses a [`ContractClass`] from a compiled Cairo 0 program's JSON
    /// given by a reader.
    pub fn from_reader<R>(mut reader: R) -> Result<Self, ProgramError>
    where
        R: std::io::Read,
    {
        let mut s = String::new();
        reader.read_to_string(&mut s)?;
        Self::from_str(s.as_str())
    }
}

// -------------------------------
//         From traits
// -------------------------------

impl FromStr for ContractClass {
    type Err = ProgramError;

    /// Parses a [`ContractClass`] from a compiled Cairo 0 program's JSON.
    fn from_str(program_json: &str) -> Result<Self, ProgramError> {
        let contract_class: starknet_api::deprecated_contract_class::ContractClass =
            serde_json::from_str(program_json)?;
        let program = to_cairo_runner_program(contract_class.program)?;
        let entry_points_by_type = convert_entry_points(contract_class.entry_points_by_type);
        let hinted_class_hash =
            compute_hinted_class_hash(&serde_json::from_str(program_json)?).unwrap();
        Ok(ContractClass {
            hinted_class_hash,
            program,
            entry_points_by_type,
            abi: contract_class.abi,
        })
    }
}

// -------------------
//  Helper Functions
// -------------------

pub(crate) fn convert_entry_points(
    entry_points: HashMap<starknet_api::deprecated_contract_class::EntryPointType, Vec<EntryPoint>>,
) -> HashMap<EntryPointType, Vec<ContractEntryPoint>> {
    let mut converted_entries: HashMap<EntryPointType, Vec<ContractEntryPoint>> = HashMap::new();
    for (entry_type, entry_points) in entry_points {
        let en_type = entry_type.into();

        let contracts_entry_points = entry_points
            .into_iter()
            .map(|e| {
                let selector = Felt252::from_bytes_be_slice(e.selector.0.bytes());
                let offset = e.offset.0;
                ContractEntryPoint::new(selector, offset)
            })
            .collect::<Vec<ContractEntryPoint>>();

        converted_entries.insert(en_type, contracts_entry_points);
    }

    converted_entries
}

pub(crate) fn to_cairo_runner_program(
    program: starknet_api::deprecated_contract_class::Program,
) -> Result<Program, ProgramError> {
    let identifiers = serde_json::from_value::<HashMap<String, Identifier>>(program.identifiers)?;

    if program.prime != *PRIME_STR {
        return Err(ProgramError::PrimeDiffers(program.prime.to_string()));
    };

    let mut error_message_attributes =
        serde_json::from_value::<Vec<Attribute>>(program.attributes).unwrap_or_default();
    error_message_attributes.retain(|attr| attr.name == "error_message");

    let program = Program::new(
        serde_json::from_value::<Vec<BuiltinName>>(program.builtins)?,
        deserialize_array_of_bigint_hex(program.data)?,
        None,
        serde_json::from_value::<HashMap<usize, Vec<HintParams>>>(program.hints)?,
        serde_json::from_value::<ReferenceManager>(program.reference_manager)?,
        identifiers,
        error_message_attributes,
        None,
    )?;

    Ok(program)
}

#[cfg(test)]
mod tests {
    use crate::core::contract_address::compute_deprecated_class_hash;

    use super::*;
    use cairo_vm::Felt252;
    use starknet_api::deprecated_contract_class::{FunctionAbiEntry, TypedParameter};

    #[test]
    fn deserialize_contract_class() {
        // This specific contract compiles with --no_debug_info
        let contract_class = ContractClass::from_path("starknet_programs/AccountPreset.json")
            .expect("should be able to read file");

        // We check only some of the attributes. Ideally we would serialize
        // and compare with original
        // TODO: add abi.
        // assert_eq!(contract_class.abi(), &None);
        assert_eq!(
            contract_class
                .program()
                .iter_builtins()
                .cloned()
                .collect::<Vec<BuiltinName>>(),
            [
                BuiltinName::pedersen,
                BuiltinName::range_check,
                BuiltinName::ecdsa,
                BuiltinName::bitwise
            ]
        );
        assert_eq!(contract_class.program().prime(), PRIME_STR);
        assert_eq!(
            contract_class
                .entry_points_by_type()
                .get(&EntryPointType::L1Handler)
                .unwrap(),
            &vec![]
        );
        assert_eq!(
            contract_class
                .entry_points_by_type()
                .get(&EntryPointType::Constructor)
                .unwrap(),
            &vec![ContractEntryPoint::new(
                Felt252::from_dec_str(
                    "1159040026212278395030414237414753050475174923702621880048416706425641521556"
                )
                .unwrap(),
                366
            )]
        );
    }

    #[test]
    fn test_compute_class_hash_0x4479c3b883b34f1eafa5065418225d78a11ee7957c371e1b285e4b77afc6dad_try_from(
    ) {
        let contract_class = ContractClass::from_path("starknet_programs/raw_contract_classes/0x4479c3b883b34f1eafa5065418225d78a11ee7957c371e1b285e4b77afc6dad.json").expect("should be able to read file");

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex("0x4479c3b883b34f1eafa5065418225d78a11ee7957c371e1b285e4b77afc6dad")
                .unwrap()
        );
    }

    #[test]
    fn parse_abi_is_correct() {
        // This specific contract compiles with --no_debug_info
        let res = ContractClass::from_path("starknet_programs/fibonacci.json");
        let contract_class = res.expect("should be able to read file");
        let expected_abi = Some(vec![ContractClassAbiEntry::Function(FunctionAbiEntry {
            name: "fib".to_string(),
            inputs: vec![
                TypedParameter {
                    name: "first_element".to_string(),
                    r#type: "felt".to_string(),
                },
                TypedParameter {
                    name: "second_element".to_string(),
                    r#type: "felt".to_string(),
                },
                TypedParameter {
                    name: "n".to_string(),
                    r#type: "felt".to_string(),
                },
            ],
            outputs: vec![TypedParameter {
                name: "res".to_string(),
                r#type: "felt".to_string(),
            }],
            state_mutability: None,
        })]);
        assert_eq!(contract_class.abi, expected_abi);
    }

    #[test]
    fn parse_without_debug_info() {
        // This specific contract compiles with --no_debug_info
        let res = ContractClass::from_path("starknet_programs/AccountPreset.json");

        let contract_class = res.expect("should be able to read file");

        let program_builtins: Vec<BuiltinName> =
            contract_class.program.iter_builtins().cloned().collect();
        assert_eq!(
            program_builtins,
            vec![
                BuiltinName::pedersen,
                BuiltinName::range_check,
                BuiltinName::ecdsa,
                BuiltinName::bitwise
            ]
        );
        assert_eq!(
            contract_class
                .entry_points_by_type
                .get(&EntryPointType::L1Handler)
                .unwrap(),
            &vec![]
        );
        assert_eq!(
            contract_class
                .entry_points_by_type
                .get(&EntryPointType::Constructor)
                .unwrap(),
            &vec![ContractEntryPoint {
                selector: Felt252::from_dec_str(
                    "1159040026212278395030414237414753050475174923702621880048416706425641521556"
                )
                .unwrap(),
                offset: 366
            }]
        );
    }

    #[test]
    fn parse_without_program_attributes() {
        // This specific contract was extracted from: https://testnet.starkscan.co/class/0x068dd0dd8a54ebdaa10563fbe193e6be1e0f7c423c0c3ce1e91c0b682a86b5f9
        let res = ContractClass::from_path(
            "starknet_programs/raw_contract_classes/program_without_attributes.json",
        );

        res.expect("should be able to read file");
    }

    #[test]
    fn parse_without_program_attributes_2() {
        // This specific contract was extracted from: https://testnet.starkscan.co/class/0x071b7f73b5e2b4f81f7cf01d4d1569ccba2921b3fa3170cf11cff3720dfe918e
        let res = ContractClass::from_path(
            "starknet_programs/raw_contract_classes/program_without_attributes_2.json",
        );

        res.expect("should be able to read file");
    }

    #[test]
    fn test_from_program_json_and_class_hash_should_equal_from_path() {
        let program_json = include_str!("../../../../starknet_programs/fibonacci.json");
        let contract_class_from_path = ContractClass::from_path("starknet_programs/fibonacci.json")
            .expect("should be able to read file");

        let contract_class_from_program_json_and_class_hash =
            ContractClass::from_program_json_and_class_hash(
                program_json,
                contract_class_from_path.hinted_class_hash,
            )
            .expect("should be able to read file");
        assert_eq!(
            contract_class_from_path,
            contract_class_from_program_json_and_class_hash
        );
    }
}
