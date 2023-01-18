use crate::business_logic::fact_state::contract_state::ContractState;

use super::{
    dict_storage::{Prefix, StorageKey},
    errors::storage_errors::StorageError,
};
use std::str;

/* -----------------------------------------------------------------------------------
   -----------------------------------------------------------------------------------

    This module implements the trait for the the storages operations.
    The trait default functions handle the different data types that can be stored,
    some assumptions and work arounds have been taken for this.

    * All data types are turned into Vec<u8> before being passed to the set_value function.
    This is due to rust restrictions on parameter types for the functions in traits.

    * The same is true for get_value but this returns an Option because there may be nothing in the storage for a given key.

    * float types are assumed to be f32, in case of needing to store f64, get_double and set_double functions can be implemented.

    * Strings are assumed to be UTF-8 valid encoding, using a different format falls in an error.

    * get_str returns a String type rather than a str to avoid lifetimes or reference issues.

  -----------------------------------------------------------------------------------
  -----------------------------------------------------------------------------------
*/

//* ------------------
//*   Storage Trait
//* ------------------

pub(crate) trait Storage {
    fn set_value(&mut self, key: &StorageKey, value: Vec<u8>) -> Result<(), StorageError>;
    fn get_value(&self, key: &StorageKey) -> Option<Vec<u8>>;
    fn delete_value(&mut self, key: &StorageKey) -> Result<Vec<u8>, StorageError>;

    fn get_value_or_fail(&self, key: &StorageKey) -> Result<Vec<u8>, StorageError> {
        self.get_value(key).ok_or(StorageError::ErrorFetchingData)
    }

    fn set_int(&mut self, key: &[u8; 32], value: i32) -> Result<(), StorageError> {
        let val = value.to_ne_bytes().to_vec();
        self.set_value(&(Prefix::Int, *key), val)
    }

    fn get_int(&self, key: &[u8; 32]) -> Result<i32, StorageError> {
        let value = self
            .get_value(&(Prefix::Int, *key))
            .ok_or(StorageError::ErrorFetchingData)?;
        let slice: [u8; 4] = value
            .try_into()
            .map_err(|_| StorageError::IncorrectDataSize)?;
        Ok(i32::from_ne_bytes(slice))
    }

    fn get_int_or_default(&self, key: &[u8; 32], default: i32) -> Result<i32, StorageError> {
        match self.get_value(&(Prefix::Int, *key)) {
            Some(val) => {
                let slice: [u8; 4] = val
                    .try_into()
                    .map_err(|_| StorageError::IncorrectDataSize)?;
                Ok(i32::from_ne_bytes(slice))
            }
            None => Ok(default),
        }
    }

    fn get_int_or_fail(&self, key: &[u8; 32]) -> Result<i32, StorageError> {
        let val = self.get_value_or_fail(&(Prefix::Int, *key))?;
        let slice: [u8; 4] = val
            .try_into()
            .map_err(|_| StorageError::IncorrectDataSize)?;
        Ok(i32::from_ne_bytes(slice))
    }

    fn set_float(&mut self, key: &[u8; 32], value: f64) -> Result<(), StorageError> {
        let val = value.to_bits().to_be_bytes().to_vec();
        self.set_value(&(Prefix::Float, *key), val)
    }

    fn get_float(&self, key: &[u8; 32]) -> Result<f64, StorageError> {
        let val = self
            .get_value(&(Prefix::Float, *key))
            .ok_or(StorageError::ErrorFetchingData)?;
        let float_bytes: [u8; 8] = val
            .try_into()
            .map_err(|_| StorageError::IncorrectDataSize)?;

        Ok(f64::from_bits(u64::from_be_bytes(float_bytes)))
    }

    fn set_str(&mut self, key: &[u8; 32], value: &str) -> Result<(), StorageError> {
        let val = value.as_bytes().to_vec();
        self.set_value(&(Prefix::Str, *key), val)
    }

    fn get_str(&self, key: &[u8; 32]) -> Result<String, StorageError> {
        let val = self
            .get_value(&(Prefix::Str, *key))
            .ok_or(StorageError::ErrorFetchingData)?;
        let str = str::from_utf8(&val[..]).map_err(|_| StorageError::IncorrectUtf8Enconding)?;
        Ok(String::from(str))
    }

    fn set_contract_state(
        &mut self,
        key: &[u8; 32],
        value: &ContractState,
    ) -> Result<(), StorageError> {
        let contract_state = serde_json::to_string(value)?.as_bytes().to_vec();

        self.set_value(&(Prefix::ContractState, *key), contract_state)
    }

    fn get_contract_state(&self, key: &[u8; 32]) -> Result<ContractState, StorageError> {
        let ser_contract_state = self
            .get_value(&(Prefix::ContractState, *key))
            .ok_or(StorageError::ErrorFetchingData)?;

        let contract_state: ContractState = serde_json::from_slice(&ser_contract_state)?;
        Ok(contract_state)
    }
}

//* -------------------------
//*   FactFetching contract
//* -------------------------
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FactFetchingContext<T: Storage> {
    storage: T,
    n_workers: Option<usize>,
}

impl<T: Storage> FactFetchingContext<T> {
    pub fn new(storage: T, n_workers: Option<usize>) -> Self {
        FactFetchingContext { storage, n_workers }
    }
}

#[cfg(test)]
mod tests {
    use felt::{Felt, NewFelt};

    use crate::{starknet_storage::dict_storage::DictStorage, utils::test_utils::storage_key};

    use super::*;

    #[test]
    fn new_ffc() {
        let mut ffc = FactFetchingContext::new(DictStorage::new(), Some(2));

        let fkey = storage_key!("0000000000000000000000000000000000000000000000000000000000000000");
        ffc.storage.set_float(&fkey, 4.0);

        assert_eq!(ffc.storage.get_float(&fkey).unwrap(), 4.0)
    }

    #[test]
    fn get_and_set_contract_state() {
        let mut storage = DictStorage::new();

        let key = storage_key!("0000000000000000000000000000000000000000000000000000000000000000");

        let contract_state = ContractState::create([8; 32].to_vec(), Felt::new(9));
        storage
            .set_contract_state(&key, &contract_state)
            .expect("Error setting contract state");

        assert_eq!(Ok(contract_state), storage.get_contract_state(&key));
    }
}
