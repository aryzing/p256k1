use base58::FromBase58;
use std::array::TryFromSliceError;

use crate::_rename::{
    secp256k1_ec_pubkey_create, secp256k1_ec_pubkey_parse, secp256k1_ec_pubkey_serialize,
    secp256k1_ecdsa_sign, secp256k1_ecdsa_signature_parse_compact,
    secp256k1_ecdsa_signature_serialize_compact, secp256k1_ecdsa_verify,
};
use crate::bindings::{secp256k1_ecdsa_signature, secp256k1_pubkey, SECP256K1_EC_COMPRESSED};
use crate::context::Context;
use crate::scalar::Scalar;

/// Errors when converting scalars
#[derive(Debug, Clone)]
pub enum ConversionError {
    /// Error converting a base58 string to bytes
    Base58(String),
}

#[derive(Debug, Clone)]
/// Errors in ECDSA signature operations
pub enum Error {
    /// Error occurred due to invalid secret key
    InvalidSecretKey,
    /// Error occurred due to invalid public key
    InvalidPublicKey,
    /// Error occurred during a try from operation
    TryFrom(String),
    /// Error converting a scalar
    Conversion(ConversionError),
}

impl From<TryFromSliceError> for Error {
    fn from(e: TryFromSliceError) -> Self {
        Error::TryFrom(e.to_string())
    }
}

/**
PublicKey is a wrapper around libsecp256k1's secp256k1_pubkey struct.
*/
#[derive(Clone, Copy)]
pub struct PublicKey {
    /// The wrapped secp256k1_pubkey public key
    key: secp256k1_pubkey,
}

impl PublicKey {
    /// Construct a public key from a given secret key
    pub fn new(sec_key: &Scalar) -> Result<Self, Error> {
        let mut pub_key = Self {
            key: secp256k1_pubkey { data: [0; 64] },
        };
        let ctx = Context::default();
        if unsafe {
            secp256k1_ec_pubkey_create(ctx.context, &mut pub_key.key, sec_key.to_bytes().as_ptr())
        } == 0
        {
            return Err(Error::InvalidSecretKey);
        }
        Ok(pub_key)
    }

    /// Serialize the key to a compressed byte array
    pub fn to_bytes(&self) -> [u8; 33] {
        let ctx = Context::default();
        let mut bytes = [0u8; 33];
        let mut len = bytes.len();

        unsafe {
            secp256k1_ec_pubkey_serialize(
                ctx.context,
                bytes.as_mut_ptr(),
                &mut len,
                &self.key,
                SECP256K1_EC_COMPRESSED,
            );
        }

        bytes
    }
}

impl TryFrom<&str> for PublicKey {
    type Error = Error;
    /// Create a pubkey from the passed byte slice
    fn try_from(s: &str) -> Result<Self, self::Error> {
        match s.from_base58() {
            Ok(bytes) => PublicKey::try_from(&bytes[..]),
            Err(e) => Err(Error::Conversion(ConversionError::Base58(format!(
                "{:?}",
                e
            )))),
        }
    }
}

impl TryFrom<&[u8]> for PublicKey {
    type Error = Error;
    /// Create a pubkey from the passed byte slice
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let mut pubkey = Self {
            key: secp256k1_pubkey { data: [0; 64] },
        };
        let ctx = Context::default();
        unsafe {
            match secp256k1_ec_pubkey_parse(
                ctx.context,
                &mut pubkey.key,
                input.as_ptr(),
                input.len(),
            ) {
                1 => Ok(pubkey),
                _ => Err(Error::InvalidPublicKey),
            }
        }
    }
}

/**
Signature is a wrapper around libsecp256k1's secp256k1_ecdsa_signature struct.
*/
pub struct Signature {
    /// The wrapped libsecp256k1 signature
    pub signature: secp256k1_ecdsa_signature,
    /// The context associated with the signature
    pub context: Context,
}

impl Signature {
    /// Construct an ECDSA signature
    pub fn new(hash: &[u8], sec_key: &Scalar) -> Result<Self, Error> {
        let mut sig = Self {
            signature: secp256k1_ecdsa_signature { data: [0; 64] },
            context: Context::default(),
        };
        if unsafe {
            secp256k1_ecdsa_sign(
                sig.context.context,
                &mut sig.signature,
                hash.as_ptr(),
                sec_key.to_bytes().as_ptr(),
                None,
                std::ptr::null_mut::<::std::os::raw::c_void>(),
            )
        } == 0
        {
            return Err(Error::InvalidSecretKey);
        }
        Ok(sig)
    }

