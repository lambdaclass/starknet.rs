%lang starknet

from starkware.starknet.common.syscalls import get_block_number, get_contract_address

@external
func test_get_block_number{syscall_ptr: felt*}() -> (block_number: felt) {
    let block_number = get_block_number();

    return (block_number);
}

@external
func test_get_contract_address{syscall_ptr: felt*}() -> (contract_address: felt) {
    let contract_address = get_contract_address();

    return (contract_address);
}
