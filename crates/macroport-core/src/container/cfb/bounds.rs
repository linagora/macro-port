// SPDX-License-Identifier: Apache-2.0
//! A scan of the tables of a container, before the `cfb` crate reads them.
//!
//! `cfb` 0.15.0 builds the whole FAT from the DIFAT before it compares the FAT
//! with the length of the file. Each DIFAT entry costs a sector of FAT in
//! memory, whichever sector it names and however often: 128 times the entry's
//! 4 bytes in a file of 512-byte sectors, 1,024 times in a file of 4,096-byte
//! sectors. A file of 1 MB can thus ask for 137 MB, and one of 278 KB for
//! 269 MB (ADR-0003). The crate also reads the whole directory chain and the
//! whole MiniFAT chain, whatever the limits of SPEC M1 say.
//!
//! [`check`] walks these tables on the raw bytes first and refuses those that a
//! file of that length cannot need. It only raises limits: whatever else looks
//! wrong (a loop, a start or a value out of range) is left to the crate, which
//! names it better. A chain that goes on past its limit is refused for its
//! length, whether or not it also loops.

use std::collections::BTreeSet;

use super::{HEADER_LEN, MAX_DIRECTORY_ENTRIES};
use crate::container::ContainerError;
use crate::report::Status;

const FAT_ENTRY_LEN: usize = 4;
const DIR_ENTRY_LEN: usize = 128;
const MINI_SECTOR_LEN: usize = 64;
const DIFAT_IN_HEADER: usize = 109;

const MAX_REGULAR_SECTOR: u32 = 0xFFFF_FFFA;
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const FREE_SECTOR: u32 = 0xFFFF_FFFF;

// Where the fields sit in the header (MS-CFB 2.2).
const MAJOR_VERSION: usize = 0x1A;
const SECTOR_SHIFT: usize = 0x1E;
const FAT_SECTOR_COUNT: usize = 0x2C;
const FIRST_DIRECTORY_SECTOR: usize = 0x30;
const FIRST_MINIFAT_SECTOR: usize = 0x3C;
const FIRST_DIFAT_SECTOR: usize = 0x44;
const HEADER_DIFAT: usize = 0x4C;

/// FAT sectors that the DIFAT may list beyond the ones the length of the file
/// calls for. A writer may reserve a few that it does not fill; sixteen cost
/// 8 KB in a file of 512-byte sectors and 64 KB in one of 4,096.
const FAT_SLACK: usize = 16;

/// A MiniFAT sector lists `sector_len / 4` mini sectors of 64 bytes, which is
/// 16 sectors whatever the version. The mini stream lives in sectors of the
/// file, so a file of N sectors needs N / 16 MiniFAT sectors at most.
const SECTORS_PER_MINIFAT_SECTOR: usize = MINI_SECTOR_LEN / FAT_ENTRY_LEN;

/// MiniFAT sectors beyond that: one for the rounding, one for a spare.
const MINIFAT_SLACK: usize = 2;

/// Refuses a container whose tables are larger than a file of this length can
/// need.
pub(super) fn check(bytes: &[u8]) -> Result<(), ContainerError> {
    match scan(bytes) {
        Ok(()) | Err(Stop::Deferred) => Ok(()),
        Err(Stop::Over(error)) => Err(error),
    }
}

/// Why the scan stopped before the end of the tables.
enum Stop {
    /// A table is odd in a way that is no limit (a loop, a sector beyond the
    /// file, a value out of range): the `cfb` crate describes it better.
    Deferred,
    /// A table crosses a limit.
    Over(ContainerError),
}

/// A limit crossed, as `Stop::Over`.
fn over(code: &'static str, message: String) -> Stop {
    Stop::Over(ContainerError::new(Status::Corrupted, code, message))
}

