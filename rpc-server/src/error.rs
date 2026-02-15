use nimiq_hash::Blake2bHash;
use nimiq_jsonrpc_core::RpcError;
use nimiq_keys::Address;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Core(#[from] nimiq_rpc_interface::error::Error),

    #[error("Block not found: {0}")]
    BlockNotFound(u32),

    #[error("Block not found: {0}")]
    BlockNotFoundByHash(Blake2bHash),

    #[error("Method not supported for a light blockchain")]
    NotSupportedForLightBlockchain,

    #[error("Method requires a history index")]
    RequiresHistoryIndex,

    #[error("No account with address: {0}")]
    AccountNotFound(Address),

    #[error("No validator with address: {0}")]
    ValidatorNotFound(Address),

    #[error("No staker with address: {0}")]
    StakerNotFound(Address),

    #[error("Serialization error: {0}")]
    Serialization(#[from] nimiq_serde::DeserializeError),

    #[error("Transaction not found: {0}")]
    TransactionNotFound(Blake2bHash),

    #[error("Multiple transactions found: {0}")]
    MultipleTransactionsFound(Blake2bHash),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("No consensus")]
    NoConsensus,
}

impl From<Error> for RpcError {
    fn from(e: Error) -> Self {
        RpcError::internal_error(Some(serde_json::value::Value::String(e.to_string())))
    }
}