    /// Verify an ECDSA signature
    pub fn verify(&self, hash: &[u8], pub_key: &PublicKey) -> bool {
        1 == unsafe {
            secp256k1_ecdsa_verify(
                self.context.context,
                &self.signature,
                hash.as_ptr(),
                &pub_key.key,
            )
        }
    }

    /// Returns the signature's deserialized underlying data
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        //Deserialize the signature's data
        unsafe {
            secp256k1_ecdsa_signature_serialize_compact(
                self.context.context,
                bytes.as_mut_ptr(),
                &self.signature,
            );
        }
        bytes
    }
}

impl TryFrom<&[u8]> for Signature {
    type Error = Error;
    /// Create an ECDSA signature given a slice of signed data.
    /// Note it also serializes the data in compact (64 byte) format
    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        let data: [u8; 64] = input[0..].try_into()?;
        Signature::try_from(data)
    }
}

impl TryFrom<[u8; 64]> for Signature {
    type Error = Error;
    /// Create an ECDSA signature given an array of signed data.
    /// Note it also serializes the data in compact (64 byte) format
    fn try_from(input: [u8; 64]) -> Result<Self, Self::Error> {
        let mut sig = Self {
            signature: secp256k1_ecdsa_signature { data: [0u8; 64] },
            context: Context::default(),
        };
        //Attempt to serialize the data into the signature
        let parsed = unsafe {
            secp256k1_ecdsa_signature_parse_compact(
                sig.context.context,
                &mut sig.signature,
                input.as_ptr(),
            )
        };
        if parsed == 0 {
            return Err(Error::TryFrom(
                "Failed to serialize input data into compact (64 byte) form.".to_string(),
            ));
        }
        Ok(sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::{OsRng, RngCore};
    use sha2::{Digest, Sha256};

    #[test]
    fn signature_generation() {
        // Generate a secret and public key
        let mut rnd = OsRng::default();
        let sec_key = Scalar::random(&mut rnd);
        let pub_key = PublicKey::new(&sec_key).unwrap();

        // Instead of signing a message directly, must sign a 32-byte hash of it.
        let msg = b"Hello, world!";
        let mut hasher = Sha256::new();
        hasher.update(msg);
        let msg_hash = hasher.finalize();
        // Generate a ECDSA signature
        let sig = Signature::new(&msg_hash, &sec_key).unwrap();

        // Verify the generated signature is valid using the msg_hash and corresponding public key
        assert!(sig.verify(&msg_hash, &pub_key));
    }

    #[test]
    fn signature_from() {
        // Create random data bytes to serialize
        let mut rng = OsRng::default();
        let mut bytes = [0u8; 64];
        rng.fill_bytes(&mut bytes);

        let sig_from_struct = Signature {
            signature: secp256k1_ecdsa_signature { data: bytes },
            context: Context::default(),
        };
        let sig_from_slice = Signature::try_from(bytes.as_slice()).unwrap();
        let sig_from_array = Signature::try_from(bytes).unwrap();

        assert_ne!(sig_from_struct.to_bytes(), sig_from_slice.to_bytes());
        assert_ne!(sig_from_struct.to_bytes(), sig_from_array.to_bytes());
        assert_eq!(sig_from_array.to_bytes(), sig_from_slice.to_bytes());

        let mut too_small = [0u8; 63];
        rng.fill_bytes(&mut too_small);
        assert!(Signature::try_from(too_small.as_slice()).is_err());

        let mut too_big = [0u8; 65];
        rng.fill_bytes(&mut too_big);
        assert!(Signature::try_from(too_big.as_slice()).is_err());
    }

    #[test]
    fn signature_serde() {
        // Generate random data bytes
        let mut rng = OsRng::default();
        let mut bytes = [0u8; 64];
        rng.fill_bytes(&mut bytes);

        //Serialize with try_from and deserialize with to_bytes
        let sig = Signature::try_from(bytes).unwrap();
        assert_ne!(sig.signature.data, bytes);
        assert_eq!(sig.to_bytes(), bytes);
    }

    #[test]
    fn pubkey_serde() {
        // Generate a secret and public key
        let mut rnd = OsRng::default();
        let sec_key = Scalar::random(&mut rnd);
        let pub_key = PublicKey::new(&sec_key).unwrap();

        //Serialize with try_from and deseriailze with to_bytes
        let pub_key_2 = PublicKey::try_from(pub_key.to_bytes().as_slice()).unwrap();
        assert_eq!(pub_key_2.to_bytes(), pub_key.to_bytes());
        assert_eq!(pub_key_2.key.data, pub_key.key.data);
    }
}
