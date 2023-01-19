use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    iter::zip,
};

use crate::{
    business_logic::state::{state_api::State, state_cache::StorageEntry},
    core::errors::{state_errors::StateError, syscall_handler_errors::SyscallHandlerError},
};
use cairo_rs::{types::relocatable::Relocatable, vm::vm_core::VirtualMachine};
use felt::{felt_str, Felt, FeltOps, NewFelt};
use num_traits::ToPrimitive;

//* -------------------
//*      Address
//* -------------------

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct Address(pub Felt);

//* -------------------
//*  Helper Functions
//* -------------------

pub fn get_integer(
    vm: &VirtualMachine,
    syscall_ptr: &Relocatable,
) -> Result<usize, SyscallHandlerError> {
    vm.get_integer(syscall_ptr)
        .map_err(|_| SyscallHandlerError::SegmentationFault)?
        .as_ref()
        .to_usize()
        .ok_or(SyscallHandlerError::FeltToUsizeFail)
}

pub fn get_big_int(
    vm: &VirtualMachine,
    syscall_ptr: &Relocatable,
) -> Result<Felt, SyscallHandlerError> {
    Ok(vm
        .get_integer(syscall_ptr)
        .map_err(|_| SyscallHandlerError::SegmentationFault)?
        .into_owned())
}

pub fn get_relocatable(
    vm: &VirtualMachine,
    syscall_ptr: &Relocatable,
) -> Result<Relocatable, SyscallHandlerError> {
    vm.get_relocatable(syscall_ptr)
        .map_err(|_| SyscallHandlerError::SegmentationFault)
}

pub fn get_integer_range(
    vm: &VirtualMachine,
    addr: &Relocatable,
    size: usize,
) -> Result<Vec<Felt>, SyscallHandlerError> {
    Ok(vm
        .get_integer_range(addr, size)
        .map_err(|_| SyscallHandlerError::SegmentationFault)?
        .into_iter()
        .map(|c| c.into_owned())
        .collect::<Vec<Felt>>())
}

pub fn felt_to_field_element(value: &Felt) -> Result<FieldElement, SyscallHandlerError> {
    FieldElement::from_dec_str(&value.to_str_radix(10))
        .map_err(|_| SyscallHandlerError::FailToComputeHash)
}

pub fn field_element_to_felt(felt: &FieldElement) -> Felt {
    Felt::from_bytes_be(&felt.to_bytes_be())
}

//* -------------------
//*    State utils
//* -------------------

/// Converts CachedState storage mapping to StateDiff storage mapping.
/// See to_cached_state_storage_mapping documentation.

pub fn to_state_diff_storage_mapping(
    storage_writes: HashMap<StorageEntry, Felt>,
) -> Result<HashMap<Felt, HashMap<[u8; 32], Address>>, StateError> {
    let mut storage_updates: HashMap<Felt, HashMap<[u8; 32], Address>> = HashMap::new();
    for ((address, key), value) in storage_writes {
        if storage_updates.contains_key(&address.0) {
            let mut map = storage_updates
                .get(&address.0)
                .ok_or(StateError::EmptyKeyInStorage)?
                .to_owned();
            map.insert(key, Address(value));
            storage_updates.insert(address.0, map);
        } else {
            let mut new_map: HashMap<[u8; 32], Address> = HashMap::new();
            new_map.insert(key, Address(value));
            storage_updates.insert(address.0, new_map);
        }
    }

    Ok(storage_updates)
}

/// Returns a mapping containing key-value pairs from a that are not included in b (if
/// a key appears in b with a different value, it will be part of the output).
/// Uses to take only updated cells from a mapping.

fn contained_and_not_updated<K, V>(key: &K, value: &V, map: HashMap<K, V>) -> bool
where
    K: Hash + Eq,
    V: PartialEq + Clone,
{
    let val = map.get(key);
    !(map.contains_key(key) && (Some(value) == val))
}

pub fn subtract_mappings<K, V>(map_a: HashMap<K, V>, map_b: HashMap<K, V>) -> HashMap<K, V>
where
    K: Hash + Eq + Clone,
    V: PartialEq + Clone,
{
    map_a
        .into_iter()
        .filter(|(k, v)| contained_and_not_updated(k, v, map_b.clone()))
        .collect()
}

/// Converts StateDiff storage mapping (addresses map to a key-value mapping) to CachedState
/// storage mapping (Tuple of address and key map to the associated value).

pub fn to_cache_state_storage_mapping(
    map: HashMap<Felt, HashMap<[u8; 32], Address>>,
) -> HashMap<StorageEntry, Felt> {
    let mut storage_writes = HashMap::new();
    for (address, contract_storage) in map {
        for (key, value) in contract_storage {
            storage_writes.insert((Address(address.clone()), key), value.0);
        }
    }
    storage_writes
}

// merge two hash maps into one

pub fn merge<K, V>(map_a: HashMap<K, V>, map_b: HashMap<K, V>) -> HashMap<K, V>
where
    K: Hash + Eq,
{
    let mut merge = HashMap::new();

    for (key, value) in map_a {
        merge.insert(key, value);
    }

    for (key, value) in map_b {
        merge.insert(key, value);
    }
    merge
}

