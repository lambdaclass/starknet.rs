use crate::services::api::contract_classes::deprecated_contract_class::{
    ContractEntryPoint, EntryPointType,
};
/// Contains functionality for computing class hashes for deprecated Declare transactions
/// (ie, declarations that do not correspond to Cairo 1 contracts)
/// The code used for hinted class hash computation was extracted from the Pathfinder and xJonathanLEI implementations
/// and can be found here:
/// https://github.com/eqlabs/pathfinder/
/// https://github.com/xJonathanLEI/starknet-rs/
use crate::{
    core::errors::contract_address_errors::ContractAddressError,
    hash_utils::compute_hash_on_elements,
    services::api::contract_classes::deprecated_contract_class::ContractClass,
};
use cairo_vm::Felt252;

use serde::Serialize;
use sha3::Digest;
use std::{borrow::Cow, collections::BTreeMap, io};

/// Instead of doing a Mask with 250 bits, we are only masking the most significant byte.
pub const MASK_3: u8 = 0x03;

/// Returns the contract entry points.
fn get_contract_entry_points(
    contract_class: &ContractClass,
    entry_point_type: &EntryPointType,
) -> Result<Vec<ContractEntryPoint>, ContractAddressError> {
    let entry_points = contract_class
        .entry_points_by_type()
        .get(entry_point_type)
        .ok_or(ContractAddressError::NoneExistingEntryPointType)?;

    let program_len = contract_class.program().iter_data().count();

    for entry_point in entry_points {
        if entry_point.offset() > program_len {
            return Err(ContractAddressError::InvalidOffset(entry_point.offset()));
        }
    }
    Ok(entry_points.to_owned())
}

/// Recursively add extra spaces to Cairo named tuple representations in a JSON structure.
fn add_extra_space_to_cairo_named_tuples(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(v) => walk_array(v),
        serde_json::Value::Object(m) => walk_map(m),
        _ => {}
    }
}

/// Helper function to walk through a JSON array and apply extra space to cairo named tuples.
fn walk_array(array: &mut [serde_json::Value]) {
    for v in array.iter_mut() {
        add_extra_space_to_cairo_named_tuples(v);
    }
}

/// Helper function to walk through a JSON map and apply extra space to cairo named tuples.
fn walk_map(object: &mut serde_json::Map<String, serde_json::Value>) {
    for (k, v) in object.iter_mut() {
        match v {
            serde_json::Value::String(s) => {
                let new_value = add_extra_space_to_named_tuple_type_definition(k, s);
                if new_value.as_ref() != s {
                    *v = serde_json::Value::String(new_value.into());
                }
            }
            _ => add_extra_space_to_cairo_named_tuples(v),
        }
    }
}

/// Add extra space to named tuple type definition.
fn add_extra_space_to_named_tuple_type_definition<'a>(
    key: &str,
    value: &'a str,
) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow::*;
    match key {
        "cairo_type" | "value" => Owned(add_extra_space_before_colon(value)),
        _ => Borrowed(value),
    }
}

/// Replaces ": " with " : " and "  :" with " :" for Cairo-specific formatting.
fn add_extra_space_before_colon(v: &str) -> String {
    // This is required because if we receive an already correct ` : `, we will still
    // "repair" it to `  : ` which we then fix at the end.
    v.replace(": ", " : ").replace("  :", " :")
}
#[derive(Default)]
struct KeccakWriter(sha3::Keccak256);

impl std::io::Write for KeccakWriter {
    /// Write data into the Keccak256 hasher.
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    /// No operation is required for flushing, as we finalize after writing.
    fn flush(&mut self) -> std::io::Result<()> {
        // noop is fine, we'll finalize after the write phase
        Ok(())
    }
}

/// Starkware doesn't use compact formatting for JSON but default python formatting.
/// This is required to hash to the same value after sorted serialization.
struct PythonDefaultFormatter;

impl serde_json::ser::Formatter for PythonDefaultFormatter {
    /// Handles formatting for array values.
    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    /// Handles formatting for object keys.
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    /// Handles formatting for object values.
    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }

    /// Custom logic for writing string fragments, handling non-ASCII characters.
    #[inline]
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let mut buf = [0, 0];

        for c in fragment.chars() {
            if c.is_ascii() {
                writer.write_all(&[c as u8])?;
            } else {
                let buf = c.encode_utf16(&mut buf);
                for i in buf {
                    write!(writer, r"\u{:04x}", i)?;
                }
            }
        }

        Ok(())
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CairoContractDefinition<'a> {
    /// Contract ABI, which has no schema definition.
    pub abi: serde_json::Value,

    /// Main program definition.
    #[serde(borrow)]
    pub program: CairoProgramToHash<'a>,

    /// The contract entry points.
    ///
    /// These are left out of the re-serialized version with the ordering requirement to a
    /// Keccak256 hash.
    #[serde(skip_serializing)]
    pub entry_points_by_type: serde_json::Value,
}

