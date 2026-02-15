use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use nimiq_rpc_interface::{consensus::ConsensusInterface, types::RPCResult};

use crate::error::Error;

pub struct ConsensusDispatcher {
    established_flag: Arc<AtomicBool>,
}

impl ConsensusDispatcher {
    pub fn new(established_flag: Arc<AtomicBool>) -> Self {
        Self { established_flag }
    }
}

#[nimiq_jsonrpc_derive::service(rename_all = "camelCase")]
#[async_trait]
impl ConsensusInterface for ConsensusDispatcher {
    type Error = Error;

    async fn is_consensus_established(&self) -> RPCResult<bool, (), Self::Error> {
        Ok(self.established_flag.load(Ordering::Acquire).into())
    }
}
