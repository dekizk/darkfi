use std::io;

use crate::{
    crypto::{address::Address, keypair::PublicKey, schnorr::Signature},
    impl_vec, net,
    util::serial::{Decodable, Encodable, SerialDecodable, SerialEncodable, VarInt},
    Result,
};

/// This struct represents a `Vote` used by the Streamlet consensus
#[derive(Debug, Clone, PartialEq, Eq, SerialDecodable, SerialEncodable)]
pub struct Vote {
    /// Node public key
    pub public_key: PublicKey,
    /// Block signature
    pub vote: Signature,
    /// Block proposal hash to vote on
    pub proposal: blake3::Hash,
    /// Slot uid, generated by the beacon
    pub slot: u64,
    /// Node wallet address
    pub address: Address,
}

impl Vote {
    pub fn new(
        public_key: PublicKey,
        vote: Signature,
        proposal: blake3::Hash,
        slot: u64,
        address: Address,
    ) -> Self {
        Self { public_key, vote, proposal, slot, address }
    }
}

impl net::Message for Vote {
    fn name() -> &'static str {
        "vote"
    }
}

impl_vec!(Vote);