// get a vector of keys from two hashmaps

pub fn get_keys<K, V>(map_a: HashMap<K, V>, map_b: HashMap<K, V>) -> Vec<K>
where
    K: Hash + Eq,
{
    let mut keys1: HashSet<K> = map_a.into_keys().collect();
    let keys2: HashSet<K> = map_b.into_keys().collect();

    keys1.extend(keys2);

    keys1.into_iter().collect()
}
//* -------------------
//* Macros
//* -------------------

use starknet_crypto::FieldElement;

#[cfg(test)]
#[macro_use]
pub mod test_utils {

    #[macro_export]
    macro_rules! any_box {
        ($val : expr) => {
            Box::new($val) as Box<dyn Any>
        };
    }
    pub(crate) use any_box;

    macro_rules! references {
        ($num: expr) => {{
            let mut references = HashMap::<
                usize,
                cairo_rs::hint_processor::hint_processor_definition::HintReference,
            >::new();
            for i in 0..$num {
                references.insert(
                    i as usize,
                    cairo_rs::hint_processor::hint_processor_definition::HintReference::new_simple(
                        (i as i32),
                    ),
                );
            }
            references
        }};
    }
    pub(crate) use references;

    macro_rules! ids_data {
        ( $( $name: expr ),* ) => {
            {
                let ids_names = vec![$( $name ),*];
                let references = $crate::utils::test_utils::references!(ids_names.len() as i32);
                let mut ids_data = HashMap::<String, cairo_rs::hint_processor::hint_processor_definition::HintReference>::new();
                for (i, name) in ids_names.iter().enumerate() {
                    ids_data.insert(name.to_string(), references.get(&i).unwrap().clone());
                }
                ids_data
            }
        };
    }
    pub(crate) use ids_data;

    macro_rules! vm {
        () => {{
            use felt::{Felt, NewFelt};
            VirtualMachine::new(false)
        }};

        ($use_trace:expr) => {{
            VirtualMachine::new($use_trace, Vec::new())
        }};
    }
    pub(crate) use vm;

    #[macro_export]
    macro_rules! add_segments {
        ($vm:expr, $n:expr) => {
            for _ in 0..$n {
                $vm.add_memory_segment();
            }
        };
    }
    pub(crate) use add_segments;

    #[macro_export]
    macro_rules! memory_insert {
        ($vm:expr, [ $( (($si:expr, $off:expr), $val:tt) ),* ] ) => {
            $( $crate::allocate_values!($vm, $si, $off, $val); )*
        };
    }
    pub(crate) use memory_insert;

    #[macro_export]
    macro_rules! allocate_values {
        ($vm: expr, $si:expr, $off:expr, ($sival:expr, $offval:expr)) => {
            let k = $crate::relocatable_value!($si, $off);
            let v = $crate::relocatable_value!($sival, $offval);
            $vm.insert_value(&k, &v).unwrap();
        };
        ($vm: expr, $si:expr, $off:expr, $val:expr) => {
            let v: felt::Felt = $val.into();
            let k = $crate::relocatable_value!($si, $off);
            $vm.insert_value(&k, v).unwrap();
        };
    }
    pub(crate) use allocate_values;

    #[macro_export]
    macro_rules! allocate_selector {
        ($vm: expr, (($si:expr, $off:expr), $val:expr)) => {
            use felt::FeltOps;
            let v = felt::Felt::from_bytes_be($val);
            let k = $crate::relocatable_value!($si, $off);
            $vm.insert_value(&k, v).unwrap();
        };
    }
    pub(crate) use allocate_selector;

    #[macro_export]
    macro_rules! relocatable_value {
        ($val1 : expr, $val2 : expr) => {
            Relocatable {
                segment_index: ($val1),
                offset: ($val2),
            }
        };
    }
    pub(crate) use relocatable_value;

    #[macro_export]
    macro_rules! exec_scopes_ref {
        () => {
            &mut ExecutionScopes::new()
        };
    }
    pub(crate) use exec_scopes_ref;

