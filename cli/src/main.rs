use actix_web::{post, web, App, HttpResponse, HttpServer};
use cairo_vm::{utils::felt_to_biguint, Felt252};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use starknet_in_rust::{
    core::{
        contract_address::compute_deprecated_class_hash,
        errors::{contract_address_errors::ContractAddressError, state_errors::StateError},
        transaction_hash::{
            calculate_declare_transaction_hash, calculate_deploy_transaction_hash,
            calculate_transaction_hash_common, TransactionHashPrefix,
        },
    },
    definitions::{
        block_context::BlockContext,
        constants::{DECLARE_VERSION, TRANSACTION_VERSION},
    },
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        TransactionExecutionContext,
    },
    hash_utils::calculate_contract_address,
    parser_errors::ParserError,
    serde_structs::read_abi,
    services::api::contract_classes::{
        compiled_class::CompiledClass, deprecated_contract_class::ContractClass,
    },
    state::{cached_state::CachedState, state_api::State},
    state::{
        contract_class_cache::PermanentContractClassCache,
        in_memory_state_reader::InMemoryStateReader, ExecutionResourcesManager, StateDiff,
    },
    transaction::{error::TransactionError, InvokeFunction},
    utils::{felt_to_hash, string_to_hash, Address},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Declare(DeclareArgs),
    Deploy(DeployArgs),
    Invoke(InvokeArgs),
    Call(CallArgs),
    #[command(name = "starknet_in_rust")]
    Devnet(DevnetArgs),
}

#[derive(Args, Serialize, Deserialize)]
pub struct DeclareArgs {
    #[arg(long)]
    contract: PathBuf,
}

#[derive(Args, Serialize, Deserialize)]
struct DeployArgs {
    #[arg(long = "class_hash")]
    class_hash: String,
    #[arg(long, default_value = "1111")]
    salt: i32,
    #[arg(long, num_args=1.., value_delimiter = ' ')]
    inputs: Option<Vec<i32>>,
}

#[derive(Args, Serialize, Deserialize)]
struct InvokeArgs {
    #[arg(long)]
    address: String,
    #[arg(long)]
    abi: PathBuf,
    #[arg(long)]
    function: String,
    #[arg(long, num_args=1.., value_delimiter = ' ')]
    inputs: Option<Vec<i32>>,
    #[arg(long)]
    hash: Option<String>,
}

#[derive(Args, Serialize, Deserialize)]
struct CallArgs {
    #[arg(long)]
    address: String,
    #[arg(long)]
    abi: PathBuf,
    #[arg(long)]
    function: String,
    #[arg(long, num_args=1.., value_delimiter = ' ')]
    inputs: Option<Vec<i32>>,
}

#[derive(Args)]
struct DevnetArgs {
    #[arg(long, default_value = "7878")]
    port: u16,
}

struct AppState {
    cached_state: Mutex<CachedState<InMemoryStateReader, PermanentContractClassCache>>,
}

fn declare_parser(
    cached_state: &mut CachedState<InMemoryStateReader, PermanentContractClassCache>,
    args: &DeclareArgs,
) -> Result<(Felt252, Felt252), ParserError> {
    let contract_class =
        ContractClass::from_path(&args.contract).map_err(ContractAddressError::Program)?;
    let class_hash = compute_deprecated_class_hash(&contract_class)?;
    cached_state.set_contract_class(
        &felt_to_hash(&class_hash),
        &CompiledClass::Deprecated(Arc::new(contract_class.clone())),
    )?;

    let tx_hash = calculate_declare_transaction_hash(
        &contract_class,
        Felt252::ZERO,
        &Address(0.into()),
        0,
        *DECLARE_VERSION,
        Felt252::ZERO,
    )?;
    Ok((class_hash, tx_hash))
}

fn deploy_parser(
    cached_state: &mut CachedState<InMemoryStateReader, PermanentContractClassCache>,
    args: &DeployArgs,
) -> Result<(Felt252, Felt252), ParserError> {
    let constructor_calldata = match &args.inputs {
        Some(vec) => vec.iter().map(|&n| n.into()).collect(),
        None => Vec::new(),
    };
    let address = calculate_contract_address(
        &args.salt.into(),
        &Felt252::from_hex(&args.class_hash)
            .map_err(|_| ParserError::ParseFelt(args.class_hash.clone()))?,
        &constructor_calldata,
        Address(0.into()),
    )?;

    cached_state.deploy_contract(Address(address), string_to_hash(&args.class_hash))?;
    let tx_hash = calculate_deploy_transaction_hash(
        0.into(),
        &Address(address),
        &constructor_calldata,
        Felt252::ZERO,
    )?;
    Ok((address, tx_hash))
}