fn scan(bytes: &[u8]) -> Result<(), Stop> {
    let header = Header::parse(bytes).ok_or(Stop::Deferred)?;
    let fat = Fat {
        bytes,
        difat: header.read_difat(bytes)?,
        header: &header,
    };
    // A directory that is left to the crate does not excuse the MiniFAT: the
    // crate may well accept the one and then read the other.
    match fat.walk(header.first_directory, header.directory_limit, || {
        let limit = header.directory_limit;
        over(
            "cfb.too_many_entries",
            format!(
                "the directory takes more than {limit} sectors, which is more than \
                 {MAX_DIRECTORY_ENTRIES} entries need"
            ),
        )
    }) {
        Err(over @ Stop::Over(_)) => return Err(over),
        Ok(()) | Err(Stop::Deferred) => {}
    }
    fat.walk(header.first_minifat, header.minifat_limit, || {
        let (limit, sector_count) = (header.minifat_limit, header.sector_count);
        over(
            "cfb.fat",
            format!(
                "the MiniFAT takes more than {limit} sectors, more than a file of \
                 {sector_count} sectors can need"
            ),
        )
    })
}

/// What the scan reads of the header, and the limits that follow from it.
struct Header {
    /// 512 for version 3, 4,096 for version 4.
    sector_len: usize,
    /// The sectors after the header sector: the length of the file in sectors,
    /// less one.
    sector_count: usize,
    /// The number of FAT sectors that the header claims.
    fat_sectors: usize,
    first_directory: u32,
    first_minifat: u32,
    first_difat: u32,
    /// The entries of the header's DIFAT up to the first free one, padded with
    /// free ones to 109.
    initial_difat: Vec<u32>,
    /// The most FAT sectors, and DIFAT sectors, that the scan allows a file of
    /// this length: what it can need, and `FAT_SLACK` more.
    fat_limit: usize,
    /// The most sectors that a directory of `MAX_DIRECTORY_ENTRIES` entries
    /// takes.
    directory_limit: usize,
    /// The most sectors that the scan allows the MiniFAT of a file of this
    /// length: what it can need, and `MINIFAT_SLACK` more.
    minifat_limit: usize,
}

impl Header {
    /// `None` for a header that the crate refuses: it says why.
    fn parse(bytes: &[u8]) -> Option<Self> {
        let header = bytes.get(..HEADER_LEN)?;
        let version = (
            le_u16(header, MAJOR_VERSION)?,
            le_u16(header, SECTOR_SHIFT)?,
        );
        let sector_len = match version {
            (3, 9) => 512,
            (4, 12) => 4096,
            _ => return None,
        };
        let sector_count = bytes.len().div_ceil(sector_len).checked_sub(1)?;

        let mut initial_difat = Vec::with_capacity(DIFAT_IN_HEADER);
        for chunk in header.get(HEADER_DIFAT..)?.as_chunks::<4>().0 {
            match u32::from_le_bytes(*chunk) {
                FREE_SECTOR => break,
                entry if entry > MAX_REGULAR_SECTOR => return None,
                entry => initial_difat.push(entry),
            }
        }
        initial_difat.resize(DIFAT_IN_HEADER, FREE_SECTOR);

        let fat_entries = sector_len / FAT_ENTRY_LEN;
        Some(Self {
            sector_len,
            sector_count,
            fat_sectors: usize::try_from(le_u32(header, FAT_SECTOR_COUNT)?).ok()?,
            first_directory: le_u32(header, FIRST_DIRECTORY_SECTOR)?,
            first_minifat: le_u32(header, FIRST_MINIFAT_SECTOR)?,
            first_difat: le_u32(header, FIRST_DIFAT_SECTOR)?,
            initial_difat,
            fat_limit: sector_count
                .checked_div(fat_entries)?
                .saturating_add(FAT_SLACK),
            directory_limit: MAX_DIRECTORY_ENTRIES.div_ceil(sector_len / DIR_ENTRY_LEN),
            minifat_limit: (sector_count / SECTORS_PER_MINIFAT_SECTOR)
                .saturating_add(MINIFAT_SLACK),
        })
    }

    /// Whether the file holds a sector of this number.
    fn has_sector(&self, sector: u32) -> bool {
        usize::try_from(sector).is_ok_and(|sector| sector < self.sector_count)
    }