    #[macro_export]
    macro_rules! run_hint {
        ($vm:expr, $ids_data:expr, $hint_code:expr, $exec_scopes:expr, $constants:expr) => {{
            let hint_data = HintProcessorData::new_default($hint_code.to_string(), $ids_data);
            let hint_processor = BuiltinHintProcessor::new_empty();
            hint_processor.execute_hint(&mut $vm, $exec_scopes, &any_box!(hint_data), $constants)
        }};
        ($vm:expr, $ids_data:expr, $hint_code:expr, $exec_scopes:expr) => {{
            let hint_data = HintProcessorData::new_default($hint_code.to_string(), $ids_data);
            let hint_processor = BuiltinHintProcessor::new_empty();
            hint_processor.execute_hint(
                &mut $vm,
                $exec_scopes,
                &any_box!(hint_data),
                &HashMap::new(),
            )
        }};
        ($vm:expr, $ids_data:expr, $hint_code:expr) => {{
            let hint_data = HintProcessorData::new_default($hint_code.to_string(), $ids_data);
            let mut hint_processor = BuiltinHintProcessor::new_empty();
            hint_processor.execute_hint(
                &mut $vm,
                exec_scopes_ref!(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
        }};
    }
    pub(crate) use run_hint;

    #[macro_export]
    macro_rules! run_syscall_hint {
        ($vm:expr, $ids_data:expr, $hint_code:expr, $exec_scopes:expr, $constants:expr) => {{
            let hint_data = HintProcessorData::new_default($hint_code.to_string(), $ids_data);
            let hint_processor = BuiltinHintProcessor::new_empty();
            hint_processor.execute_hint(&mut $vm, $exec_scopes, &any_box!(hint_data), $constants)
        }};
        ($vm:expr, $ids_data:expr, $hint_code:expr, $exec_scopes:expr) => {{
            let hint_data = HintProcessorData::new_default($hint_code.to_string(), $ids_data);
            let hint_processor = BuiltinHintProcessor::new_empty();
            hint_processor.execute_hint(
                &mut $vm,
                $exec_scopes,
                &any_box!(hint_data),
                &HashMap::new(),
            )
        }};
        ($vm:expr, $ids_data:expr, $hint_code:expr) => {{
            let hint_data = HintProcessorData::new_default($hint_code.to_string(), $ids_data);
            let mut hint_processor = SyscallHintProcessor::new_empty().unwrap();
            hint_processor.execute_hint(
                &mut $vm,
                exec_scopes_ref!(),
                &any_box!(hint_data),
                &HashMap::new(),
            )
        }};
    }
    pub(crate) use run_syscall_hint;

    macro_rules! storage_key {
        ( $key:literal ) => {{
            assert_eq!($key.len(), 64, "keys must be 64 nibbles in length.");
            let key: [u8; 32] = $key
                .as_bytes()
                .chunks_exact(2)
                .map(|x| {
                    u8::from_str_radix(std::str::from_utf8(x).unwrap(), 16)
                        .expect("Key contains non-hexadecimal characters.")
                })
                .collect::<Vec<u8>>()
                .try_into()
                .unwrap();

            key
        }};
    }
    pub(crate) use storage_key;
}

#[cfg(test)]
mod test {
    use felt::Felt;
    use std::collections::HashMap;

    use crate::utils::{merge, subtract_mappings, Address};

    use super::{test_utils::storage_key, to_state_diff_storage_mapping};

    #[test]
    fn to_state_diff_storage_mapping_test() {
        let mut storage: HashMap<(Address, [u8; 32]), Felt> = HashMap::new();
        let address1: Address = Address(1.into());
        let key1 = [0; 32];
        let value1: Felt = 2.into();

        let address2: Address = Address(3.into());
        let key2 = [1; 32];

        let value2: Felt = 4.into();

        storage.insert((address1.clone(), key1), value1.clone());
        storage.insert((address2.clone(), key2), value2.clone());

        let map = to_state_diff_storage_mapping(storage).unwrap();

        assert_eq!(
            *map.get(&address1.0).unwrap().get(&key1).unwrap(),
            Address(value1)
        );
        assert_eq!(
            *map.get(&address2.0).unwrap().get(&key2).unwrap(),
            Address(value2)
        );
    }

    #[test]
    fn merge_test() {
        let mut a = HashMap::new();
        let mut b = HashMap::new();

        a.insert("key1", 1);
        a.insert("key2", 2);
        a.insert("key3", 3);

        b.insert("key1", 2);
        b.insert("key4", 6);

        let c = [("key1", 2), ("key2", 2), ("key3", 3), ("key4", 6)]
            .into_iter()
            .collect::<HashMap<&str, i32>>();
        assert_eq!(merge(a, b), c);
    }

    #[test]

    fn subtract_mappings_test() {
        let mut a = HashMap::new();
        let mut b = HashMap::new();

        a.insert("a", 2);
        a.insert("b", 3);

        b.insert("c", 2);
        b.insert("d", 43);
        b.insert("a", 3);

        let res = [("a", 2), ("b", 3)]
            .into_iter()
            .collect::<HashMap<&str, i32>>();

        assert_eq!(subtract_mappings(a, b), res);

        let mut c = HashMap::new();
        let mut d = HashMap::new();

        c.insert(1, 2);
        c.insert(3, 4);
        c.insert(6, 7);

        d.insert(1, 3);
        d.insert(3, 5);
        d.insert(6, 8);

        let res = [(1, 2), (3, 4), (6, 7)]
            .into_iter()
            .collect::<HashMap<i32, i32>>();

        assert_eq!(subtract_mappings(c, d), res);

        let mut e = HashMap::new();
        let mut f = HashMap::new();
        e.insert(1, 2);
        e.insert(3, 4);
        e.insert(6, 7);

        f.insert(1, 2);
        f.insert(3, 4);
        f.insert(6, 7);

        assert_eq!(subtract_mappings(e, f), HashMap::new())
    }
}
