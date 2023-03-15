#![deny(warnings)]

use self::{
    cached_state::PyCachedState,
    types::{
        block_info::PyBlockInfo, contract_class::PyContractClass,
        contract_entry_point::PyContractEntryPoint,
    },
};
use pyo3::prelude::*;

mod cached_state;
mod starknet_state;
mod types;

#[pymodule]
pub fn starknet_rs(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<PyBlockInfo>()?;
    m.add_class::<PyCachedState>()?;
    m.add_class::<PyContractClass>()?;
    m.add_class::<PyContractEntryPoint>()?;

    Ok(())
}
