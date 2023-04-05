// This contract is a modification from:
// https://github.com/OpenZeppelin/cairo-contracts/blob/main/src/openzeppelin/introspection/erc165/IERC165.cairo

%lang starknet

from starkware.cairo.common.cairo_builtins import HashBuiltin
from starkware.cairo.common.bool import FALSE

@external
func supportsInterface{syscall_ptr: felt*, pedersen_ptr: HashBuiltin*, range_check_ptr}(
    interface_id: felt
) -> (success: felt) {
    return (success=FALSE);
}
