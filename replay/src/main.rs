use std::{collections::HashMap, sync::Arc};

use clap::{Parser, Subcommand};
use rpc_state_reader::{
    execute_tx_configurable, execute_tx_configurable_with_state, get_transaction_hashes,
    rpc_state::{RpcChain, RpcState, RpcTransactionReceipt},
    RpcStateReader,
};
use starknet_api::hash::StarkFelt;
use starknet_api::{
    block::BlockNumber,
    stark_felt,
    transaction::{Transaction, TransactionHash},
};
use starknet_in_rust::{
    execution::TransactionExecutionInfo,
    state::{cached_state::CachedState, contract_class_cache::PermanentContractClassCache},
};

#[derive(Debug, Parser)]
#[command(about = "Replay is a tool for executing Starknet transactions.", long_about = None)]
struct ReplayCLI {
    #[command(subcommand)]
    subcommand: ReplayExecute,
}

#[derive(Subcommand, Debug)]
enum ReplayExecute {
    #[clap(about = "Execute a single transaction given a transaction hash.")]
    Tx {
        tx_hash: String,
        chain: String,
        block_number: u64,
        silent: Option<bool>,
    },
    #[clap(about = "Execute all the invoke transactions in a given block.")]
    Block {
        chain: String,
        block_number: u64,
        silent: Option<bool>,
    },
    #[clap(about = "Execute all the invoke transactions in a given range of blocks.")]
    BlockRange {
        block_start: u64,
        block_end: u64,
        chain: String,
        silent: Option<bool>,
    },
    #[clap(
        about = "Execute all the invoke transactions in a given range of blocks.
    Runs the all transactions twice, once to fill up the caches and a second one to benchmark."
    )]
    BenchBlockRange {
        block_start: u64,
        block_end: u64,
        chain: String,
    },
}

fn main() {
    let cli = ReplayCLI::parse();

    match cli.subcommand {
        ReplayExecute::Tx {
            tx_hash,
            chain,
            block_number,
            silent,
        } => {
            show_execution_data(tx_hash, &chain, block_number, silent);
        }
        ReplayExecute::Block {
            block_number,
            chain,
            silent,
        } => {
            println!("Executing block number: {}", block_number);
            let rpc_chain = parse_network(&chain);
            let block_number = BlockNumber(block_number);
            let transaction_hashes = get_transaction_hashes(block_number, rpc_chain)
                .expect("Unable to fetch the transaction hashes.");

            for tx_hash in transaction_hashes {
                show_execution_data(tx_hash, &chain, block_number.0, silent);
            }
        }
        ReplayExecute::BlockRange {
            block_start,
            block_end,
            chain,
            silent,
        } => {
            println!("Executing block range: {} - {}", block_start, block_end);
            let rpc_chain = parse_network(&chain);
            for block_number in block_start..=block_end {
                let block_number = BlockNumber(block_number);
                let transaction_hashes = get_transaction_hashes(block_number, rpc_chain)
                    .expect("Unable to fetch the transaction hashes.");

                for tx_hash in transaction_hashes {
                    show_execution_data(tx_hash, &chain, block_number.0, silent);
                }
            }
        }
        ReplayExecute::BenchBlockRange {
            block_start,
            block_end,
            chain,
        } => {
            println!("Filling up Cache");
            let network = parse_network(&chain);
            // Create a single class_cache for all states
            let class_cache = Arc::new(PermanentContractClassCache::default());
            // HashMaps to cache txs & states
            let mut transactions =
                HashMap::<BlockNumber, Vec<(TransactionHash, Transaction)>>::new();
            let mut cached_states = HashMap::<
                BlockNumber,
                CachedState<RpcStateReader, PermanentContractClassCache>,
            >::new();
            for block_number in block_start..=block_end {
                // For each block:
                let block_number = BlockNumber(block_number);
                // Create a cached state
                let rpc_reader = RpcStateReader::new(
                    RpcState::new_infura(network, block_number.into()).unwrap(),
                );
                let mut state = CachedState::new(Arc::new(rpc_reader), class_cache.clone());
                // Fetch txs for the block
                let transaction_hashes = get_transaction_hashes(block_number, network)
                    .expect("Unable to fetch the transaction hashes.");
                let mut txs_in_block = Vec::<(TransactionHash, Transaction)>::new();
                for tx_hash in transaction_hashes {
                    // Fetch tx and add it to txs_in_block cache
                    let tx_hash = TransactionHash(stark_felt!(tx_hash.strip_prefix("0x").unwrap()));
                    let tx = state.state_reader.0.get_transaction(&tx_hash).unwrap();
                    txs_in_block.push((tx_hash, tx));
                }
                // Add the txs from the current block to the transactions cache
                transactions.insert(block_number, txs_in_block);
                // Clean writes from cached_state
                state.cache_mut().storage_writes_mut().clear();
                state.cache_mut().class_hash_writes_mut().clear();
                state.cache_mut().nonce_writes_mut().clear();
                // Add the cached state for the current block to the cached_states cache
                cached_states.insert(block_number, state);
            }
            // Benchmark run should make no api requests as all data is cached

            println!("Executing block range: {} - {}", block_start, block_end);
            for block_number in block_start..=block_end {
                let block_number = BlockNumber(block_number);
                // Fetch state
                let state = cached_states.get_mut(&block_number).unwrap();
                // Fetch txs
                let block_txs = transactions.get(&block_number).unwrap();
                // Run txs
                for (tx_hash, tx) in block_txs {
                    execute_tx_configurable_with_state(
                        tx_hash,
                        tx.clone(),
                        network,
                        block_number,
                        false,
                        true,
                        state,
                    )
                    .unwrap();
                }
            }
        }
    }
}

fn parse_network(network: &str) -> RpcChain {
    match network.to_lowercase().as_str() {
        "mainnet" => RpcChain::MainNet,
        "testnet" => RpcChain::TestNet,
        "testnet2" => RpcChain::TestNet2,
        _ => panic!("Invalid network name, it should be one of: mainnet, testnet, testnet2"),
    }
}

fn show_execution_data(tx_hash: String, chain: &str, block_number: u64, silent: Option<bool>) {
    let rpc_chain = parse_network(chain);
    if silent.is_none() || !silent.unwrap() {
        println!("Executing transaction with hash: {}", tx_hash);
        println!("Block number: {}", block_number);
        println!("Chain: {}", chain);
    }
    let previous_block_number = BlockNumber(block_number - 1);

    let (tx_info, _trace, receipt) =
        match execute_tx_configurable(&tx_hash, rpc_chain, previous_block_number, false, true) {
            Ok(x) => x,
            Err(error_reason) => {
                println!("Error: {}", error_reason);
                return;
            }
        };
    let TransactionExecutionInfo {
        revert_error,
        actual_fee,
        ..
    } = tx_info;

    let sir_actual_fee = actual_fee;

    let RpcTransactionReceipt {
        actual_fee,
        execution_status,
        ..
    } = receipt;

    if silent.is_none() || !silent.unwrap() {
        println!("[RPC] Execution status: {:?}", execution_status);
        if let Some(revert_error) = revert_error {
            println!("[SIR] Revert error: {}", revert_error);
        }
        println!("[RPC] Actual fee: {} wei", actual_fee);
        println!("[SIR] Actual fee: {} wei", sir_actual_fee);
    }
}
