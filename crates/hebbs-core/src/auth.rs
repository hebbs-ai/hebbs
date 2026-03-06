use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use sha2::{Digest, Sha256};

use hebbs_storage::{ColumnFamilyName, StorageBackend};

/// API key prefix for visual identification in logs and config.
pub const KEY_PREFIX: &str = "hb_";

/// Meta CF key prefix for stored API key records.
const AUTH_KEY_PREFIX: &str = "auth:key:";

/// Permission bitmask for read operations (recall, prime, subscribe, insights, get).
pub const PERM_READ: u8 = 0x01;
/// Permission bitmask for write operations (remember, revise, forget).
pub const PERM_WRITE: u8 = 0x02;
/// Permission bitmask for admin operations (reflect, reflect_policy, key management).
pub const PERM_ADMIN: u8 = 0x04;

/// Stored API key record (never contains the plaintext key).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeyRecord {
    pub key_hash: [u8; 32],
    pub tenant_id: String,
    pub name: String,
    pub permissions: u8,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

impl KeyRecord {
    pub fn has_permission(&self, perm: u8) -> bool {
        self.permissions & perm == perm
    }

    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            now_us() > exp
        } else {
            false
        }
    }

    pub fn permissions_string(&self) -> String {
        let mut perms = Vec::new();
        if self.permissions & PERM_READ != 0 {
            perms.push("read");
        }
        if self.permissions & PERM_WRITE != 0 {
            perms.push("write");
        }
        if self.permissions & PERM_ADMIN != 0 {
            perms.push("admin");
        }
        perms.join(",")
    }
}

/// In-memory cache of API key records, keyed by SHA-256 hash.
/// Wrapped in RwLock: reads (hot path) take shared lock,
/// writes (admin key create/revoke) take exclusive lock.
pub struct KeyCache {
    keys: RwLock<HashMap<[u8; 32], KeyRecord>>,
}

impl KeyCache {
    pub fn new() -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
        }
    }

    /// Load all key records from the meta CF into the cache.
    pub fn load_from_storage(&self, storage: &dyn StorageBackend) -> Result<usize, String> {
        let entries = storage
            .prefix_iterator(ColumnFamilyName::Meta, AUTH_KEY_PREFIX.as_bytes())
            .map_err(|e| format!("failed to load auth keys: {}", e))?;

        let mut keys = self.keys.write();
        keys.clear();

        for (_key, value) in &entries {
            match serde_json::from_slice::<KeyRecord>(value) {
                Ok(record) => {
                    keys.insert(record.key_hash, record);
                }
                Err(e) => {
                    eprintln!("failed to deserialize auth key record: {}", e);
                }
            }
        }

        Ok(keys.len())
    }

    /// Validate a raw API key string. Returns the associated KeyRecord if valid.
    pub fn validate(&self, raw_key: &str) -> Result<KeyRecord, AuthError> {
        if !raw_key.starts_with(KEY_PREFIX) {
            return Err(AuthError::MalformedToken);
        }

        let hash = hash_key(raw_key);
        let keys = self.keys.read();

        match keys.get(&hash) {
            None => Err(AuthError::UnknownKey),
            Some(record) => {
                if record.is_expired() {
                    return Err(AuthError::ExpiredKey);
                }
                Ok(record.clone())
            }
        }
    }

    /// Insert a new key record into the cache and persist to storage.
    pub fn insert(&self, storage: &dyn StorageBackend, record: KeyRecord) -> Result<(), String> {
        let storage_key = format!("{}{}", AUTH_KEY_PREFIX, hex::encode(record.key_hash));
        let value =
            serde_json::to_vec(&record).map_err(|e| format!("failed to serialize key: {}", e))?;

        storage
            .put(ColumnFamilyName::Meta, storage_key.as_bytes(), &value)
            .map_err(|e| format!("failed to persist key: {}", e))?;

        self.keys.write().insert(record.key_hash, record);
        Ok(())
    }

    /// Revoke (delete) a key by its hash.
    pub fn revoke(
        &self,
        storage: &dyn StorageBackend,
        key_hash: &[u8; 32],
    ) -> Result<bool, String> {
        let storage_key = format!("{}{}", AUTH_KEY_PREFIX, hex::encode(key_hash));
        storage
            .delete(ColumnFamilyName::Meta, storage_key.as_bytes())
            .map_err(|e| format!("failed to delete key: {}", e))?;

        let removed = self.keys.write().remove(key_hash).is_some();
        Ok(removed)
    }

    /// List all key records for a given tenant.
    pub fn list_for_tenant(&self, tenant_id: &str) -> Vec<KeyRecord> {
        self.keys
            .read()
            .values()
            .filter(|r| r.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    pub fn key_count(&self) -> usize {
        self.keys.read().len()
    }
}

impl Default for KeyCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a new random API key and its record.
/// Returns (raw_key_string, KeyRecord).
pub fn generate_key(
    tenant_id: &str,
    name: &str,
    permissions: u8,
    expires_at: Option<u64>,
) -> (String, KeyRecord) {
    let mut random_bytes = [0u8; 32];
    getrandom(&mut random_bytes);

    let raw_key = format!("{}{}", KEY_PREFIX, base64url_encode(&random_bytes));

    let hash = hash_key(&raw_key);

    let record = KeyRecord {
        key_hash: hash,
        tenant_id: tenant_id.to_string(),
        name: name.to_string(),
        permissions,
        created_at: now_us(),
        expires_at,
    };

    (raw_key, record)
}

/// Hash a raw API key with SHA-256.
pub fn hash_key(raw_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Errors specific to authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    MissingHeader,
    MalformedToken,
    UnknownKey,
    ExpiredKey,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHeader => write!(f, "missing authorization header"),
            Self::MalformedToken => write!(f, "malformed API key (expected 'Bearer hb_...')"),
            Self::UnknownKey => write!(f, "unknown API key"),
            Self::ExpiredKey => write!(f, "API key has expired"),
        }
    }
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

