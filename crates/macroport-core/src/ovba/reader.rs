// SPDX-License-Identifier: Apache-2.0
//! A cursor over the records of a `dir` stream.

use super::OvbaError;

/// The first 2 bytes of `bytes` as a little-endian number, if there are that
/// many.
pub(super) fn le_u16(bytes: &[u8]) -> Option<u16> {
    bytes
        .first_chunk::<2>()
        .map(|bytes| u16::from_le_bytes(*bytes))
}

/// The first 4 bytes of `bytes` as a little-endian number, if there are that
/// many.
pub(super) fn le_u32(bytes: &[u8]) -> Option<u32> {
    bytes
        .first_chunk::<4>()
        .map(|bytes| u32::from_le_bytes(*bytes))
}

/// A cursor over the stream. Every read is bounded by what is left.
pub(super) struct Reader<'a> {
    rest: &'a [u8],
    /// The length of the whole stream, to give the offset of an error.
    length: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(stream: &'a [u8]) -> Self {
        Self {
            rest: stream,
            length: stream.len(),
        }
    }

    fn cut_short(&self) -> OvbaError {
        let offset = self.length.saturating_sub(self.rest.len());
        OvbaError::corrupted(
            "ovba.dir.truncated",
            format!("the dir stream ends inside a record at offset {offset}"),
        )
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], OvbaError> {
        let Some((head, tail)) = self.rest.split_at_checked(count) else {
            return Err(self.cut_short());
        };
        self.rest = tail;
        Ok(head)
    }

    pub(super) fn skip(&mut self, count: usize) -> Result<(), OvbaError> {
        self.take(count).map(|_| ())
    }

    pub(super) fn u16(&mut self) -> Result<u16, OvbaError> {
        let Some((bytes, tail)) = self.rest.split_first_chunk::<2>() else {
            return Err(self.cut_short());
        };
        self.rest = tail;
        Ok(u16::from_le_bytes(*bytes))
    }

    fn u32(&mut self) -> Result<u32, OvbaError> {
        let Some((bytes, tail)) = self.rest.split_first_chunk::<4>() else {
            return Err(self.cut_short());
        };
        self.rest = tail;
        Ok(u32::from_le_bytes(*bytes))
    }

    /// The data of a record whose id was just read: its size, then its bytes.
    pub(super) fn record_data(&mut self, id: u16) -> Result<&'a [u8], OvbaError> {
        let size = self.u32()?;
        let data = usize::try_from(size)
            .ok()
            .and_then(|size| self.rest.split_at_checked(size));
        let Some((data, tail)) = data else {
            return Err(OvbaError::corrupted(
                "ovba.dir.truncated",
                format!(
                    "record {id:#06x} declares {size} bytes and {} remain",
                    self.rest.len()
                ),
            ));
        };
        self.rest = tail;
        Ok(data)
    }
}
