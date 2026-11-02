use crate::output::{ErrorCode, OwcError, Result};
use aes::Aes256;
#[cfg(test)]
use cbc::cipher::BlockEncryptMut;
use cbc::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use sha2::Sha512;

const PAGE_SIZE: usize = 4096;
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
type Aes256CbcDec = cbc::Decryptor<Aes256>;
#[cfg(test)]
type Aes256CbcEnc = cbc::Encryptor<Aes256>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherProfile {
    Sha1_64000,
    Sha512_256000,
}

impl CipherProfile {
    pub const ALL: [Self; 2] = [Self::Sha1_64000, Self::Sha512_256000];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sha1_64000 => "sha1-64000",
            Self::Sha512_256000 => "sha512-256000",
        }
    }

    // S3: These profile parameters follow the reconstruction contract and are selected by page-1 HMAC.
    pub fn reserve(self) -> usize {
        match self {
            Self::Sha1_64000 => 48,
            Self::Sha512_256000 => 80,
        }
    }

    fn iterations(self) -> u32 {
        match self {
            Self::Sha1_64000 => 64_000,
            Self::Sha512_256000 => 256_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecryptedDatabase {
    pub bytes: Vec<u8>,
    pub profile: CipherProfile,
}

pub fn decrypt_database(encrypted: &[u8], raw_key: &[u8]) -> Result<DecryptedDatabase> {
    if encrypted.starts_with(SQLITE_HEADER) {
        return Err(OwcError::new(
            ErrorCode::FormatMismatch,
            "Input already has a plaintext SQLite header.",
            "Provide an encrypted database snapshot.",
        ));
    }
    if raw_key.len() != 32
        || encrypted.len() < PAGE_SIZE
        || !encrypted.len().is_multiple_of(PAGE_SIZE)
    {
        return Err(OwcError::new(
            ErrorCode::FormatMismatch,
            "Input does not match the expected 4096-byte encrypted database layout.",
            "Check the snapshot file and database key.",
        ));
    }
    let salt: [u8; 16] = encrypted
        .get(..16)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| OwcError::data("Encrypted database salt is incomplete."))?;

    for profile in CipherProfile::ALL {
        let keys = derive_keys(raw_key, &salt, profile);
        if !page_hmac_matches(encrypted, 1, true, profile, &keys.1)? {
            continue;
        }
        let mut plain = Vec::with_capacity(encrypted.len());
        plain.extend_from_slice(SQLITE_HEADER);
        let pages = encrypted.len() / PAGE_SIZE;
        for page_no in 1..=pages {
            let page_start = if page_no == 1 {
                0
            } else {
                (page_no - 1) * PAGE_SIZE
            };
            let page_end = page_start + PAGE_SIZE;
            let page = if page_no == 1 {
                &encrypted[16..page_end]
            } else {
                &encrypted[page_start..page_end]
            };
            let off = if page_no == 1 { 16 } else { 0 };
            let body_len = PAGE_SIZE - profile.reserve() - off;
            let ciphertext = &page[..body_len];
            let iv = &page[body_len..body_len + 16];
            if !page_hmac_matches(encrypted, page_no, page_no == 1, profile, &keys.1)? {
                return Err(OwcError::data(format!(
                    "Encrypted database page {page_no} failed HMAC verification."
                )));
            }
            let mut body = ciphertext.to_vec();
            Aes256CbcDec::new_from_slices(&keys.0, iv)
                .map_err(|_| OwcError::data("Could not initialize the AES cipher."))?
                .decrypt_padded_mut::<NoPadding>(&mut body)
                .map_err(|_| OwcError::data("Could not decrypt a database page."))?;
            plain.extend_from_slice(&body);
            plain.resize(plain.len() + profile.reserve(), 0);
        }
        if plain.get(..16) != Some(SQLITE_HEADER.as_slice()) {
            return Err(OwcError::new(
                ErrorCode::FormatMismatch,
                "Decrypted page 1 does not contain a SQLite header.",
                "Check the database key and cipher profile.",
            ));
        }
        return Ok(DecryptedDatabase {
            bytes: plain,
            profile,
        });
    }
    Err(OwcError::new(
        ErrorCode::KeyNotFound,
        "The supplied key did not authenticate database page 1.",
        "Run `owc key scan --account <corp_id>` or provide the correct --key-file.",
    ))
}

fn derive_keys(raw_key: &[u8], salt: &[u8; 16], profile: CipherProfile) -> ([u8; 32], [u8; 32]) {
    let mut enc_key = [0_u8; 32];
    match profile {
        CipherProfile::Sha1_64000 => {
            pbkdf2_hmac::<Sha1>(raw_key, salt, profile.iterations(), &mut enc_key)
        }
        CipherProfile::Sha512_256000 => {
            pbkdf2_hmac::<Sha512>(raw_key, salt, profile.iterations(), &mut enc_key)
        }
    }
    let mut mac_salt = *salt;
    for byte in &mut mac_salt {
        *byte ^= 0x3a;
    }
    let mut mac_key = [0_u8; 32];
    match profile {
        CipherProfile::Sha1_64000 => pbkdf2_hmac::<Sha1>(&enc_key, &mac_salt, 2, &mut mac_key),
        CipherProfile::Sha512_256000 => pbkdf2_hmac::<Sha512>(&enc_key, &mac_salt, 2, &mut mac_key),
    }
    (enc_key, mac_key)
}

fn page_hmac_matches(
    encrypted: &[u8],
    page_no: usize,
    first: bool,
    profile: CipherProfile,
    mac_key: &[u8; 32],
) -> Result<bool> {
    let page_start = if first { 0 } else { (page_no - 1) * PAGE_SIZE };
    let page_end = page_start + PAGE_SIZE;
    let raw_page = encrypted
        .get(page_start..page_end)
        .ok_or_else(|| OwcError::data("Encrypted database page is truncated."))?;
    let page = if first { &raw_page[16..] } else { raw_page };
    let offset = if first { 16 } else { 0 };
    let body_len = PAGE_SIZE - profile.reserve() - offset;
    let iv_start = body_len;
    let hmac_start = iv_start + 16;
    let hmac_len = match profile {
        CipherProfile::Sha1_64000 => 20,
        CipherProfile::Sha512_256000 => 64,
    };
    let expected = page
        .get(hmac_start..hmac_start + hmac_len)
        .ok_or_else(|| OwcError::data("Encrypted database page reserve area is truncated."))?;
    let iv = &page[iv_start..iv_start + 16];
    let body = &page[..body_len];
    let page_number = u32::try_from(page_no)
        .map_err(|_| OwcError::data("Database contains too many pages."))?
        .to_le_bytes();
    match profile {
        CipherProfile::Sha1_64000 => {
            let mut mac = Hmac::<Sha1>::new_from_slice(mac_key)
                .map_err(|_| OwcError::data("Could not initialize page HMAC."))?;
            mac.update(body);
            mac.update(iv);
            mac.update(&page_number);
            let calculated = mac.finalize().into_bytes();
            Ok(constant_time_eq(expected, &calculated))
        }
        CipherProfile::Sha512_256000 => {
            let mut mac = Hmac::<Sha512>::new_from_slice(mac_key)
                .map_err(|_| OwcError::data("Could not initialize page HMAC."))?;
            mac.update(body);
            mac.update(iv);
            mac.update(&page_number);
            let calculated = mac.finalize().into_bytes();
            Ok(constant_time_eq(expected, &calculated))
        }
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

#[cfg(test)]
pub(crate) fn encrypt_db(plain: &[u8], raw_key: &[u8], profile: CipherProfile) -> Result<Vec<u8>> {
    if raw_key.len() != 32
        || plain.len() < PAGE_SIZE
        || !plain.len().is_multiple_of(PAGE_SIZE)
        || plain.get(..16) != Some(SQLITE_HEADER.as_slice())
        || plain.get(20).copied() != Some(profile.reserve() as u8)
    {
        return Err(OwcError::data(
            "Test encryption requires a page-aligned SQLite database with matching reserved bytes.",
        ));
    }
    let mut salt = [0_u8; 16];
    for (index, byte) in salt.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(19).wrapping_add(73);
    }
    let keys = derive_keys(raw_key, &salt, profile);
    let mut encrypted = Vec::with_capacity(plain.len());
    encrypted.extend_from_slice(&salt);
    for page_no in 1..=(plain.len() / PAGE_SIZE) {
        let page_start = (page_no - 1) * PAGE_SIZE;
        let off = if page_no == 1 { 16 } else { 0 };
        let body = &plain[page_start + off..page_start + PAGE_SIZE - profile.reserve()];
        let iv: Vec<u8> = (0..16)
            .map(|idx| (idx as u8).wrapping_add(page_no as u8).wrapping_mul(7))
            .collect();
        let mut ciphertext = body.to_vec();
        Aes256CbcEnc::new_from_slices(&keys.0, &iv)
            .map_err(|_| OwcError::data("Could not initialize the AES cipher."))?
            .encrypt_padded_mut::<NoPadding>(&mut ciphertext, body.len())
            .map_err(|_| OwcError::data("Could not encrypt a database page."))?;
        let page_number = u32::try_from(page_no)
            .map_err(|_| OwcError::data("Database contains too many pages."))?
            .to_le_bytes();
        let hmac = match profile {
            CipherProfile::Sha1_64000 => {
                let mut mac = Hmac::<Sha1>::new_from_slice(&keys.1)
                    .map_err(|_| OwcError::data("Could not initialize page HMAC."))?;
                mac.update(&ciphertext);
                mac.update(&iv);
                mac.update(&page_number);
                mac.finalize().into_bytes().to_vec()
            }
            CipherProfile::Sha512_256000 => {
                let mut mac = Hmac::<Sha512>::new_from_slice(&keys.1)
                    .map_err(|_| OwcError::data("Could not initialize page HMAC."))?;
                mac.update(&ciphertext);
                mac.update(&iv);
                mac.update(&page_number);
                mac.finalize().into_bytes().to_vec()
            }
        };
        encrypted.extend_from_slice(&ciphertext);
        encrypted.extend_from_slice(&iv);
        encrypted.extend_from_slice(&hmac);
        encrypted.resize(encrypted.len() + profile.reserve() - 16 - hmac.len(), 0);
    }
    Ok(encrypted)
}