fn base64url_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(data.len() * 4 / 3 + 4);
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut i = 0;
    while i + 2 < data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        let _ = write!(
            result,
            "{}{}{}{}",
            ALPHABET[((n >> 18) & 0x3F) as usize] as char,
            ALPHABET[((n >> 12) & 0x3F) as usize] as char,
            ALPHABET[((n >> 6) & 0x3F) as usize] as char,
            ALPHABET[(n & 0x3F) as usize] as char,
        );
        i += 3;
    }

    let remaining = data.len() - i;
    if remaining == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        let _ = write!(
            result,
            "{}{}{}",
            ALPHABET[((n >> 18) & 0x3F) as usize] as char,
            ALPHABET[((n >> 12) & 0x3F) as usize] as char,
            ALPHABET[((n >> 6) & 0x3F) as usize] as char,
        );
    } else if remaining == 1 {
        let n = (data[i] as u32) << 16;
        let _ = write!(
            result,
            "{}{}",
            ALPHABET[((n >> 18) & 0x3F) as usize] as char,
            ALPHABET[((n >> 12) & 0x3F) as usize] as char,
        );
    }

    result
}

fn getrandom(buf: &mut [u8]) {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    for chunk in buf.chunks_mut(8) {
        let s = RandomState::new();
        let val = s.build_hasher().finish().to_le_bytes();
        let len = chunk.len().min(8);
        chunk[..len].copy_from_slice(&val[..len]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hebbs_storage::InMemoryBackend;
    use std::sync::Arc;

    fn test_storage() -> Arc<InMemoryBackend> {
        Arc::new(InMemoryBackend::new())
    }

    #[test]
    fn generate_and_validate_key() {
        let storage = test_storage();
        let cache = KeyCache::new();

        let (raw_key, record) = generate_key("tenant_a", "test key", PERM_READ | PERM_WRITE, None);
        cache.insert(storage.as_ref(), record).unwrap();

        let validated = cache.validate(&raw_key).unwrap();
        assert_eq!(validated.tenant_id, "tenant_a");
        assert!(validated.has_permission(PERM_READ));
        assert!(validated.has_permission(PERM_WRITE));
        assert!(!validated.has_permission(PERM_ADMIN));
    }

    #[test]
    fn unknown_key_rejected() {
        let cache = KeyCache::new();
        assert_eq!(
            cache.validate("hb_unknown_key_here"),
            Err(AuthError::UnknownKey)
        );
    }

    #[test]
    fn malformed_key_rejected() {
        let cache = KeyCache::new();
        assert_eq!(
            cache.validate("not_a_valid_key"),
            Err(AuthError::MalformedToken)
        );
    }

    #[test]
    fn expired_key_rejected() {
        let storage = test_storage();
        let cache = KeyCache::new();

        let (raw_key, record) = generate_key("tenant_a", "expired", PERM_READ, Some(1));
        cache.insert(storage.as_ref(), record).unwrap();

        assert_eq!(cache.validate(&raw_key), Err(AuthError::ExpiredKey));
    }

    #[test]
    fn revoke_key() {
        let storage = test_storage();
        let cache = KeyCache::new();

        let (raw_key, record) = generate_key("tenant_a", "to_revoke", PERM_READ, None);
        let hash = record.key_hash;
        cache.insert(storage.as_ref(), record).unwrap();

        assert!(cache.validate(&raw_key).is_ok());

        cache.revoke(storage.as_ref(), &hash).unwrap();

        assert_eq!(cache.validate(&raw_key), Err(AuthError::UnknownKey));
    }

    #[test]
    fn load_from_storage_persists() {
        let storage = test_storage();
        let cache1 = KeyCache::new();

        let (raw_key, record) = generate_key("tenant_a", "persistent", PERM_READ, None);
        cache1.insert(storage.as_ref(), record).unwrap();

        let cache2 = KeyCache::new();
        let loaded = cache2.load_from_storage(storage.as_ref()).unwrap();
        assert_eq!(loaded, 1);
        assert!(cache2.validate(&raw_key).is_ok());
    }

    #[test]
    fn list_for_tenant() {
        let storage = test_storage();
        let cache = KeyCache::new();

        let (_, r1) = generate_key("tenant_a", "key1", PERM_READ, None);
        let (_, r2) = generate_key("tenant_a", "key2", PERM_WRITE, None);
        let (_, r3) = generate_key("tenant_b", "key3", PERM_ADMIN, None);
        cache.insert(storage.as_ref(), r1).unwrap();
        cache.insert(storage.as_ref(), r2).unwrap();
        cache.insert(storage.as_ref(), r3).unwrap();

        assert_eq!(cache.list_for_tenant("tenant_a").len(), 2);
        assert_eq!(cache.list_for_tenant("tenant_b").len(), 1);
        assert_eq!(cache.list_for_tenant("tenant_c").len(), 0);
    }

    #[test]
    fn permissions_string_formatting() {
        let (_, record) = generate_key("t", "n", PERM_READ | PERM_WRITE | PERM_ADMIN, None);
        assert_eq!(record.permissions_string(), "read,write,admin");

        let (_, record) = generate_key("t", "n", PERM_READ, None);
        assert_eq!(record.permissions_string(), "read");
    }

    #[test]
    fn key_has_prefix() {
        let (raw_key, _) = generate_key("t", "n", PERM_READ, None);
        assert!(raw_key.starts_with(KEY_PREFIX));
    }
}
