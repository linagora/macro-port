// SPDX-License-Identifier: Apache-2.0
//! The data encryption of MS-OVBA (section 2.4.3): a reversible obfuscation,
//! which the `PROJECT` stream applies to the protection state of a project.
//! It hides nothing from a reader who knows the algorithm, and it is no
//! password check: reading the data back needs no key but the one that the
//! structure holds.

/// The only version of the encryption.
const VERSION: u8 = 2;

/// The bytes that key the next one, as the specification names them.
struct State {
    /// The last byte read, decrypted (UnencryptedByte1).
    plain: u8,
    /// The last byte read, as it is in the structure (EncryptedByte1).
    cipher: u8,
    /// The byte before that, as it is in the structure (EncryptedByte2).
    previous_cipher: u8,
}

impl State {
    /// The byte that `cipher` encrypts, and the state for the next one.
    fn decrypt(&mut self, cipher: u8) -> u8 {
        let plain = cipher ^ self.previous_cipher.wrapping_add(self.plain);
        self.previous_cipher = self.cipher;
        self.cipher = cipher;
        self.plain = plain;
        plain
    }
}

/// Decrypts an Encrypted Data Structure: the seed, the version and the key
/// (1 byte each), 0 to 3 bytes that are ignored, the length of the data (4
/// bytes) and the data.
///
/// Returns the data, or `None` if the structure is not one: it is too short,
/// its version is not 2 or its length is not the number of bytes that follow
/// it.
pub(super) fn decrypt(structure: &[u8]) -> Option<Vec<u8>> {
    let (&[seed, version_enc, key_enc], rest) = structure.split_first_chunk::<3>()?;
    if (seed ^ version_enc) != VERSION {
        return None;
    }
    let mut state = State {
        plain: seed ^ key_enc,
        cipher: key_enc,
        previous_cipher: version_enc,
    };
    // Seed BAND 6, halved.
    let ignored = usize::from((seed & 6) / 2);
    let (ignored_enc, rest) = rest.split_at_checked(ignored)?;
    let (&length_enc, data_enc) = rest.split_first_chunk::<4>()?;
    for &byte in ignored_enc {
        state.decrypt(byte);
    }
    let length = u32::from_le_bytes(length_enc.map(|byte| state.decrypt(byte)));
    // The length is compared before anything is allocated for the data.
    if usize::try_from(length) != Ok(data_enc.len()) {
        return None;
    }
    Some(data_enc.iter().map(|&byte| state.decrypt(byte)).collect())
}

#[cfg(test)]
mod tests {
    use super::decrypt;

    /// The data of the structure that these hexadecimal digits write.
    fn data(digits: &str) -> Option<Vec<u8>> {
        let structure: Vec<u8> = digits
            .as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        decrypt(&structure)
    }

    #[test]
    fn decrypts_the_examples_of_the_specification() {
        // The GC of the example PROJECT stream, whose project is visible.
        assert_eq!(data("1517CAF1D6F9D7F9D706"), Some(vec![0xFF]));
        // Its DPB, which holds the other state that a GC may hold, and its
        // CMG, which holds four bytes.
        assert_eq!(data("0E0CD1ECDFF4E7F5E7F5E7"), Some(vec![0x00]));
        assert_eq!(data("0705D8E3D8EDDBF1DBF1DBF1DBF1"), Some(vec![0; 4]));
    }

    #[test]
    fn decrypts_the_gc_that_excel_wrote() {
        // The project of 01-01-hide-unhide-columns-find.xls, which has no
        // password: it is visible.
        assert_eq!(data("D5D7DC4524EB25EB2514"), Some(vec![0xFF]));
        // The projects of 01-01-hide-unhide-columns-find.xlsm,
        // 08-01-udf-regex.xlsm and 12-02-mailing-outlook-html-signature.xlsm,
        // which were locked with a password: they are not visible. The seed
        // of the first makes three bytes of IgnoredEnc, that of the others
        // none.
        assert_eq!(data("8F8D23BC2DC4B3C5B3C5B3"), Some(vec![0x00]));
        assert_eq!(data("20228CCF8DCF8DCF"), Some(vec![0x00]));
        assert_eq!(data("696BC516C616C616"), Some(vec![0x00]));
    }
}
