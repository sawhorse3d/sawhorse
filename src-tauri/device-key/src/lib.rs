//! The Sawhorse desktop device key (ADR-0004).
//!
//! Render credits have no accounts: the render service knows a device by its Ed25519 public
//! key, and `install_id = base32(sha256(public key))[:20]` (lowercase RFC 4648, no padding).
//! On the desktop the 32-byte seed lives in the OS secure store (Windows Credential Manager,
//! macOS Keychain, Secret Service on Linux) and never crosses into the webview: the front end
//! asks for the public key and for signatures through two Tauri commands.
//!
//! The same derivation and signatures are implemented in the app (`src/credits/device.ts`)
//! and the render service (`render-service/src/crypto.ts`); all three test the same vector.

use std::fmt;
use std::sync::Mutex;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Keychain service name (the app identifier) and account for the seed.
pub const SERVICE: &str = "app.sawhorse.desktop";
pub const ACCOUNT: &str = "render-device-key-v1";

#[derive(Debug)]
pub enum Error {
    /// The secure store refused or failed (locked keychain, no Secret Service, ...).
    Store(String),
    /// A stored seed that is not 32 bytes of base64url. It is left in place, not overwritten:
    /// replacing it would orphan the credits bound to the old key.
    Corrupt(String),
    /// The OS random source failed.
    Random(String),
    /// Bad input from the front end.
    Input(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "the system keychain refused the device key: {e}"),
            Error::Corrupt(e) => write!(
                f,
                "the stored device key is unreadable ({e}); move your credits with a claim code"
            ),
            Error::Random(e) => write!(f, "no randomness for a new device key: {e}"),
            Error::Input(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Where the seed is kept.
pub trait SecretStore {
    fn load(&self) -> Result<Option<String>, Error>;
    fn save(&self, secret: &str) -> Result<(), Error>;
}

/// The OS secure store through the `keyring` crate.
pub struct KeyringStore {
    entry: keyring::Entry,
}

impl KeyringStore {
    pub fn new() -> Result<Self, Error> {
        Self::with_names(SERVICE, ACCOUNT)
    }

    pub fn with_names(service: &str, account: &str) -> Result<Self, Error> {
        keyring::Entry::new(service, account)
            .map(|entry| Self { entry })
            .map_err(|e| Error::Store(e.to_string()))
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<String>, Error> {
        match self.entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Store(e.to_string())),
        }
    }

    fn save(&self, secret: &str) -> Result<(), Error> {
        self.entry
            .set_password(secret)
            .map_err(|e| Error::Store(e.to_string()))
    }
}

/// In-memory store (tests, and a fallback a caller may choose when no keychain exists).
#[derive(Default)]
pub struct MemoryStore(Mutex<Option<String>>);

impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, Error> {
        Ok(self
            .0
            .lock()
            .map_err(|_| Error::Store("poisoned".into()))?
            .clone())
    }

    fn save(&self, secret: &str) -> Result<(), Error> {
        *self.0.lock().map_err(|_| Error::Store("poisoned".into()))? = Some(secret.to_owned());
        Ok(())
    }
}

/// Lowercase RFC 4648 base32, no padding.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            out.push(ALPHABET[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// `base32(sha256(public_key))[:20]`.
pub fn install_id(public_key: &[u8; 32]) -> String {
    let mut id = base32(&Sha256::digest(public_key));
    id.truncate(20);
    id
}

pub struct DeviceKey {
    signing: SigningKey,
}

impl DeviceKey {
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(seed),
        }
    }

    /// The stored key, or a new one saved to the store on first use.
    pub fn load_or_create(store: &dyn SecretStore) -> Result<Self, Error> {
        if let Some(stored) = store.load()? {
            let bytes = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(stored.trim())
                    .map_err(|e| Error::Corrupt(e.to_string()))?,
            );
            let seed: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| Error::Corrupt(format!("{} bytes, expected 32", bytes.len())))?;
            let seed = Zeroizing::new(seed);
            return Ok(Self::from_seed(&seed));
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        getrandom::fill(seed.as_mut()).map_err(|e| Error::Random(e.to_string()))?;
        let encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(seed.as_ref()));
        store.save(&encoded)?;
        // Read it back: a keychain that silently drops writes must not hand out a key that
        // would be gone on the next launch (and with it, the credits bought for it).
        match store.load()? {
            Some(s) if s.trim() == encoded.as_str() => Ok(Self::from_seed(&seed)),
            _ => Err(Error::Store(
                "the keychain did not keep the new device key".into(),
            )),
        }
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn public_key_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.public_key())
    }

    pub fn install_id(&self) -> String {
        install_id(&self.public_key())
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }

    /// Sign base64url message bytes from the front end; base64url signature back.
    pub fn sign_b64(&self, message_b64: &str) -> Result<String, Error> {
        let message = URL_SAFE_NO_PAD
            .decode(message_b64)
            .map_err(|e| Error::Input(format!("message is not base64url: {e}")))?;
        if message.len() > 4096 {
            return Err(Error::Input(
                "message too long to be a request signature".into(),
            ));
        }
        Ok(URL_SAFE_NO_PAD.encode(self.sign(&message)))
    }
}

