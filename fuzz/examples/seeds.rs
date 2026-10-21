// SPDX-License-Identifier: Apache-2.0
//! Writes the seed corpora of the fuzz targets from the binary fixtures, which
//! are only read, so that a run starts from real workbooks. The seeds of each
//! target go to `<out>/<target>/`:
//! `cargo run --manifest-path fuzz/Cargo.toml --example seeds -- corpus/fixtures <out>`

use std::error::Error;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::{env, fs};

use macroport_core::container::{CfbFile, read_ooxml};
use macroport_core::ovba::{MAX_DIR_STREAM, decompress, parse_dir};
use zip::ZipArchive;

const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Where the VBA project sits in a compound file: in a workbook (`.xls`) and
/// in a `vbaProject.bin`. Its `VBA` storage and `PROJECT` stream are here.
const PROJECT_ROOTS: [&str; 2] = ["_VBA_PROJECT_CUR/", ""];

/// The XML parts that `ooxml_xml` reads, in the order of its part numbers.
const XML_PARTS: [&str; 3] = [
    "_rels/.rels",
    "xl/_rels/workbook.xml.rels",
    "[Content_Types].xml",
];

/// The most that is read of a part: one fixture is a zip bomb.
const MAX_PART: u64 = 1 << 20;

/// What `ovba_decompress` reads first: the largest limit it can ask for.
const LIMIT_PREFIX: [u8; 2] = u16::MAX.to_le_bytes();

struct Seeds {
    out: PathBuf,
}

impl Seeds {
    fn write(&self, target: &str, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let folder = self.out.join(target);
        fs::create_dir_all(&folder)?;
        fs::write(folder.join(name), bytes)?;
        Ok(())
    }

    /// A fixture: a ZIP archive, a compound file, or neither (an export).
    fn add_file(&self, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        if bytes.starts_with(b"PK") {
            self.write("zip_entry", name, bytes)?;
            self.add_xml_parts(name, bytes)?;
            let package = read_ooxml(bytes).ok();
            if let Some(project) = package.and_then(|package| package.vba_project) {
                self.add_container(&format!("{name}.vbaProject.bin"), &project)?;
            }
        } else if bytes.starts_with(&CFB_SIGNATURE) {
            self.add_container(name, bytes)?;
        }
        Ok(())
    }

    /// The XML parts of a package, when it is a zip archive at all.
    fn add_xml_parts(&self, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let Ok(mut archive) = ZipArchive::new(Cursor::new(bytes)) else {
            return Ok(());
        };
        for (number, part) in (0_u8..).zip(XML_PARTS) {
            let Ok(entry) = archive.by_name(part) else {
                continue;
            };
            // `ooxml_xml` reads first the number of the part.
            let mut seed = vec![number];
            if entry.take(MAX_PART).read_to_end(&mut seed).is_ok() {
                self.write("ooxml_xml", &format!("{name}.part{number}"), &seed)?;
            }
        }
        Ok(())
    }

    /// A compound file, its `Workbook` stream and its VBA project.
    fn add_container(&self, name: &str, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        self.write("cfb_streams", name, bytes)?;
        let Ok(mut file) = CfbFile::open(bytes) else {
            return Ok(());
        };
        if let Ok(Some(workbook)) = file.read_stream("Workbook") {
            self.write("biff_workbook", name, &workbook)?;
        }
        for root in PROJECT_ROOTS {
            self.add_project(name, &mut file, root)?;
        }
        Ok(())
    }

    /// The `dir` and `PROJECT` streams of the VBA project under `root`, and
    /// the source of its modules.
    fn add_project(
        &self,
        name: &str,
        file: &mut CfbFile<'_>,
        root: &str,
    ) -> Result<(), Box<dyn Error>> {
        let Ok(Some(compressed)) = file.read_stream(&format!("{root}VBA/dir")) else {
            return Ok(());
        };
        self.write("dir_stream", &format!("{name}.dir.compressed"), &compressed)?;
        let seed = [&LIMIT_PREFIX, &compressed[..]].concat();
        self.write("ovba_decompress", &format!("{name}.dir"), &seed)?;
        let dir = decompress(&compressed, MAX_DIR_STREAM).ok();
        if let Some(dir) = &dir {
            self.write("dir_stream", &format!("{name}.dir"), dir)?;
        }
        let info = dir.and_then(|dir| parse_dir(&dir).ok());
        // The targets that take a code page read first whether there is one
        // (0 or 1), then the page.
        let code_page = info.as_ref().and_then(|info| info.code_page);
        let code_page_prefix =
            code_page.map_or(vec![0], |page| [&[1][..], &page.to_le_bytes()].concat());
        if let Ok(Some(project)) = file.read_stream(&format!("{root}PROJECT")) {
            let seed = [&code_page_prefix[..], &project].concat();
            self.write("project_stream", &format!("{name}.project"), &seed)?;
        }
        for (index, module) in info.iter().flat_map(|info| &info.modules).enumerate() {
            let Ok(Some(stream)) = file.read_stream(&format!("{root}VBA/{}", module.stream_name))
            else {
                continue;
            };
            // The source follows the performance cache, at the text offset.
            let offset = usize::try_from(module.text_offset).unwrap_or(usize::MAX);
            if let Some(source) = stream.get(offset..) {
                let seed = [&LIMIT_PREFIX, source].concat();
                self.write("ovba_decompress", &format!("{name}.module{index}"), &seed)?;
            }
            // `module_source` reads the offset (2 bytes) and the code page first.
            if let Ok(offset) = u16::try_from(module.text_offset) {
                let seed = [&offset.to_le_bytes()[..], &code_page_prefix, &stream].concat();
                self.write("module_source", &format!("{name}.module{index}"), &seed)?;
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let [fixtures, out] = args.as_slice() else {
        return Err("usage: seeds <fixtures folder> <output folder>".into());
    };
    let seeds = Seeds {
        out: PathBuf::from(out),
    };
    for folder in ["generated", "malformed"] {
        for entry in fs::read_dir(Path::new(fixtures).join(folder))? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            seeds.add_file(name, &fs::read(&path)?)?;
        }
    }
    Ok(())
}
