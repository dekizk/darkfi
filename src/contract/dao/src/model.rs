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

use core::str::FromStr;

use darkfi_sdk::{
    crypto::{
        note::AeadEncryptedNote, pasta_prelude::*, poseidon_hash, MerkleNode, Nullifier, PublicKey,
        TokenId,
    },
    error::ContractError,
    pasta::pallas,
};
use darkfi_serial::{Decodable, Encodable, SerialDecodable, SerialEncodable};

#[cfg(feature = "client")]
use darkfi_serial::async_trait;

use darkfi_sdk::crypto::{ShareAddress, ShareAddressType};

/// DAOs are represented on chain as a commitment to this object
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct Dao {
    pub proposer_limit: u64,
    pub quorum: u64,
    pub approval_ratio_quot: u64,
    pub approval_ratio_base: u64,
    pub gov_token_id: TokenId,
    pub public_key: PublicKey,
    pub bulla_blind: pallas::Base,
}

impl Dao {
    pub fn to_bulla(&self) -> DaoBulla {
        let proposer_limit = pallas::Base::from(self.proposer_limit);
        let quorum = pallas::Base::from(self.quorum);
        let approval_ratio_quot = pallas::Base::from(self.approval_ratio_quot);
        let approval_ratio_base = pallas::Base::from(self.approval_ratio_base);
        let (pub_x, pub_y) = self.public_key.xy();
        let bulla = poseidon_hash([
            proposer_limit,
            quorum,
            approval_ratio_quot,
            approval_ratio_base,
            self.gov_token_id.inner(),
            pub_x,
            pub_y,
            self.bulla_blind,
        ]);
        DaoBulla(bulla)
    }
}

/// A `DaoBulla` represented in the state
#[derive(Debug, Copy, Clone, Eq, PartialEq, SerialEncodable, SerialDecodable)]
pub struct DaoBulla(pallas::Base);

impl DaoBulla {
    /// Reference the raw inner base field element
    pub fn inner(&self) -> pallas::Base {
        self.0
    }

    /// Create a `DaoBulla` object from given bytes, erroring if the
    /// input bytes are noncanonical.
    pub fn from_bytes(x: [u8; 32]) -> Result<Self, ContractError> {
        match pallas::Base::from_repr(x).into() {
            Some(v) => Ok(Self(v)),
            None => {
                Err(ContractError::IoError("Failed to instantiate DaoBulla from bytes".to_string()))
            }
        }
    }

    /// Convert the `DaoBulla` type into 32 raw bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_repr()
    }
}

impl std::hash::Hash for DaoBulla {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.to_bytes());
    }
}

darkfi_sdk::fp_from_bs58!(DaoBulla);
darkfi_sdk::fp_to_bs58!(DaoBulla);
darkfi_sdk::ty_from_fp!(DaoBulla);

impl TryInto<DaoBulla> for ShareAddress {
    type Error = String;
    fn try_into(self) -> Result<DaoBulla, Self::Error> {
        if self.prefix != ShareAddressType::DaoBulla {
            return Err("wrong prefix".to_string())
        }
        let sk: Result<DaoBulla, ContractError> = DaoBulla::from_bytes(self.raw);
        Ok(sk.unwrap())
    }
}

#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoAuthCall {
    pub contract_id: pallas::Base,
    pub function_code: u8,
    pub auth_data: Vec<u8>,
}

pub trait VecAuthCallCommit {
    fn commit(&self) -> pallas::Base;
}

impl VecAuthCallCommit for Vec<DaoAuthCall> {
    fn commit(&self) -> pallas::Base {
        let mut hasher = blake3::Hasher::new();
        self.encode(&mut hasher).unwrap();
        let hash = hasher.finalize();
        let bytes = hash.as_bytes();
        let raw_base: [u64; 4] = Decodable::decode(&mut bytes.as_slice()).unwrap();
        pallas::Base::from_raw(raw_base)
    }
}

#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoProposal {
    pub auth_calls: Vec<DaoAuthCall>,
    pub creation_day: u64,
    pub duration_days: u64,
    /// Arbitrary data provided by the user. We don't use this.
    pub user_data: pallas::Base,
    pub dao_bulla: DaoBulla,
    pub blind: pallas::Base,
}

impl DaoProposal {
    pub fn to_bulla(&self) -> DaoProposalBulla {
        let bulla = poseidon_hash([
            self.auth_calls.commit(),
            pallas::Base::from(self.creation_day),
            pallas::Base::from(self.duration_days),
            self.user_data,
            self.dao_bulla.inner(),
            self.blind,
        ]);
        DaoProposalBulla(bulla)
    }
}

/// A `DaoProposalBulla` represented in the state
#[derive(Debug, Copy, Clone, Eq, PartialEq, SerialEncodable, SerialDecodable)]
pub struct DaoProposalBulla(pallas::Base);

impl DaoProposalBulla {
    /// Reference the raw inner base field element
    pub fn inner(&self) -> pallas::Base {
        self.0
    }