// It's important that this is ordered alphabetically because the fields need to be in
// sorted order for the keccak hashed representation.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CairoProgramToHash<'a> {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attributes: Vec<serde_json::Value>,

    #[serde(borrow)]
    pub builtins: Vec<Cow<'a, str>>,

    // Added in Starknet 0.10, so we have to handle this not being present.
    #[serde(borrow, skip_serializing_if = "Option::is_none")]
    pub compiler_version: Option<Cow<'a, str>>,

    #[serde(borrow)]
    pub data: Vec<Cow<'a, str>>,

    #[serde(borrow)]
    pub debug_info: Option<&'a serde_json::value::RawValue>,

    // Important that this is ordered by the numeric keys, not lexicographically
    pub hints: BTreeMap<u64, Vec<serde_json::Value>>,

    pub identifiers: serde_json::Value,

    #[serde(borrow)]
    pub main_scope: Cow<'a, str>,

    // Unlike most other integers, this one is hex string. We don't need to interpret it,
    // it just needs to be part of the hashed output.
    #[serde(borrow)]
    pub prime: Cow<'a, str>,

    pub reference_manager: serde_json::Value,
}

/// Computes the hash of the contract class, including hints.
/// We are not supporting backward compatibility now.
pub(crate) fn compute_hinted_class_hash(
    contract_class: &serde_json::Value,
) -> Result<Felt252, ContractAddressError> {
    let program_as_string = contract_class.to_string();
    let mut cairo_program_hash: CairoContractDefinition = serde_json::from_str(&program_as_string)
        .map_err(|err| ContractAddressError::InvalidProgramJson(err.to_string()))?;

    cairo_program_hash
        .program
        .attributes
        .iter_mut()
        .try_for_each(|attr| -> anyhow::Result<()> {
            let vals = attr
                .as_object_mut()
                .ok_or(ContractAddressError::InvalidProgramJson(
                    "value not object".to_string(),
                ))?;

            match vals.get_mut("accessible_scopes") {
                Some(serde_json::Value::Array(array)) => {
                    if array.is_empty() {
                        vals.remove("accessible_scopes");
                    }
                }
                Some(_other) => {
                    anyhow::bail!(
                        r#"A program's attribute["accessible_scopes"] was not an array type."#
                    );
                }
                None => {}
            }
            // We don't know what this type is supposed to be, but if its missing it is null.
            if vals.get("flow_tracking_data") == Some(&serde_json::Value::Null) {
                vals.remove("flow_tracking_data");
            }

            Ok(())
        })
        .map_err(|err| ContractAddressError::InvalidProgramJson(err.to_string()))?;
    // Handle a backwards compatibility hack which is required if compiler_version is not present.
    // See `insert_space` for more details.
    if cairo_program_hash.program.compiler_version.is_none() {
        add_extra_space_to_cairo_named_tuples(&mut cairo_program_hash.program.identifiers);
        add_extra_space_to_cairo_named_tuples(&mut cairo_program_hash.program.reference_manager);
    }
    let mut ser =
        serde_json::Serializer::with_formatter(KeccakWriter::default(), PythonDefaultFormatter);

    cairo_program_hash
        .serialize(&mut ser)
        .map_err(|err| ContractAddressError::InvalidProgramJson(err.to_string()))?;

    let KeccakWriter(hash) = ser.into_inner();
    Ok(truncated_keccak(<[u8; 32]>::from(hash.finalize())))
}

/// Truncate the given Keccak hash to fit within Felt252's constraints.
pub(crate) fn truncated_keccak(mut plain: [u8; 32]) -> Felt252 {
    // python code masks with (2**250 - 1) which starts 0x03 and is followed by 31 0xff in be
    // truncation is needed not to overflow the field element.
    plain[0] &= MASK_3;
    Felt252::from_bytes_be(&plain)
}

/// Returns the hashed entry points of a contract class.
fn get_contract_entry_points_hashed(
    contract_class: &ContractClass,
    entry_point_type: &EntryPointType,
) -> Result<Felt252, ContractAddressError> {
    Ok(compute_hash_on_elements(
        &get_contract_entry_points(contract_class, entry_point_type)?
            .iter()
            .flat_map(|contract_entry_point| {
                vec![
                    *contract_entry_point.selector(),
                    Felt252::from(contract_entry_point.offset()),
                ]
            })
            .collect::<Vec<Felt252>>(),
    )?)
}

