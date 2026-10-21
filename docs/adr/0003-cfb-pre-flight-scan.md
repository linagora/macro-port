# ADR-0003: Pre-flight scan of CFB tables

Date: 2026-10-21 · Status: proposed (to validate) · Decided by: night agent for ticket #33, to be validated by Michel-Marie Maudet (LINAGORA)

## Context

SPEC M1 keeps the peak memory per file under 256 MB and bounds a compound file: 2,000 directory entries, storages 32 deep, streams of 64 MB. ADR-0002 chose `cfb` 0.15.0 and would reconsider an own reader only if fuzzing showed `cfb` unfit.

`CompoundFile::open` allocates from what the tables of a container claim before it compares them with the length of the file (`open_internal` in `lib.rs` of cfb 0.15.0, then `Allocator::new` in `internal/alloc.rs`, which makes the comparison):

- It builds the whole FAT from the DIFAT first: one sector of FAT for each DIFAT entry, whichever sector the entry names and however often. Only then does it compare the length of the FAT with the number of sectors of the file.
- It reads the whole directory chain, and the whole MiniFAT chain, whatever the entry limit of SPEC M1 says.

A small file can therefore ask for much more memory than its length. Fuzzing for crashes does not show it: the allocation succeeds, then the crate returns an error.

## Measures

A throwaway probe (release build, rustc 1.99.1, macOS, `/usr/bin/time -l`, 2026-10-21) writes a container whose 109 header entries and chained DIFAT sectors all list sector 1, then opens it with `cfb` alone and with `CfbFile::open`.

- Version 3 (512-byte sectors), 1,026,560 bytes: `cfb` alone peaks at 137.7 MB, about 134 times the file, and says "Malformed FAT" after about 120 ms. With 514,560 bytes: 70.6 MB.
- Version 4 (4,096-byte sectors), 278,528 bytes: 269.4 MB, about 967 times the file. With 430,080 bytes: 424.8 MB.
- The same four files through `CfbFile::open` are refused in 12 to 29 microseconds, at the 2.6 to 3.2 MB the probe needs anyway.
- By arithmetic (not measured), a version 4 file of 200 MB, the default maximum input of SPEC M1, could ask for about 200 GB, which the allocator may answer by aborting the process.
- The largest `.xls` fixture (266,752 bytes), five runs of each in separate processes: 26 to 31 microseconds with `cfb` alone (and 104 on a cold first run), 29 to 33 through the core. The difference is noise.

## Decisions

Keep `cfb`. Before `CompoundFile::open`, `CfbFile::open` scans the tables on the raw bytes (`container/cfb/bounds.rs`), reading the header, the DIFAT sectors and the FAT entries of two chains, and refuses what a file of that length cannot need:

- The DIFAT lists at most `sectors / (sector_len / 4) + 16` FAT sectors, and its chain has at most as many DIFAT sectors (`cfb.fat`).
- The directory chain has at most as many sectors as `MAX_DIRECTORY_ENTRIES` entries fill: 500 in version 3, 63 in version 4 (`cfb.too_many_entries`).
- The MiniFAT chain has at most `sectors / 16 + 2` sectors (`cfb.fat`).

The scan only raises limits. A table that is odd in any other way (a loop, a start or a value out of range) is left to the crate, which names it, and the scan goes on to the next table. A pointer beyond the file ends a chain, as it does in the crate, which repairs it so (`Allocator::validate`, permissive mode) instead of refusing the file: the crate then goes on to read the tables after it. A chain that goes on past its limit is refused for its length, whether or not it also loops.

The tolerances are choices. The 16 FAT sectors beyond what the length calls for allow a writer to reserve some that it does not fill: 8 KB in version 3, 64 KB in version 4. All the `.xls` fixtures and all the `vbaProject.bin` parts of the `.xlsm` and `.xlsb` fixtures pass the scan. A MiniFAT sector lists `sector_len / 4` mini sectors of 64 bytes, which fill 16 sectors of the file in either version, and the mini stream lives in sectors of the file, so a MiniFAT of more than `sectors / 16` sectors lists more than the file holds: two sectors are added for the rounding and a spare.

What the crate can allocate after the scan follows the length of the file: the FAT takes 4 bytes for each sector of the file and 16 sectors more, the directory about 256 KB, the MiniFAT a fraction of the file.

## Alternatives rejected

- **An own MS-CFB reader, or a fork of `cfb`.** ADR-0002 keeps the reader for the case where fuzzing shows `cfb` unfit. The problem is one precondition that can be checked in front of the crate, and either choice is more code to fuzz or to maintain.
- **A cap on the file size.** SPEC M1 accepts files up to 200 MB by default, and a version 4 bomb of 278,528 bytes asks for 269 MB: no cap that admits what the SPEC admits bounds the allocation.
- **A metered reader**, as the OOXML layer does for zip. The crate allocates from the numbers of the header, not from the bytes it reads, so a budget on bytes never sees it.
- **Counting the used directory slots only.** It needs the directory read, and the bound on sectors needs no read. The cost is below.

## Consequences

- A directory chain of more sectors than 2,000 entries fill is refused even if most of its entries are free. No fixture has one and I know of no writer that would (a hypothesis: no real file at hand shows it).
- The scan mirrors how the crate assembles the DIFAT: the header entries up to the first free one, the chain, and the trailing zeros that some writers use as padding. An upgrade of `cfb` can change that; the corpus tests (which open every `.xls` and every `vbaProject.bin` of the fixtures) and the boundary tests in `tests/compound_file/bounds.rs` catch a divergence that refuses a valid file, but not one that lets a bomb through, so the measure above should be repeated on an upgrade.
- SPEC M1 sets 2,000 entries for the container and 2,000 for the modules of a project. A project of 2,000 modules has more than 2,000 entries, so the SPEC cannot hold both. The container limit wins here.
