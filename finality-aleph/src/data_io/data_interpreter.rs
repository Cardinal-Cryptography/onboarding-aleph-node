use crate::{
    data_io::{
        chain_info::{AuxFinalizationChainInfoProvider, CachedChainInfoProvider},
        status_provider::get_proposal_status,
        AlephData, ChainInfoProvider,
    },
    BlockHashNum, SessionBoundaries,
};
use async_trait::async_trait;
use futures::channel::mpsc;
use log::{debug, error, warn};
use sc_client_api::HeaderBackend;
use sp_runtime::traits::{Block as BlockT, NumberFor, One, Zero};
use std::default::Default;
use std::sync::Arc;

type InterpretersChainInfoProvider<B, C> =
    CachedChainInfoProvider<B, AuxFinalizationChainInfoProvider<B, Arc<C>>>;

/// Takes as input ordered `AlephData` from `AlephBFT` and pushes blocks that should be finalized
/// to an output channel. The other end of the channel is held by the aggregator whose goal is to
/// create multisignatures under the finalized blocks.
pub struct OrderedDataInterpreter<B: BlockT, C: HeaderBackend<B>> {
    blocks_to_finalize_tx: mpsc::UnboundedSender<BlockHashNum<B>>,
    chain_info_provider: InterpretersChainInfoProvider<B, C>,
    last_finalized_by_aleph: BlockHashNum<B>,
    session_boundaries: SessionBoundaries<B>,
}

fn get_last_block_prev_session<B: BlockT, C: HeaderBackend<B>>(
    session_boundaries: SessionBoundaries<B>,
    mut client: Arc<C>,
) -> BlockHashNum<B> {
    if session_boundaries.first_block() > NumberFor::<B>::zero() {
        // We are in session > 0, we take the last block of previous session.
        let last_prev_session_num = session_boundaries.first_block() - NumberFor::<B>::one();
        client.get_finalized_at(last_prev_session_num).expect(
            "Last block of previous session must have been finalized before starting the current",
        )
    } else {
        // We are in session 0, we take the genesis block -- it is finalized by definition.
        client
            .get_finalized_at(NumberFor::<B>::zero())
            .expect("Genesis block must be available")
    }
}

impl<B: BlockT, C: HeaderBackend<B>> OrderedDataInterpreter<B, C> {
    pub fn new(
        blocks_to_finalize_tx: mpsc::UnboundedSender<BlockHashNum<B>>,
        client: Arc<C>,
        session_boundaries: SessionBoundaries<B>,
    ) -> Self {
        let last_finalized_by_aleph =
            get_last_block_prev_session(session_boundaries.clone(), client.clone());
        let chain_info_provider =
            AuxFinalizationChainInfoProvider::new(client, last_finalized_by_aleph.clone());
        let chain_info_provider =
            CachedChainInfoProvider::new(chain_info_provider, Default::default());

        OrderedDataInterpreter {
            blocks_to_finalize_tx,
            chain_info_provider,
            last_finalized_by_aleph,
            session_boundaries,
        }
    }

    fn block_to_finalize_from_data(&mut self, new_data: AlephData<B>) -> Option<BlockHashNum<B>> {
        match new_data {
            AlephData::Empty => None,
            AlephData::HeadProposal(unvalidated_proposal) => {
                let proposal = if let Some(proposal) =
                    unvalidated_proposal.validate_bounds(&self.session_boundaries)
                {
                    proposal
                } else {
                    warn!(target:"afa", "Incorrect proposal {:?} passed through data availability, session bounds: {:?}", unvalidated_proposal, self.session_boundaries);
                    return None;
                };

                // WARNING: If we ever enable pruning, this code (and the code in Data Store) must be carefully analyzed
                // for possible safety violations.

                use crate::data_io::proposal::ProposalStatus::*;
                let status = get_proposal_status(&mut self.chain_info_provider, &proposal, None);
                match status {
                    Finalize(block) => Some(block),
                    Ignore => {
                        debug!(target:"afa", "HopelessFork {:?} encountered in Data. Skipping.", proposal);
                        None
                    }
                    Pending(pending_status) => {
                        panic!(
                            "Pending proposal {:?} with status {:?} encountered in Data.",
                            proposal, pending_status
                        );
                    }
                }
            }
        }
    }
}

#[async_trait]
impl<B: BlockT, C: HeaderBackend<B> + Send + 'static> aleph_bft::FinalizationHandler<AlephData<B>>
    for OrderedDataInterpreter<B, C>
{
    async fn data_finalized(&mut self, data: AlephData<B>) {
        if let Some(block) = self.block_to_finalize_from_data(data) {
            self.last_finalized_by_aleph = block.clone();
            self.chain_info_provider
                .inner()
                .update_aux_finalized(block.clone());
            if let Err(err) = self.blocks_to_finalize_tx.unbounded_send(block) {
                error!(target: "afa", "Error in sending a block from FinalizationHandler, {}", err);
            }
        }
    }
}