fn invoke_parser(
    cached_state: &mut CachedState<InMemoryStateReader, PermanentContractClassCache>,
    args: &InvokeArgs,
) -> Result<(Felt252, Felt252), ParserError> {
    let contract_address = Address(
        Felt252::from_hex(&args.address)
            .map_err(|_| ParserError::ParseFelt(args.address.clone()))?,
    );
    let class_hash = cached_state.get_class_hash_at(&contract_address)?;
    let contract_class: ContractClass = cached_state
        .get_contract_class(&class_hash)?
        .try_into()
        .map_err(StateError::from)?;
    let function_entrypoint_indexes = read_abi(&args.abi);
    let transaction_hash = args.hash.clone().map(|f| {
        Felt252::from_hex(&f)
            .map_err(|_| ParserError::ParseFelt(f.clone()))
            .unwrap()
    });
    let entry_points_by_type = contract_class.entry_points_by_type().clone();
    let (entry_point_index, entry_point_type) = function_entrypoint_indexes
        .get(&args.function)
        .ok_or_else(|| ParserError::FunctionEntryPoint(args.function.clone()))?;

    let entrypoint_selector = entry_points_by_type
        .get(entry_point_type)
        .ok_or(ParserError::EntryPointType(*entry_point_type))?
        .get(*entry_point_index)
        .ok_or(ParserError::EntryPointIndex(*entry_point_index))?
        .selector();

    let calldata = match &args.inputs {
        Some(vec) => vec.iter().map(|&n| n.into()).collect(),
        None => Vec::new(),
    };
    let internal_invoke = InvokeFunction::new_with_tx_hash(
        contract_address.clone(),
        *entrypoint_selector,
        Default::default(),
        *TRANSACTION_VERSION,
        calldata.clone(),
        vec![],
        Some(Felt252::ZERO),
        transaction_hash.unwrap(),
    )?;
    let mut transactional_state = cached_state.create_transactional()?;
    let _tx_info = internal_invoke.apply(
        &mut transactional_state,
        &BlockContext::default(),
        0,
        #[cfg(feature = "cairo-native")]
        None,
    )?;
    cached_state.apply_state_update(&StateDiff::from_cached_state(transactional_state.cache())?)?;

    let tx_hash = calculate_transaction_hash_common(
        TransactionHashPrefix::Invoke,
        *TRANSACTION_VERSION,
        &contract_address,
        *entrypoint_selector,
        &calldata,
        0,
        Felt252::ZERO,
        &[],
    )?;

    Ok((contract_address.0, tx_hash))
}

fn call_parser(
    cached_state: &mut CachedState<InMemoryStateReader, PermanentContractClassCache>,
    args: &CallArgs,
) -> Result<Vec<Felt252>, ParserError> {
    let contract_address = Address(
        Felt252::from_hex(&args.address)
            .map_err(|_| ParserError::ParseFelt(args.address.clone()))?,
    );
    let class_hash = cached_state.get_class_hash_at(&contract_address)?;
    let contract_class: ContractClass = cached_state
        .get_contract_class(&class_hash)?
        .try_into()
        .map_err(StateError::from)?;
    let function_entrypoint_indexes = read_abi(&args.abi);
    let entry_points_by_type = contract_class.entry_points_by_type().clone();
    let (entry_point_index, entry_point_type) = function_entrypoint_indexes
        .get(&args.function)
        .ok_or_else(|| ParserError::FunctionEntryPoint(args.function.clone()))?;

    let entrypoint_selector = entry_points_by_type
        .get(entry_point_type)
        .ok_or(ParserError::EntryPointType(*entry_point_type))?
        .get(*entry_point_index)
        .ok_or(ParserError::EntryPointIndex(*entry_point_index))?
        .selector();
    let caller_address = Address(0.into());
    let calldata = match &args.inputs {
        Some(vec) => vec.iter().map(|&n| n.into()).collect(),
        None => Vec::new(),
    };
    let execution_entry_point = ExecutionEntryPoint::new(
        contract_address,
        calldata,
        *entrypoint_selector,
        caller_address,
        *entry_point_type,
        None,
        None,
        0,
    );
    let block_context = BlockContext::default();
    let ExecutionResult { call_info, .. } = execution_entry_point.execute(
        cached_state,
        &block_context,
        &mut ExecutionResourcesManager::default(),
        &mut TransactionExecutionContext::default(),
        false,
        block_context.invoke_tx_max_n_steps(),
        #[cfg(feature = "cairo-native")]
        None,
    )?;

    let call_info = call_info.ok_or(TransactionError::CallInfoIsNone)?;

    Ok(call_info.retdata)
}

