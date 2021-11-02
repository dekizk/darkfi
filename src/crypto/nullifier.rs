use std::io;

use pasta_curves::{arithmetic::FieldExt, pallas};

use crate::{
    serial::{Decodable, Encodable, ReadExt, WriteExt},
    Result,
};

#[derive(Clone, Copy, Debug)]
pub struct Nullifier(pallas::Base);

impl Nullifier {
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        pallas::Base::from_bytes(bytes).map(Nullifier).unwrap()
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub(crate) fn inner(&self) -> pallas::Base {
        self.0
    }
}

impl Encodable for Nullifier {
    fn encode<S: io::Write>(&self, mut s: S) -> Result<usize> {
        s.write_slice(&self.to_bytes()[..])?;
        Ok(32)
    }
}

impl Decodable for Nullifier {
    fn decode<D: io::Read>(mut d: D) -> Result<Self> {
        let mut bytes = [0u8; 32];
        d.read_slice(&mut bytes)?;
        let result = Self::from_bytes(&bytes);
        Ok(result)
    }
}