    /// Create a `DaoBulla` object from given bytes, erroring if the
    /// input bytes are noncanonical.
    pub fn from_bytes(x: [u8; 32]) -> Result<Self, ContractError> {
        match pallas::Base::from_repr(x).into() {
            Some(v) => Ok(Self(v)),
            None => Err(ContractError::IoError(
                "Failed to instantiate DaoProposalBulla from bytes".to_string(),
            )),
        }
    }

    /// Convert the `DaoBulla` type into 32 raw bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_repr()
    }
}

impl std::hash::Hash for DaoProposalBulla {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.to_bytes());
    }
}

darkfi_sdk::fp_from_bs58!(DaoProposalBulla);
darkfi_sdk::fp_to_bs58!(DaoProposalBulla);
darkfi_sdk::ty_from_fp!(DaoProposalBulla);

/// Parameters for `Dao::Mint`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoMintParams {
    /// The DAO bulla
    pub dao_bulla: DaoBulla,
    /// The DAO public key
    pub dao_pubkey: PublicKey,
}

/// State update for `Dao::Mint`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoMintUpdate {
    /// Revealed DAO bulla
    pub dao_bulla: DaoBulla,
}

/// Parameters for `Dao::Propose`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoProposeParams {
    /// Merkle root of the DAO in the DAO state
    pub dao_merkle_root: MerkleNode,
    /// Token ID commitment for the proposal
    pub token_commit: pallas::Base,
    /// Bulla of the DAO proposal
    pub proposal_bulla: DaoProposalBulla,
    /// Encrypted note
    pub note: AeadEncryptedNote,
    /// Inputs for the proposal
    pub inputs: Vec<DaoProposeParamsInput>,
}

/// Input for a DAO proposal
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoProposeParamsInput {
    /// Value commitment for the input
    pub value_commit: pallas::Point,
    /// Merkle root for the input's inclusion proof
    pub merkle_root: MerkleNode,
    /// Public key used for signing
    pub signature_public: PublicKey,
}

/// State update for `Dao::Propose`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoProposeUpdate {
    /// Minted proposal bulla
    pub proposal_bulla: DaoProposalBulla,
    /// Snapshotted Merkle root in the Money state
    pub snapshot_root: MerkleNode,
}

/// Metadata for a DAO proposal on the blockchain
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoProposalMetadata {
    /// Vote aggregate
    pub vote_aggregate: DaoBlindAggregateVote,
    /// Snapshotted Merkle root in the Money state
    pub snapshot_root: MerkleNode,
}

/// Parameters for `Dao::Vote`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoVoteParams {
    /// Token commitment for the vote inputs
    pub token_commit: pallas::Base,
    /// Proposal bulla being voted on
    pub proposal_bulla: DaoProposalBulla,
    /// Commitment for yes votes
    pub yes_vote_commit: pallas::Point,
    /// Encrypted note
    pub note: AeadEncryptedNote,
    /// Inputs for the vote
    pub inputs: Vec<DaoVoteParamsInput>,
}

/// Input for a DAO proposal vote
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoVoteParamsInput {
    /// Revealed nullifier
    pub nullifier: Nullifier,
    /// Vote commitment
    pub vote_commit: pallas::Point,
    /// Merkle root for the input's inclusion proof
    pub merkle_root: MerkleNode,
    /// Public key used for signing
    pub signature_public: PublicKey,
}

/// State update for `Dao::Vote`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoVoteUpdate {
    /// The proposal bulla being voted on
    pub proposal_bulla: DaoProposalBulla,
    /// The updated proposal metadata
    pub proposal_metadata: DaoProposalMetadata,
    /// Vote nullifiers,
    pub vote_nullifiers: Vec<Nullifier>,
}

/// Represents a single or multiple blinded votes.
/// These can be summed together.
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoBlindAggregateVote {
    /// Weighted vote commit
    pub yes_vote_commit: pallas::Point,
    /// All value staked in the vote
    pub all_vote_commit: pallas::Point,
}

impl DaoBlindAggregateVote {
    /// Aggregate a vote with existing one
    pub fn aggregate(&mut self, other: Self) {
        self.yes_vote_commit += other.yes_vote_commit;
        self.all_vote_commit += other.all_vote_commit;
    }
}

impl Default for DaoBlindAggregateVote {
    fn default() -> Self {
        Self {
            yes_vote_commit: pallas::Point::identity(),
            all_vote_commit: pallas::Point::identity(),
        }
    }
}

/// Parameters for `Dao::Exec`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoExecParams {
    /// The proposal bulla
    pub proposal_bulla: DaoProposalBulla,
    pub proposal_auth_calls: Vec<DaoAuthCall>,
    /// Aggregated blinds for the vote commitments
    pub blind_total_vote: DaoBlindAggregateVote,
}

/// State update for `Dao::Exec`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoExecUpdate {
    /// The proposal bulla
    pub proposal_bulla: DaoProposalBulla,
}

#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoAuthCoinAttrs {
    pub value: pallas::Base,
    pub token_id: pallas::Base,
    pub serial: pallas::Base,
    pub spend_hook: pallas::Base,
    pub user_data: pallas::Base,

    pub ephem_pubkey: PublicKey,
}

/// Parameters for `Dao::AuthMoneyTransfer`
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DaoAuthMoneyTransferParams {
    pub enc_attrs: Vec<DaoAuthCoinAttrs>,
}
