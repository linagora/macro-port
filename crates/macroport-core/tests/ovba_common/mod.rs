// SPDX-License-Identifier: Apache-2.0
//! Helpers that the MS-OVBA tests share.

// The helpers build their own inputs: a panic in one is a failed test.
#![allow(clippy::unwrap_used)]

/// A deterministic source of bytes for the tests that throw damaged or random
/// input at a parser: no `rand` crate, and the same input on every run.
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    fn next_byte(&mut self) -> u8 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 24) as u8
    }

    /// `valid` with `count` of its bytes replaced.
    pub fn damage(&mut self, valid: &[u8], count: usize) -> Vec<u8> {
        let mut damaged = valid.to_vec();
        for _ in 0..count {
            let index = usize::from(u16::from_le_bytes([self.next_byte(), self.next_byte()]));
            let index = index.checked_rem(damaged.len()).unwrap();
            *damaged.get_mut(index).unwrap() = self.next_byte();
        }
        damaged
    }

    /// Up to 255 random bytes.
    pub fn random_bytes(&mut self) -> Vec<u8> {
        let length = usize::from(self.next_byte());
        (0..length).map(|_| self.next_byte()).collect()
    }
}
