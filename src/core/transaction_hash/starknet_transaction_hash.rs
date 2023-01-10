use felt::{felt_str, Felt};

use crate::{
    core::errors::syscall_handler_errors::SyscallHandlerError, hash_utils::compute_hash_on_elements,
};

#[derive(Debug)]
pub enum TransactionHashPrefix {
    Declare,
    Deploy,
    DeployAccount,
    Invoke,
    L1Handler,
}

impl TransactionHashPrefix {
    fn get_prefix(&self) -> Felt {
        match self {
            TransactionHashPrefix::Declare => felt_str!("28258975365558885"),
            TransactionHashPrefix::Deploy => felt_str!("110386840629113"),
            TransactionHashPrefix::DeployAccount => {
                felt_str!("2036277798190617858034555652763252")
            }
            TransactionHashPrefix::Invoke => felt_str!("115923154332517"),
            TransactionHashPrefix::L1Handler => felt_str!("510926345461491391292786"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_transaction_hash_common(
    tx_hash_prefix: TransactionHashPrefix,
    version: u64,
    contract_address: u64,
    entry_point_selector: u64,
    calldata: &[Felt],
    max_fee: u64,
    chain_id: u64,
    additional_data: &[u64],
) -> Result<Felt, SyscallHandlerError> {
    /// Calculates the transaction hash in the StarkNet network - a unique identifier of the
    /// transaction.
    /// The transaction hash is a hash chain of the following information:
    ///    1. A prefix that depends on the transaction type.
    ///    2. The transaction's version.
    ///    3. Contract address.
    ///    4. Entry point selector.
    ///    5. A hash chain of the calldata.
    ///    6. The transaction's maximum fee.
    ///    7. The network's chain ID.
    /// Each hash chain computation begins with 0 as initialization and ends with its length appended.
    /// The length is appended in order to avoid collisions of the following kind:
    /// H([x,y,z]) = h(h(x,y),z) = H([w, z]) where w = h(x,y).
    let calldata_hash = compute_hash_on_elements(calldata)?;

    let mut data_to_hash: Vec<Felt> = vec![
        tx_hash_prefix.get_prefix(),
        version.into(),
        contract_address.into(),
        entry_point_selector.into(),
        calldata_hash,
        max_fee.into(),
        chain_id.into(),
    ];

    data_to_hash.extend(additional_data.iter().map(|n| (*n).into()));

    compute_hash_on_elements(&data_to_hash)
}

pub fn calculate_deploy_transaction_hash(
    version: u64,
    contract_address: u64,
    constructor_calldata: &[Felt],
    chain_id: u64,
) -> Result<Felt, SyscallHandlerError> {
    calculate_transaction_hash_common(
        TransactionHashPrefix::Deploy,
        version,
        contract_address,
        // TODO: A constant CONSTRUCTOR_ENTRY_POINT_SELECTOR must be provided here.
        // See https://github.com/starkware-libs/cairo-lang/blob/9889fbd522edc5eff603356e1912e20642ae20af/src/starkware/starknet/public/abi.py#L53
        todo!(),
        constructor_calldata,
        // Field max_fee is considered 0 for Deploy transaction hash calculation purposes.
        0,
        chain_id,
        &Vec::new(),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_deploy_account_transaction_hash(
    version: u64,
    contract_address: u64,
    class_hash: u64,
    constructor_calldata: &[Felt],
    max_fee: u64,
    nonce: u64,
    salt: u64,
    chain_id: u64,
) -> Result<Felt, SyscallHandlerError> {
    let mut calldata: Vec<Felt> = vec![class_hash, salt].iter().map(|n| (*n).into()).collect();
    calldata.extend_from_slice(constructor_calldata);

    calculate_transaction_hash_common(
        TransactionHashPrefix::DeployAccount,
        version,
        contract_address,
        0,
        &calldata,
        max_fee,
        chain_id,
        &[nonce],
    )
}

#[cfg(test)]
mod tests {
    use felt::felt_str;

    use super::*;

    #[test]
    fn calculate_transaction_hash_common_test() {
        let tx_hash_prefix = TransactionHashPrefix::Declare;
        let version = 0;
        let contract_addres = 42;
        let entry_point_selector = 100;
        let calldata = vec![540.into(), 338.into()];
        let max_fee = 10;
        let chain_id = 1;
        let additional_data: Vec<u64> = Vec::new();

        // Expected value taken from Python implementation of calculate_transaction_hash_common function
        let expected = felt_str!(
            "2401716064129505935860131145275652294383308751137512921151718435935971973354"
        );

        let result = calculate_transaction_hash_common(
            tx_hash_prefix,
            version,
            contract_addres,
            entry_point_selector,
            &calldata,
            max_fee,
            chain_id,
            &additional_data,
        )
        .unwrap();

        assert_eq!(result, expected);
    }
}