/// URLs the desktop app may open in the system browser: Stripe Checkout, and the render
/// service's own simulated checkout on a loopback address during development.
pub fn external_url_allowed(url: &str) -> bool {
    if let Some(rest) = url.strip_prefix("https://checkout.stripe.com/") {
        return !rest.contains(char::is_whitespace);
    }
    for loopback in ["http://127.0.0.1:", "http://localhost:"] {
        if let Some(rest) = url.strip_prefix(loopback) {
            let (port, path) = rest.split_once('/').unwrap_or((rest, ""));
            return !port.is_empty()
                && port.chars().all(|c| c.is_ascii_digit())
                && path.starts_with("dev/checkout/");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    // The vector shared with src/credits/vector.test.ts and render-service/test/vector.test.ts.
    const SEED: [u8; 32] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31,
    ];
    const PUBLIC_KEY: &str = "A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg";
    const INSTALL_ID: &str = "kzdvvj2umnduyauf35o3";
    const CANONICAL: &str = "sawhorse-v1\nPOST\n/v1/checkout\n96d8e19549877ae78a69219aea38f01653b12016ff1030bb45d6b9b780d6fcc2\n1760140800\nAAAAAAAAAAAAAAAAAAAAAA";
    const SIGNATURE: &str =
        "24sJ6AYzwwh1SduKox6hs3dTTm83RQTODQVsPbsl3bb4Bta0tkXu2ioHAprJmec2-2A_YCBgj9wFwOoN8FOtDg";

    #[test]
    fn shared_vector() {
        let k = DeviceKey::from_seed(&SEED);
        assert_eq!(k.public_key_b64(), PUBLIC_KEY);
        assert_eq!(k.install_id(), INSTALL_ID);
        assert_eq!(
            URL_SAFE_NO_PAD.encode(k.sign(CANONICAL.as_bytes())),
            SIGNATURE
        );
        assert_eq!(
            k.sign_b64(&URL_SAFE_NO_PAD.encode(CANONICAL)).unwrap(),
            SIGNATURE
        );
    }

    #[test]
    fn base32_matches_rfc4648() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "my");
        assert_eq!(base32(b"foobar"), "mzxw6ytboi");
        assert_eq!(base32(&[255; 40]), "7".repeat(64));
    }

    #[test]
    fn creates_once_then_loads_the_same_key() {
        let store = MemoryStore::default();
        let a = DeviceKey::load_or_create(&store).unwrap();
        let b = DeviceKey::load_or_create(&store).unwrap();
        assert_eq!(a.public_key(), b.public_key());
        assert_eq!(a.install_id().len(), 20);
        let other = DeviceKey::load_or_create(&MemoryStore::default()).unwrap();
        assert_ne!(a.public_key(), other.public_key());
        let sig = Signature::from_bytes(&a.sign(b"hello"));
        assert!(VerifyingKey::from_bytes(&b.public_key())
            .unwrap()
            .verify(b"hello", &sig)
            .is_ok());
    }

    #[test]
    fn a_corrupt_seed_is_reported_not_replaced() {
        let store = MemoryStore::default();
        store.save("not base64url !!").unwrap();
        assert!(matches!(
            DeviceKey::load_or_create(&store),
            Err(Error::Corrupt(_))
        ));
        store.save(&URL_SAFE_NO_PAD.encode([7u8; 16])).unwrap();
        assert!(matches!(
            DeviceKey::load_or_create(&store),
            Err(Error::Corrupt(_))
        ));
        assert_eq!(
            store.load().unwrap().unwrap(),
            URL_SAFE_NO_PAD.encode([7u8; 16])
        );
    }

    struct ForgetfulStore;
    impl SecretStore for ForgetfulStore {
        fn load(&self) -> Result<Option<String>, Error> {
            Ok(None)
        }
        fn save(&self, _: &str) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn a_keychain_that_drops_writes_is_an_error() {
        assert!(matches!(
            DeviceKey::load_or_create(&ForgetfulStore),
            Err(Error::Store(_))
        ));
    }

    #[test]
    fn rejects_bad_sign_input() {
        let k = DeviceKey::from_seed(&SEED);
        assert!(k.sign_b64("***").is_err());
        assert!(k
            .sign_b64(&URL_SAFE_NO_PAD.encode(vec![0u8; 5000]))
            .is_err());
    }

    #[test]
    fn only_checkout_urls_open_externally() {
        assert!(external_url_allowed(
            "https://checkout.stripe.com/c/pay/cs_test_a1#fid"
        ));
        assert!(external_url_allowed(
            "http://127.0.0.1:8787/dev/checkout/cs_dev_abc"
        ));
        assert!(external_url_allowed(
            "http://localhost:8787/dev/checkout/cs_dev_abc"
        ));
        assert!(!external_url_allowed(
            "https://checkout.stripe.com.evil.example/x"
        ));
        assert!(!external_url_allowed("http://checkout.stripe.com/x"));
        assert!(!external_url_allowed("http://127.0.0.1:8787/v1/balance"));
        assert!(!external_url_allowed("http://127.0.0.1:87x/dev/checkout/a"));
        assert!(!external_url_allowed(
            "file:///C:/Windows/System32/calc.exe"
        ));
        assert!(!external_url_allowed("javascript:alert(1)"));
    }

    /// The real OS keychain. Run by hand: `cargo test -p sawhorse-device-key -- --ignored`.
    #[test]
    #[ignore]
    fn os_keychain_round_trip() {
        let store =
            KeyringStore::with_names("app.sawhorse.desktop.test", "device-key-test").unwrap();
        let a = DeviceKey::load_or_create(&store).unwrap();
        let b = DeviceKey::load_or_create(&store).unwrap();
        assert_eq!(a.public_key(), b.public_key());
    }
}
