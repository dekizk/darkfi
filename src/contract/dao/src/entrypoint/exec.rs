/* This file is part of DarkFi (https://dark.fi)
 *
 * Copyright (C) 2020-2023 Dyne.org foundation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use darkfi_money_contract::{model::MoneyTransferParamsV1, MoneyFunction};
use darkfi_sdk::{
    crypto::{contract_id::MONEY_CONTRACT_ID, pasta_prelude::*, ContractId, PublicKey},
    dark_tree::DarkLeaf,
    db::{db_del, db_get, db_lookup},
    error::{ContractError, ContractResult},
    msg,
    pasta::pallas,
    ContractCall,
};
use darkfi_serial::{deserialize, serialize, Encodable, WriteExt};

use crate::{
    error::DaoError,
    model::{DaoExecParams, DaoExecUpdate, DaoProposalMetadata, VecAuthCallCommit},
    DaoFunction, DAO_CONTRACT_DB_PROPOSAL_BULLAS, DAO_CONTRACT_ZKAS_DAO_EXEC_NS,
};

/// `get_metdata` function for `Dao::Exec`
pub(crate) fn dao_exec_get_metadata(
    cid: ContractId,
    call_idx: u32,
    calls: Vec<DarkLeaf<ContractCall>>,
) -> Result<Vec<u8>, ContractError> {
    let self_ = &calls[call_idx as usize];
    let params: DaoExecParams = deserialize(&self_.data.data[1..])?;

    // Public inputs for the ZK proofs we have to verify
    let mut zk_public_inputs: Vec<(String, Vec<pallas::Base>)> = vec![];
    // Public keys for the transaction signatures we have to verify
    let signature_pubkeys: Vec<PublicKey> = vec![];

    let blind_vote = params.blind_total_vote;
    let yes_vote_coords = blind_vote.yes_vote_commit.to_affine().coordinates().unwrap();
    let all_vote_coords = blind_vote.all_vote_commit.to_affine().coordinates().unwrap();

    /*
    let mut input_valcoms = pallas::Point::identity();
    for input in &money_xfer_params.inputs {
        input_valcoms += input.value_commit;
    }
    let input_value_coords = input_valcoms.to_affine().coordinates().unwrap();

    assert!(money_xfer_params.inputs.len() > 0);
    // This value should be the same for all inputs, as enforced in process_instruction() below.
    let input_user_data_enc = money_xfer_params.inputs[0].user_data_enc;
    */

    zk_public_inputs.push((
        DAO_CONTRACT_ZKAS_DAO_EXEC_NS.to_string(),
        vec![
            params.proposal.inner(),
            params.proposal_auth_calls.commit(),
            *yes_vote_coords.x(),
            *yes_vote_coords.y(),
            *all_vote_coords.x(),
            *all_vote_coords.y(),
        ],
    ));

    // Serialize everything gathered and return it
    let mut metadata = vec![];
    zk_public_inputs.encode(&mut metadata)?;
    signature_pubkeys.encode(&mut metadata)?;

    Ok(metadata)
}

/// `process_instruction` function for `Dao::Exec`
pub(crate) fn dao_exec_process_instruction(
    cid: ContractId,
    call_idx: u32,
    calls: Vec<DarkLeaf<ContractCall>>,
) -> Result<Vec<u8>, ContractError> {
    let self_ = &calls[call_idx as usize];
    let params: DaoExecParams = deserialize(&self_.data.data[1..])?;

    // Check children of DAO exec match the specified calls
    for auth_call in &params.proposal_auth_calls {
        let child_idx = self_.children_indexes[auth_call.index];
        let child = &calls[child_idx];
        let call = &child.data;

        let contract_id = call.contract_id.inner();
        let function_code = call.data[0];

        if contract_id != auth_call.contract_id || function_code != auth_call.function_code {
            msg!("[Dao::Exec] Error: wrong child call");
            return Err(DaoError::ExecCallWrongChildCall.into())
        }
    }

    /*
    // MoneyTransfer should all have the same user_data set.
    // We check this by ensuring that user_data_enc is also the same for all inputs.
    // This means using the same blinding factor for all input's user_data.
    assert!(mt_params.inputs.len() > 0);
    let user_data_enc = mt_params.inputs[0].user_data_enc;
    for input in &mt_params.inputs[1..] {
        if input.user_data_enc != user_data_enc {
            msg!("[Dao::Exec] Error: Money inputs unmatched user_data_enc");
            return Err(DaoError::ExecCallInvalidFormat.into())
        }
    }

    // ======
    // Checks
    // ======
    // MoneyTransfer should have exactly 2 outputs
    if mt_params.outputs.len() != 2 {
        msg!("[Dao::Exec] Error: Money outputs != 2");
        return Err(DaoError::ExecCallOutputsLenNot2.into())
    }
    */

    // 2. Get the ProposalVote from DAO state
    let proposal_db = db_lookup(cid, DAO_CONTRACT_DB_PROPOSAL_BULLAS)?;
    let Some(data) = db_get(proposal_db, &serialize(&params.proposal))? else {
        msg!("[Dao::Exec] Error: Proposal {:?} not found", params.proposal);
        return Err(DaoError::ProposalNonexistent.into())
    };
    let proposal: DaoProposalMetadata = deserialize(&data)?;

    // 3. Check yes_vote commit and all_vote_commit are the same as in BlindAggregateVote
    if proposal.vote_aggregate.yes_vote_commit != params.blind_total_vote.yes_vote_commit ||
        proposal.vote_aggregate.all_vote_commit != params.blind_total_vote.all_vote_commit
    {
        return Err(DaoError::VoteCommitMismatch.into())
    }

    // Create state update
    let update = DaoExecUpdate { proposal: params.proposal };
    let mut update_data = vec![];
    update_data.write_u8(DaoFunction::Exec as u8)?;
    update.encode(&mut update_data)?;
    Ok(update_data)
}

/// `process_update` function for `Dao::Exec`
pub(crate) fn dao_exec_process_update(cid: ContractId, update: DaoExecUpdate) -> ContractResult {
    // Grab all db handles we want to work on
    let proposal_vote_db = db_lookup(cid, DAO_CONTRACT_DB_PROPOSAL_BULLAS)?;

    // Remove proposal from db
    db_del(proposal_vote_db, &serialize(&update.proposal))?;

    Ok(())
}