async fn devnet_parser(devnet_args: &DevnetArgs) -> Result<(), ParserError> {
    start_devnet(devnet_args.port).await?;
    Ok(())
}

#[post("/declare")]
async fn declare_req(data: web::Data<AppState>, args: web::Json<DeclareArgs>) -> HttpResponse {
    let mut cached_state = data.cached_state.lock().unwrap();
    match declare_parser(&mut cached_state, &args) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => HttpResponse::ExpectationFailed().body(e.to_string()),
    }
}

#[post("/deploy")]
async fn deploy_req(data: web::Data<AppState>, args: web::Json<DeployArgs>) -> HttpResponse {
    let mut cached_state = data.cached_state.lock().unwrap();
    match deploy_parser(&mut cached_state, &args) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => HttpResponse::ExpectationFailed().body(e.to_string()),
    }
}

#[post("/invoke")]
async fn invoke_req(data: web::Data<AppState>, args: web::Json<InvokeArgs>) -> HttpResponse {
    let mut cached_state = data.cached_state.lock().unwrap();
    match invoke_parser(&mut cached_state, &args) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => HttpResponse::ExpectationFailed().body(e.to_string()),
    }
}

#[post("/call")]
async fn call_req(data: web::Data<AppState>, args: web::Json<CallArgs>) -> HttpResponse {
    println!("call received");
    let mut cached_state = data.cached_state.lock().unwrap();
    match call_parser(&mut cached_state, &args) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => HttpResponse::ExpectationFailed().body(e.to_string()),
    }
}

pub async fn start_devnet(port: u16) -> Result<(), std::io::Error> {
    let cached_state = web::Data::new(AppState {
        cached_state: Mutex::new(CachedState::new(
            Arc::new(InMemoryStateReader::default()),
            Arc::new(PermanentContractClassCache::default()),
        )),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(cached_state.clone())
            .service(declare_req)
            .service(deploy_req)
            .service(invoke_req)
            .service(call_req)
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
}

#[actix_web::main]
async fn main() -> Result<(), ParserError> {
    let cli = Cli::parse();
    match &cli.command {
        Commands::Declare(declare_args) => {
            let response = awc::Client::new()
                .post("http://127.0.0.1:7878/declare")
                .send_json(&declare_args)
                .await;
            match response {
                Ok(mut resp) => {
                    match resp.json::<(Felt252, Felt252)>().await {
                        Ok(body) => println!("Declare transaction was sent.\nContract class hash: 0x{:x}\nTransaction hash: 0x{:x}", felt_to_biguint(body.0), felt_to_biguint(body.1)),
                        Err(e) => println!("{e}")
                    }
                },
                Err(ref e) => println!("{e}"),
            };
            Ok(())
        }
        Commands::Deploy(deploy_args) => {
            let response = awc::Client::new()
                .post("http://127.0.0.1:7878/deploy")
                .send_json(&deploy_args)
                .await;
            match response {
                Ok(mut resp) => {
                    match resp.json::<(Felt252, Felt252)>().await {
                        Ok(body) => println!("Invoke transaction for contract deployment was sent.\nContract address: 0x{:x}\nTransaction hash: 0x{:x}", felt_to_biguint(body.0), felt_to_biguint(body.1)),
                        Err(e) => println!("{e}")
                    }
                },
                Err(ref e) => println!("{e}"),
            };
            Ok(())
        }
        Commands::Invoke(invoke_args) => {
            let response = awc::Client::new()
                .post("http://127.0.0.1:7878/invoke")
                .send_json(&invoke_args)
                .await;
            match response {
                Ok(mut resp) => {
                    match resp.json::<(Felt252, Felt252)>().await {
                        Ok(body) => println!("Invoke transaction was sent.\nContract address: 0x{:x}\nTransaction hash: 0x{:x}", felt_to_biguint(body.0), felt_to_biguint(body.1)),
                        Err(e) => println!("{e}")
                    }
                },
                Err(ref e) => println!("{e}"),
            };
            Ok(())
        }
        Commands::Call(call_args) => {
            let response = awc::Client::new()
                .post("http://127.0.0.1:7878/call")
                .send_json(&call_args)
                .await;
            match response {
                Ok(mut resp) => match resp.json::<Vec<Felt252>>().await {
                    Ok(body) => println!(
                        "{}",
                        body.iter()
                            .fold(String::new(), |acc, arg| acc + &format!("{arg}"))
                    ),
                    Err(e) => println!("{e}"),
                },
                Err(ref e) => println!("{e}"),
            };
            Ok(())
        }
        Commands::Devnet(devnet_args) => devnet_parser(devnet_args).await,
    }?;
    Ok(())
}
