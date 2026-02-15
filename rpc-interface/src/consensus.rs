use async_trait::async_trait;

use crate::types::RPCResult;

#[nimiq_jsonrpc_derive::proxy(name = "ConsensusProxy", rename_all = "camelCase")]
#[async_trait]
pub trait ConsensusInterface {
    type Error;

    /// Returns whether consensus is established.
    async fn is_consensus_established(&self) -> RPCResult<bool, (), Self::Error>;
}