/// Compute the hash for a deprecated contract class.
pub fn compute_deprecated_class_hash(
    contract_class: &ContractClass,
) -> Result<Felt252, ContractAddressError> {
    // Deprecated API version.
    let api_version = Felt252::ZERO;

    // Entrypoints by type, hashed.
    let external_functions =
        get_contract_entry_points_hashed(contract_class, &EntryPointType::External)?;
    let l1_handlers = get_contract_entry_points_hashed(contract_class, &EntryPointType::L1Handler)?;
    let constructors =
        get_contract_entry_points_hashed(contract_class, &EntryPointType::Constructor)?;

    // Builtin list but with the "_builtin" suffix removed.
    // This could be Vec::with_capacity when using the latest version of cairo-vm which includes .builtins_len() method for Program.
    let mut builtin_list_vec = Vec::new();

    for builtin_name in contract_class.program().iter_builtins() {
        builtin_list_vec.push(Felt252::from_bytes_be_slice(
            builtin_name.to_str().as_bytes(),
        ));
    }

    let builtin_list = compute_hash_on_elements(&builtin_list_vec)?;

    let hinted_class_hash = contract_class.hinted_class_hash();

    let mut bytecode_vector = Vec::new();

    for data in contract_class.program().iter_data() {
        bytecode_vector.push(
            *data
                .get_int_ref()
                .ok_or(ContractAddressError::NoneIntMaybeRelocatable)?,
        );
    }

    let bytecode = compute_hash_on_elements(&bytecode_vector)?;

    let flatted_contract_class: Vec<Felt252> = vec![
        api_version,
        external_functions,
        l1_handlers,
        constructors,
        builtin_list,
        *hinted_class_hash,
        bytecode,
    ];

    Ok(compute_hash_on_elements(&flatted_contract_class)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairo_vm::Felt252;
    use coverage_helper::test;

    #[test]
    fn test_compute_hinted_class_hash_with_abi() {
        let contract_class =
            ContractClass::from_path("starknet_programs/raw_contract_classes/class_with_abi.json")
                .unwrap();

        assert_eq!(
            contract_class.hinted_class_hash(),
            &Felt252::from_dec_str(
                "1164033593603051336816641706326288678020608687718343927364853957751413025239",
            )
            .unwrap()
        );
    }

    #[test]
    fn test_compute_class_hash_1354433237b0039baa138bf95b98fe4a8ae3df7ac4fd4d4845f0b41cd11bec4() {
        let contract_class = ContractClass::from_path("starknet_programs/raw_contract_classes/0x1354433237b0039baa138bf95b98fe4a8ae3df7ac4fd4d4845f0b41cd11bec4.json").unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex("1354433237b0039baa138bf95b98fe4a8ae3df7ac4fd4d4845f0b41cd11bec4")
                .unwrap()
        );
    }

    #[test]
    fn test_compute_class_hash_0x03131fa018d520a037686ce3efddeab8f28895662f019ca3ca18a626650f7d1e()
    {
        let contract_class = ContractClass::from_path("starknet_programs/raw_contract_classes/0x03131fa018d520a037686ce3efddeab8f28895662f019ca3ca18a626650f7d1e.json").unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex("03131fa018d520a037686ce3efddeab8f28895662f019ca3ca18a626650f7d1e")
                .unwrap()
        );
    }

    #[test]
    fn test_compute_class_hash_0x025ec026985a3bf9d0cc1fe17326b245dfdc3ff89b8fde106542a3ea56c5a918()
    {
        let contract_class = ContractClass::from_path("starknet_programs/raw_contract_classes/0x025ec026985a3bf9d0cc1fe17326b245dfdc3ff89b8fde106542a3ea56c5a918.json").unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex(
                "0x025ec026985a3bf9d0cc1fe17326b245dfdc3ff89b8fde106542a3ea56c5a918",
            )
            .unwrap()
        );
    }

    #[test]
    fn test_compute_class_hash_0x02c3348ad109f7f3967df6494b3c48741d61675d9a7915b265aa7101a631dc33()
    {
        let contract_class = ContractClass::from_path("starknet_programs/raw_contract_classes/0x02c3348ad109f7f3967df6494b3c48741d61675d9a7915b265aa7101a631dc33.json").unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex("0x02c3348ad109f7f3967df6494b3c48741d61675d9a7915b265aa7101a631dc33")
                .unwrap()
        );
    }

    // This was the contract class that caused an outage in Mainnet.
    // More info in EqLabs report: https://eqlabs.github.io/pathfinder/blog/2023-06-17_mainnet_incident.html
    #[test]
    fn test_compute_class_hash_0x00801ad5dc7c995addf7fbce1c4c74413586acb44f9ff44ba903a08a6153fa80()
    {
        let contract_class = ContractClass::from_path("starknet_programs/raw_contract_classes/0x00801ad5dc7c995addf7fbce1c4c74413586acb44f9ff44ba903a08a6153fa80.json").unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_dec_str(
                "226341635385251092193534262877925620859725853394183386505497817801290939008"
            )
            .unwrap()
        );
    }

    #[test]
    fn test_compute_class_hash_0x4d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f_mainnet(
    ) {
        let contract_class = ContractClass::from_path(
            "starknet_programs/raw_contract_classes/0x04d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f_mainnet.json"
        ).unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex("0x4d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f",)
                .unwrap()
        );
    }

    #[test]
    fn test_compute_class_hash_0x4d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f_goerli(
    ) {
        let contract_class = ContractClass::from_path(
            "starknet_programs/raw_contract_classes/0x04d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f_goerli.json"
        ).unwrap();

        assert_eq!(
            compute_deprecated_class_hash(&contract_class).unwrap(),
            Felt252::from_hex("0x4d07e40e93398ed3c76981e72dd1fd22557a78ce36c0515f679e27f0bb5bc5f")
                .unwrap()
        );
    }
}
