use crate::common::{
    store_next_authority_change, NextChangeInAuthority, GRANDPA_AUTHORITY_CHANGE_INTERMEDIATE_KEY,
    NEXT_CHANGE_IN_AUTHORITY_KEY,
};
use parity_scale_codec::alloc::collections::hash_map::RandomState;
use parity_scale_codec::alloc::collections::HashMap;
use parity_scale_codec::alloc::sync::Arc;
use parity_scale_codec::Encode;
use sc_client_api::backend::TransactionFor;
use sc_client_api::{AuxStore, Backend};
use sp_blockchain::Error as BlockchainError;
use sp_consensus::{
    BlockCheckParams, BlockImport, BlockImportParams, Error as ConsensusError, ImportResult,
};
use sp_runtime::traits::Block as BlockT;
use std::marker::PhantomData;
use std::ops::Deref;

// Wrapper around grandpa block import, which is mainly used to do
// some ibc client specific book-keeping.
pub struct BlockImportWrapper<Inner, Block, Backend, AuxStore> {
    wrapped_block_import: Inner,
    aux_store: Arc<AuxStore>,
    _phantom_data: PhantomData<Block>,
    _phantom_data2: PhantomData<Backend>,
}

impl<Inner, Block, BE, AS> BlockImportWrapper<Inner, Block, BE, AS>
where
    Block: BlockT,
    BE: Backend<Block>,
    Inner: BlockImport<Block, Error = ConsensusError, Transaction = TransactionFor<BE, Block>>,
    AS: AuxStore,
{
    pub fn new(wrapped_block_import: Inner, aux_store: Arc<AS>) -> Self {
        Self {
            wrapped_block_import,
            aux_store,
            _phantom_data: PhantomData,
            _phantom_data2: PhantomData,
        }
    }
}

impl<Inner, Block, BE, AS> BlockImport<Block> for BlockImportWrapper<Inner, Block, BE, AS>
where
    Block: BlockT,
    BE: Backend<Block>,
    Inner: BlockImport<Block, Error = ConsensusError, Transaction = TransactionFor<BE, Block>>,
    AS: AuxStore,
{
    type Error = ConsensusError;
    type Transaction = TransactionFor<BE, Block>;

    fn check_block(&mut self, block: BlockCheckParams<Block>) -> Result<ImportResult, Self::Error> {
        self.wrapped_block_import.check_block(block)
    }

    fn import_block(
        &mut self,
        mut block: BlockImportParams<Block, Self::Transaction>,
        cache: HashMap<[u8; 4], Vec<u8>, RandomState>,
    ) -> Result<ImportResult, Self::Error> {
        let possible_next_change_in_authority = match block
            .take_intermediate::<NextChangeInAuthority<Block>>(
                GRANDPA_AUTHORITY_CHANGE_INTERMEDIATE_KEY,
            ) {
            Err(e) => match e {
                Self::Error::NoIntermediate => Ok(None),
                _ => Err(e),
            },
            Ok(next_change_in_authority) => Ok(Some(next_change_in_authority)),
        }?;

        let result = self.wrapped_block_import.import_block(block, cache);

        let should_store_next_authority_change = match &result {
            Ok(ImportResult::Imported(imported_aux)) => {
                !imported_aux.bad_justification && !imported_aux.needs_finality_proof
            }
            _ => false,
        };

        if should_store_next_authority_change && possible_next_change_in_authority.is_some() {
            let next_change_in_authority = possible_next_change_in_authority.unwrap();
            store_next_authority_change(self.aux_store.clone(), next_change_in_authority.deref())
                .map_err(|err| Self::Error::Other(Box::new(err)))?;
        }

        result
    }
}