/* This file is part of DarkFi (https://dark.fi)
 *
 * Copyright (C) 2020-2024 Dyne.org foundation
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

use darkfi_sdk::{
    crypto::{
        schnorr::{SchnorrSecret, Signature},
        MerkleTree, SecretKey,
    },
    pasta::{group::ff::FromUniformBytes, pallas},
    tx::TransactionHash,
};
#[cfg(feature = "async-serial")]
use darkfi_serial::async_trait;

use darkfi_serial::{deserialize, serialize, SerialDecodable, SerialEncodable};
use num_bigint::BigUint;

use crate::{tx::Transaction, util::time::Timestamp, Error, Result};

use super::{parse_record, parse_u64_key_record, Header, HeaderHash, SledDbOverlayPtr};

/// This struct represents a tuple of the form (`header`, `txs`, `signature`).
/// The header and transactions are stored as hashes, serving as pointers to the actual data
/// in the sled database.
/// NOTE: This struct fields are considered final, as it represents a blockchain block.
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct Block {
    /// Block header
    pub header: HeaderHash,
    /// Trasaction hashes
    pub txs: Vec<TransactionHash>,
    /// Block producer signature
    pub signature: Signature,
}

impl Block {
    pub fn new(header: HeaderHash, txs: Vec<TransactionHash>, signature: Signature) -> Self {
        Self { header, txs, signature }
    }

    /// A block's hash is the same as the hash of its header
    pub fn hash(&self) -> HeaderHash {
        self.header
    }

    /// Generate a `Block` from a `BlockInfo`
    pub fn from_block_info(block_info: &BlockInfo) -> Self {
        let header = block_info.header.hash();
        let txs = block_info.txs.iter().map(|tx| tx.hash()).collect();
        let signature = block_info.signature;
        Self { header, txs, signature }
    }
}

/// Structure representing full block data, acting as
/// a wrapper struct over `Block`, enabling us to include
/// more information that might be used in different block
/// version, without affecting the original struct.
#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct BlockInfo {
    /// Block header data
    pub header: Header,
    /// Transactions payload
    pub txs: Vec<Transaction>,
    /// Block producer signature
    pub signature: Signature,
}

impl Default for BlockInfo {
    /// Represents the genesis block on current timestamp
    fn default() -> Self {
        Self {
            header: Header::default(),
            txs: vec![Transaction::default()],
            signature: Signature::dummy(),
        }
    }
}

impl BlockInfo {
    pub fn new(header: Header, txs: Vec<Transaction>, signature: Signature) -> Self {
        Self { header, txs, signature }
    }

    /// Generate an empty block for provided Header.
    /// Transactions and the producer signature must be added after.
    pub fn new_empty(header: Header) -> Self {
        let txs = vec![];
        let signature = Signature::dummy();
        Self { header, txs, signature }
    }

    /// A block's hash is the same as the hash of its header
    pub fn hash(&self) -> HeaderHash {
        self.header.hash()
    }

    /// Append a transaction to the block. Also adds it to the Merkle tree.
    pub fn append_tx(&mut self, tx: Transaction) {
        append_tx_to_merkle_tree(&mut self.header.tree, &tx);
        self.txs.push(tx);
    }

    /// Append a vector of transactions to the block. Also adds them to the
    /// Merkle tree.
    pub fn append_txs(&mut self, txs: Vec<Transaction>) {
        for tx in txs {
            self.append_tx(tx);
        }
    }

    /// Sign block header using provided secret key
    // TODO: sign more stuff?
    pub fn sign(&mut self, secret_key: &SecretKey) {
        self.signature = secret_key.sign(self.hash().inner());
    }
}

/// Auxiliary structure used to keep track of blocks order.
#[derive(Debug, SerialEncodable, SerialDecodable)]
pub struct BlockOrder {
    /// Order number
    pub number: u64,
    /// Block headerhash of that number
    pub block: HeaderHash,
}

/// Auxiliary structure used to keep track of block ranking information.
/// Note: we only need height cummulative ranks, but we also keep its actual
/// ranks, so we can verify the sequence and/or know specific block height
/// ranks, if ever needed.
#[derive(Debug)]
pub struct BlockRanks {
    /// Block target rank
    pub target_rank: BigUint,
    /// Height cummulative targets rank
    pub targets_rank: BigUint,
    /// Block hash rank
    pub hash_rank: BigUint,
    /// Height cummulative hashes rank
    pub hashes_rank: BigUint,
}

impl BlockRanks {
    pub fn new(
        target_rank: BigUint,
        targets_rank: BigUint,
        hash_rank: BigUint,
        hashes_rank: BigUint,
    ) -> Self {
        Self { target_rank, targets_rank, hash_rank, hashes_rank }
    }
}

// Note: Doing all the imports here as this might get obselete if
// we implemented Encodable/Decodable for num_bigint::BigUint.
impl darkfi_serial::Encodable for BlockRanks {
    fn encode<S: std::io::Write>(&self, mut s: S) -> std::io::Result<usize> {
        let mut len = 0;
        len += self.target_rank.to_bytes_be().encode(&mut s)?;
        len += self.targets_rank.to_bytes_be().encode(&mut s)?;
        len += self.hash_rank.to_bytes_be().encode(&mut s)?;
        len += self.hashes_rank.to_bytes_be().encode(&mut s)?;
        Ok(len)
    }
}

impl darkfi_serial::Decodable for BlockRanks {
    fn decode<D: std::io::Read>(mut d: D) -> std::io::Result<Self> {
        let bytes: Vec<u8> = darkfi_serial::Decodable::decode(&mut d)?;
        let target_rank: BigUint = BigUint::from_bytes_be(&bytes);
        let bytes: Vec<u8> = darkfi_serial::Decodable::decode(&mut d)?;
        let targets_rank: BigUint = BigUint::from_bytes_be(&bytes);
        let bytes: Vec<u8> = darkfi_serial::Decodable::decode(&mut d)?;
        let hash_rank: BigUint = BigUint::from_bytes_be(&bytes);
        let bytes: Vec<u8> = darkfi_serial::Decodable::decode(&mut d)?;
        let hashes_rank: BigUint = BigUint::from_bytes_be(&bytes);
        let ret = Self { target_rank, targets_rank, hash_rank, hashes_rank };
        Ok(ret)
    }
}

/// Auxiliary structure used to keep track of block PoW difficulty information.
/// Note: we only need height cummulative difficulty, but we also keep its actual
/// difficulty, so we can verify the sequence and/or know specific block height
/// difficulty, if ever needed.
#[derive(Debug)]
pub struct BlockDifficulty {
    /// Block height number
    pub height: u64,
    /// Block creation timestamp
    pub timestamp: Timestamp,
    /// Height difficulty
    pub difficulty: BigUint,
    /// Height cummulative difficulty (total + height difficulty)
    pub cummulative_difficulty: BigUint,
    /// Block ranks
    pub ranks: BlockRanks,
}

impl BlockDifficulty {
    pub fn new(
        height: u64,
        timestamp: Timestamp,
        difficulty: BigUint,
        cummulative_difficulty: BigUint,
        ranks: BlockRanks,
    ) -> Self {
        Self { height, timestamp, difficulty, cummulative_difficulty, ranks }
    }

    /// Represents the genesis block difficulty
    pub fn genesis(timestamp: Timestamp) -> Self {
        let ranks = BlockRanks::new(
            BigUint::from(0u64),
            BigUint::from(0u64),
            BigUint::from(0u64),
            BigUint::from(0u64),
        );
        BlockDifficulty::new(0, timestamp, BigUint::from(0u64), BigUint::from(0u64), ranks)
    }
}

// Note: Doing all the imports here as this might get obselete if
// we implemented Encodable/Decodable for num_bigint::BigUint.
impl darkfi_serial::Encodable for BlockDifficulty {
    fn encode<S: std::io::Write>(&self, mut s: S) -> std::io::Result<usize> {
        let mut len = 0;
        len += self.height.encode(&mut s)?;
        len += self.timestamp.encode(&mut s)?;
        len += self.difficulty.to_bytes_be().encode(&mut s)?;
        len += self.cummulative_difficulty.to_bytes_be().encode(&mut s)?;
        len += self.ranks.encode(&mut s)?;
        Ok(len)
    }
}

impl darkfi_serial::Decodable for BlockDifficulty {
    fn decode<D: std::io::Read>(mut d: D) -> std::io::Result<Self> {
        let height: u64 = darkfi_serial::Decodable::decode(&mut d)?;
        let timestamp: Timestamp = darkfi_serial::Decodable::decode(&mut d)?;
        let bytes: Vec<u8> = darkfi_serial::Decodable::decode(&mut d)?;
        let difficulty: BigUint = BigUint::from_bytes_be(&bytes);
        let bytes: Vec<u8> = darkfi_serial::Decodable::decode(&mut d)?;
        let cummulative_difficulty: BigUint = BigUint::from_bytes_be(&bytes);
        let ranks: BlockRanks = darkfi_serial::Decodable::decode(&mut d)?;
        let ret = Self { height, timestamp, difficulty, cummulative_difficulty, ranks };
        Ok(ret)
    }
}

const SLED_BLOCK_TREE: &[u8] = b"_blocks";
const SLED_BLOCK_ORDER_TREE: &[u8] = b"_block_order";
const SLED_BLOCK_DIFFICULTY_TREE: &[u8] = b"_block_difficulty";

/// The `BlockStore` is a structure representing all `sled` trees related
/// to storing the blockchain's blocks information.
#[derive(Clone)]
pub struct BlockStore {
    /// Main `sled` tree, storing all the blockchain's blocks, where the
    /// key is the blocks' hash, and value is the serialized block.
    pub main: sled::Tree,
    /// The `sled` tree storing the order of the blockchain's blocks,
    /// where the key is the order number, and the value is the blocks'
    /// hash.
    pub order: sled::Tree,
    /// The `sled` tree storing the the difficulty information of the
    /// blockchain's blocks, where the key is the block height number,
    /// and the value is the blocks' hash.
    pub difficulty: sled::Tree,
}

impl BlockStore {
    /// Opens a new or existing `BlockStore` on the given sled database.
    pub fn new(db: &sled::Db) -> Result<Self> {
        let main = db.open_tree(SLED_BLOCK_TREE)?;
        let order = db.open_tree(SLED_BLOCK_ORDER_TREE)?;
        let difficulty = db.open_tree(SLED_BLOCK_DIFFICULTY_TREE)?;
        Ok(Self { main, order, difficulty })
    }

    /// Insert a slice of [`Block`] into the store's main tree.
    pub fn insert(&self, blocks: &[Block]) -> Result<Vec<HeaderHash>> {
        let (batch, ret) = self.insert_batch(blocks);
        self.main.apply_batch(batch)?;
        Ok(ret)
    }

    /// Insert a slice of `u64` and block hashes into the store's
    /// order tree.
    pub fn insert_order(&self, order: &[u64], hashes: &[HeaderHash]) -> Result<()> {
        let batch = self.insert_batch_order(order, hashes);
        self.order.apply_batch(batch)?;
        Ok(())
    }

    /// Insert a slice of [`BlockDifficulty`] into the store's
    /// difficulty tree.
    pub fn insert_difficulty(&self, block_difficulties: &[BlockDifficulty]) -> Result<()> {
        let batch = self.insert_batch_difficulty(block_difficulties);
        self.difficulty.apply_batch(batch)?;
        Ok(())
    }

    /// Generate the sled batch corresponding to an insert to the main
    /// tree, so caller can handle the write operation.
    /// The block's hash() function output is used as the key,
    /// while value is the serialized [`Block`] itself.
    /// On success, the function returns the block hashes in the same order.
    pub fn insert_batch(&self, blocks: &[Block]) -> (sled::Batch, Vec<HeaderHash>) {
        let mut ret = Vec::with_capacity(blocks.len());
        let mut batch = sled::Batch::default();

        for block in blocks {
            let blockhash = block.hash();
            batch.insert(blockhash.inner(), serialize(block));
            ret.push(blockhash);
        }

        (batch, ret)
    }

    /// Generate the sled batch corresponding to an insert to the order
    /// tree, so caller can handle the write operation.
    /// The block order number is used as the key, and the block hash is used as value.
    pub fn insert_batch_order(&self, order: &[u64], hashes: &[HeaderHash]) -> sled::Batch {
        let mut batch = sled::Batch::default();

        for (i, number) in order.iter().enumerate() {
            batch.insert(&number.to_be_bytes(), hashes[i].inner());
        }

        batch
    }

    /// Generate the sled batch corresponding to an insert to the difficulty
    /// tree, so caller can handle the write operation.
    /// The block's height number is used as the key, while value is
    //  the serialized [`BlockDifficulty`] itself.
    pub fn insert_batch_difficulty(&self, block_difficulties: &[BlockDifficulty]) -> sled::Batch {
        let mut batch = sled::Batch::default();

        for block_difficulty in block_difficulties {
            batch.insert(&block_difficulty.height.to_be_bytes(), serialize(block_difficulty));
        }

        batch
    }

    /// Check if the store's main tree contains a given block hash.
    pub fn contains(&self, blockhash: &HeaderHash) -> Result<bool> {
        Ok(self.main.contains_key(blockhash.inner())?)
    }

    /// Check if the store's order tree contains a given order number.
    pub fn contains_order(&self, number: u64) -> Result<bool> {
        Ok(self.order.contains_key(number.to_be_bytes())?)
    }

    /// Fetch given block hashes from the store's main tree.
    /// The resulting vector contains `Option`, which is `Some` if the block
    /// was found in the block store, and otherwise it is `None`, if it has not.
    /// The second parameter is a boolean which tells the function to fail in
    /// case at least one block was not found.
    pub fn get(&self, block_hashes: &[HeaderHash], strict: bool) -> Result<Vec<Option<Block>>> {
        let mut ret = Vec::with_capacity(block_hashes.len());

        for hash in block_hashes {
            if let Some(found) = self.main.get(hash.inner())? {
                let block = deserialize(&found)?;
                ret.push(Some(block));
                continue
            }
            if strict {
                return Err(Error::BlockNotFound(hash.as_string()))
            }
            ret.push(None);
        }

        Ok(ret)
    }

    /// Fetch given order numbers from the store's order tree.
    /// The resulting vector contains `Option`, which is `Some` if the number
    /// was found in the block order store, and otherwise it is `None`, if it has not.
    /// The second parameter is a boolean which tells the function to fail in
    /// case at least one order number was not found.
    pub fn get_order(&self, order: &[u64], strict: bool) -> Result<Vec<Option<HeaderHash>>> {
        let mut ret = Vec::with_capacity(order.len());

        for number in order {
            if let Some(found) = self.order.get(number.to_be_bytes())? {
                let block_hash = deserialize(&found)?;
                ret.push(Some(block_hash));
                continue
            }
            if strict {
                return Err(Error::BlockNumberNotFound(*number))
            }
            ret.push(None);
        }

        Ok(ret)
    }

    /// Fetch given block height numbers from the store's difficulty tree.
    /// The resulting vector contains `Option`, which is `Some` if the block
    /// height number was found in the block difficulties store, and otherwise
    /// it is `None`, if it has not.
    /// The second parameter is a boolean which tells the function to fail in
    /// case at least one block height number was not found.
    pub fn get_difficulty(
        &self,
        heights: &[u64],
        strict: bool,
    ) -> Result<Vec<Option<BlockDifficulty>>> {
        let mut ret = Vec::with_capacity(heights.len());

        for height in heights {
            if let Some(found) = self.difficulty.get(height.to_be_bytes())? {
                let block_difficulty = deserialize(&found)?;
                ret.push(Some(block_difficulty));
                continue
            }
            if strict {
                return Err(Error::BlockDifficultyNotFound(*height))
            }
            ret.push(None);
        }

        Ok(ret)
    }

    /// Retrieve all blocks from the store's main tree in the form of a
    /// tuple (`hash`, `block`).
    /// Be careful as this will try to load everything in memory.
    pub fn get_all(&self) -> Result<Vec<(HeaderHash, Block)>> {
        let mut blocks = vec![];

        for block in self.main.iter() {
            blocks.push(parse_record(block.unwrap())?);
        }

        Ok(blocks)
    }

    /// Retrieve complete order from the store's order tree in the form
    /// of a vector containing (`number`, `hash`) tuples.
    /// Be careful as this will try to load everything in memory.
    pub fn get_all_order(&self) -> Result<Vec<(u64, HeaderHash)>> {
        let mut order = vec![];

        for record in self.order.iter() {
            order.push(parse_u64_key_record(record.unwrap())?);
        }

        Ok(order)
    }

    /// Retrieve all block difficulties from the store's difficulty tree in
    /// the form of a vector containing (`height`, `difficulty`) tuples.
    /// Be careful as this will try to load everything in memory.
    pub fn get_all_difficulty(&self) -> Result<Vec<(u64, BlockDifficulty)>> {
        let mut block_difficulties = vec![];

        for record in self.difficulty.iter() {
            block_difficulties.push(parse_u64_key_record(record.unwrap())?);
        }

        Ok(block_difficulties)
    }

    /// Fetch n hashes after given order number. In the iteration, if an order
    /// number is not found, the iteration stops and the function returns what
    /// it has found so far in the `BlockOrderStore`.
    pub fn get_after(&self, number: u64, n: u64) -> Result<Vec<HeaderHash>> {
        let mut ret = vec![];

        let mut key = number;
        let mut counter = 0;
        while counter <= n {
            if let Some(found) = self.order.get_gt(key.to_be_bytes())? {
                let (number, hash) = parse_u64_key_record(found)?;
                key = number;
                ret.push(hash);
                counter += 1;
                continue
            }
            break
        }

        Ok(ret)
    }

    /// Fetch the first block hash in the order tree, based on the `Ord`
    /// implementation for `Vec<u8>`.
    pub fn get_first(&self) -> Result<(u64, HeaderHash)> {
        let found = match self.order.first()? {
            Some(s) => s,
            None => return Err(Error::BlockNumberNotFound(0)),
        };
        let (number, hash) = parse_u64_key_record(found)?;

        Ok((number, hash))
    }

    /// Fetch the last block hash in the order tree, based on the `Ord`
    /// implementation for `Vec<u8>`.
    pub fn get_last(&self) -> Result<(u64, HeaderHash)> {
        let found = self.order.last()?.unwrap();
        let (number, hash) = parse_u64_key_record(found)?;

        Ok((number, hash))
    }

    /// Fetch the last record in the difficulty tree, based on the `Ord`
    /// implementation for `Vec<u8>`. If the tree is empty,
    /// returns `None`.
    pub fn get_last_difficulty(&self) -> Result<Option<BlockDifficulty>> {
        let Some(found) = self.difficulty.last()? else { return Ok(None) };
        let block_difficulty = deserialize(&found.1)?;
        Ok(Some(block_difficulty))
    }

    /// Fetch the last N records from the difficulty store, in order.
    pub fn get_last_n_difficulties(&self, n: usize) -> Result<Vec<BlockDifficulty>> {
        // Build an iterator to retrieve last N records
        let records = self.difficulty.iter().rev().take(n);
        // Since the iterator grabs in right -> left order,
        // we deserialize found records, and push them in reverse order
        let mut last_n = vec![];
        for record in records {
            last_n.insert(0, deserialize(&record?.1)?);
        }

        Ok(last_n)
    }

    /// Retrieve store's order tree records count.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Check if store's order tree contains any records.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Overlay structure over a [`BlockStore`] instance.
pub struct BlockStoreOverlay(SledDbOverlayPtr);

impl BlockStoreOverlay {
    pub fn new(overlay: &SledDbOverlayPtr) -> Result<Self> {
        overlay.lock().unwrap().open_tree(SLED_BLOCK_TREE)?;
        overlay.lock().unwrap().open_tree(SLED_BLOCK_ORDER_TREE)?;
        overlay.lock().unwrap().open_tree(SLED_BLOCK_DIFFICULTY_TREE)?;
        Ok(Self(overlay.clone()))
    }

    /// Insert a slice of [`Block`] into the overlay's main tree.
    /// The block's hash() function output is used as the key,
    /// while value is the serialized [`Block`] itself.
    /// On success, the function returns the block hashes in the same order.
    pub fn insert(&self, blocks: &[Block]) -> Result<Vec<HeaderHash>> {
        let mut ret = Vec::with_capacity(blocks.len());
        let mut lock = self.0.lock().unwrap();

        for block in blocks {
            let blockhash = block.hash();
            lock.insert(SLED_BLOCK_TREE, blockhash.inner(), &serialize(block))?;
            ret.push(blockhash);
        }

        Ok(ret)
    }

    /// Insert a slice of `u64` and block hashes into overlay's order tree.
    /// The block order number is used as the key, and the blockhash is used as value.
    pub fn insert_order(&self, order: &[u64], hashes: &[HeaderHash]) -> Result<()> {
        if order.len() != hashes.len() {
            return Err(Error::InvalidInputLengths)
        }

        let mut lock = self.0.lock().unwrap();

        for (i, number) in order.iter().enumerate() {
            lock.insert(SLED_BLOCK_ORDER_TREE, &number.to_be_bytes(), hashes[i].inner())?;
        }

        Ok(())
    }

    /// Insert a slice of [`BlockDifficulty`] into the overlay's difficulty tree.
    pub fn insert_difficulty(&self, block_difficulties: &[BlockDifficulty]) -> Result<()> {
        let mut lock = self.0.lock().unwrap();

        for block_difficulty in block_difficulties {
            lock.insert(
                SLED_BLOCK_DIFFICULTY_TREE,
                &block_difficulty.height.to_be_bytes(),
                &serialize(block_difficulty),
            )?;
        }

        Ok(())
    }

    /// Fetch given block hashes from the overlay's main tree.
    /// The resulting vector contains `Option`, which is `Some` if the block
    /// was found in the overlay, and otherwise it is `None`, if it has not.
    /// The second parameter is a boolean which tells the function to fail in
    /// case at least one block was not found.
    pub fn get(&self, block_hashes: &[HeaderHash], strict: bool) -> Result<Vec<Option<Block>>> {
        let mut ret = Vec::with_capacity(block_hashes.len());
        let lock = self.0.lock().unwrap();

        for hash in block_hashes {
            if let Some(found) = lock.get(SLED_BLOCK_TREE, hash.inner())? {
                let block = deserialize(&found)?;
                ret.push(Some(block));
                continue
            }
            if strict {
                return Err(Error::BlockNotFound(hash.as_string()))
            }
            ret.push(None);
        }

        Ok(ret)
    }

    /// Fetch given order numbers from the overlay's order tree.
    /// The resulting vector contains `Option`, which is `Some` if the number
    /// was found in the overlay, and otherwise it is `None`, if it has not.
    /// The second parameter is a boolean which tells the function to fail in
    /// case at least one number was not found.
    pub fn get_order(&self, order: &[u64], strict: bool) -> Result<Vec<Option<HeaderHash>>> {
        let mut ret = Vec::with_capacity(order.len());
        let lock = self.0.lock().unwrap();

        for number in order {
            if let Some(found) = lock.get(SLED_BLOCK_ORDER_TREE, &number.to_be_bytes())? {
                let block_hash = deserialize(&found)?;
                ret.push(Some(block_hash));
                continue
            }
            if strict {
                return Err(Error::BlockNumberNotFound(*number))
            }
            ret.push(None);
        }

        Ok(ret)
    }

    /// Fetch the last block hash in the overlay's order tree, based on the `Ord`
    /// implementation for `Vec<u8>`.
    pub fn get_last(&self) -> Result<(u64, HeaderHash)> {
        let found = match self.0.lock().unwrap().last(SLED_BLOCK_ORDER_TREE)? {
            Some(b) => b,
            None => return Err(Error::BlockNumberNotFound(0)),
        };
        let (number, hash) = parse_u64_key_record(found)?;

        Ok((number, hash))
    }

    /// Check if overlay's order tree contains any records.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.0.lock().unwrap().is_empty(SLED_BLOCK_ORDER_TREE)?)
    }
}

/// Auxiliary function to append a transaction to a Merkle tree.
pub fn append_tx_to_merkle_tree(tree: &mut MerkleTree, tx: &Transaction) {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(tx.hash().inner());
    let leaf = pallas::Base::from_uniform_bytes(&buf);
    tree.append(leaf.into());
}
