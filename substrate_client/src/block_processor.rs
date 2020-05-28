use crate::block_import_wrapper::BlockImportWrapper;
use crate::client::Client;
use crate::common::initialize_backend;
use crate::db;
use crate::dummy_objs::DummyCallExecutor;
use crate::dummy_objs::{DummyFetchChecker, DummyGenesisGrandpaAuthoritySetProvider};
use crate::runtime::RuntimeApiConstructor;
use crate::storage::Storage;
use crate::types::Block;
use crate::verifier::GrandpaVerifier;
use sc_client_api::FetchChecker;
use sc_finality_grandpa as grandpa;
use sp_blockchain::Result as ClientResult;
use sp_consensus::import_queue::{import_single_block, BlockImportResult, IncomingBlock};
use sp_consensus::BlockOrigin;
use sp_runtime::traits::NumberFor;
use std::sync::Arc;

pub type BlockProcessor<B> =
    Box<dyn FnMut(IncomingBlock<B>) -> Result<BlockImportResult<NumberFor<B>>, String>>;

pub fn setup_block_processor(
    encoded_data: Vec<u8>,
    max_non_finalized_blocks_allowed: u64,
) -> ClientResult<(BlockProcessor<Block>, db::Data)> {
    // This dummy genesis provider will panic, if auxiliary storage
    // does not contain authority set at LIGHT_AUTHORITY_SET_KEY.
    let dummy_grandpa_genesis_authority_set_provider = DummyGenesisGrandpaAuthoritySetProvider {};

    let (backend, data) = initialize_backend(encoded_data, max_non_finalized_blocks_allowed)?;

    // Custom client implementation with dummy runtime
    let client: Arc<Client<_, _, RuntimeApiConstructor, DummyCallExecutor<Block, Storage>>> =
        Arc::new(Client::new(backend.clone(), true));

    // This is to prevent grandpa light import queue to accidentally
    // re-write authority set
    let read_only_aux_store_client = Arc::new(client.clone_with_read_only_aux_store());

    // Since, we don't handle finality proof inside grandpa light import queue,
    // we don't need concrete implementation of FetchChecker trait.
    let fetch_checker: Arc<dyn FetchChecker<Block>> = Arc::new(DummyFetchChecker {});

    // We need to re-initialize grandpa light import queue because
    // current version read/write authority set from private field instead of
    // auxiliary storage.
    let block_processor_fn = Box::new(move |incoming_block: IncomingBlock<Block>| {
        let grandpa_block_import = grandpa::light_block_import(
            read_only_aux_store_client.clone(),
            backend.clone(),
            &dummy_grandpa_genesis_authority_set_provider,
            Arc::new(fetch_checker.clone()),
        )
        .map_err(|e| format!("{}", e))?;
        let mut grandpa_verifier = GrandpaVerifier::new(client.clone());
        let mut block_import_wrapper: BlockImportWrapper<_, _> =
            BlockImportWrapper::new(grandpa_block_import.clone(), client.clone());
        import_single_block(
            &mut block_import_wrapper,
            BlockOrigin::NetworkBroadcast,
            incoming_block,
            &mut grandpa_verifier,
        )
        .map_err(|e| format!("{:?}", e))
    });

    Ok((block_processor_fn, data))
}