    /// The bytes of `sector`, if the file holds all of it.
    fn sector<'a>(&self, bytes: &'a [u8], sector: u32) -> Option<&'a [u8]> {
        let start = usize::try_from(sector)
            .ok()?
            .checked_add(1)?
            .checked_mul(self.sector_len)?;
        bytes.get(start..start.checked_add(self.sector_len)?)
    }

    /// The DIFAT as the `cfb` crate assembles it: the entries of the header,
    /// then those of the DIFAT sectors, less the trailing ones that are only
    /// padding. Refused when it lists more FAT sectors than the file can need.
    fn read_difat(&self, bytes: &[u8]) -> Result<Vec<u32>, Stop> {
        let limit = self.fat_limit;
        let too_large = || {
            over(
                "cfb.fat",
                format!(
                    "the DIFAT is larger than a file of {} sectors can need: \
                     {limit} FAT sectors at most",
                    self.sector_count
                ),
            )
        };

        let mut difat = self.initial_difat.clone();
        let mut seen = BTreeSet::new();
        let mut sector = self.first_difat;
        while sector != END_OF_CHAIN && sector != FREE_SECTOR {
            if !self.has_sector(sector) || !seen.insert(sector) {
                return Err(Stop::Deferred);
            }
            // A DIFAT sector that lists no FAT sector is padding, which no
            // writer chains by the dozen: the same limit bounds the chain, and
            // so the entries read here.
            if seen.len() > limit {
                return Err(too_large());
            }
            let body = self.sector(bytes, sector).ok_or(Stop::Deferred)?;
            let (entries, next) = body.split_last_chunk::<4>().ok_or(Stop::Deferred)?;
            for chunk in entries.as_chunks::<4>().0 {
                let entry = u32::from_le_bytes(*chunk);
                if entry != FREE_SECTOR && entry > MAX_REGULAR_SECTOR {
                    return Err(Stop::Deferred);
                }
                difat.push(entry);
            }
            sector = u32::from_le_bytes(*next);
        }

        // Some writers pad the last DIFAT sector with zeros, not free entries.
        // The crate drops them when the header counts fewer FAT sectors.
        while difat.len() > DIFAT_IN_HEADER
            && difat.len() > self.fat_sectors
            && difat.last() == Some(&0)
        {
            difat.pop();
        }
        while difat.last() == Some(&FREE_SECTOR) {
            difat.pop();
        }
        if difat.len() > limit {
            return Err(too_large());
        }
        Ok(difat)
    }
}

/// The FAT, reached through the DIFAT without being built.
struct Fat<'a> {
    bytes: &'a [u8],
    header: &'a Header,
    difat: Vec<u32>,
}

impl Fat<'_> {
    /// The FAT entry of `sector`: `None` when no FAT sector holds it.
    fn next(&self, sector: u32) -> Option<u32> {
        let per_sector = self.header.sector_len / FAT_ENTRY_LEN;
        let index = usize::try_from(sector).ok()?;
        let fat_sector = *self.difat.get(index.checked_div(per_sector)?)?;
        let body = self.header.sector(self.bytes, fat_sector)?;
        let entry = index.checked_rem(per_sector)?.checked_mul(FAT_ENTRY_LEN)?;
        le_u32(body, entry)
    }

    /// Follows the chain that starts at `start` and refuses it when it passes
    /// `limit` sectors. A chain that does not end properly (a loop, a start or
    /// a value out of range) is left to the crate.
    fn walk(&self, start: u32, limit: usize, too_long: impl FnOnce() -> Stop) -> Result<(), Stop> {
        let mut seen = BTreeSet::new();
        let mut sector = start;
        while sector != END_OF_CHAIN {
            if !self.header.has_sector(sector) || !seen.insert(sector) {
                return Err(Stop::Deferred);
            }
            if seen.len() > limit {
                return Err(too_long());
            }
            sector = self.next(sector).ok_or(Stop::Deferred)?;
            // The crate ends the chain at a pointer beyond the file, rather
            // than refuse the file (`Allocator::validate`), and so does the walk.
            if sector <= MAX_REGULAR_SECTOR && !self.header.has_sector(sector) {
                sector = END_OF_CHAIN;
            }
        }
        Ok(())
    }
}

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let field = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes(field.try_into().ok()?))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(field.try_into().ok()?))
}
