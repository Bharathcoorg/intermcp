//! ADR 001: Signed Execution Receipts & RFC 8785 JSON Canonicalization Scheme (JCS).
//! Provides machine-verifiable cryptographic provenance for agent tool invocations.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use subtle::ConstantTimeEq;

use crate::error::FastMcpError;

/// RFC 8785 JSON Canonicalization Scheme (JCS) serializer.
/// Produces deterministic, byte-for-byte canonical JSON representations.
/// Note: Pinned to `serde_jcs = "0.1"` which strictly implements RFC 8785 shortest-round-trip
/// IEEE 754 float formatting and UTF-16 code unit key sorting for `serde_json::Value`.
pub fn canonicalize_json(value: &Value) -> Result<Vec<u8>, FastMcpError> {
    serde_jcs::to_vec(value).map_err(FastMcpError::Serialization)
}

/// Compute SHA-256 digest over RFC 8785 canonical JSON bytes.
pub fn hash_canonical_json(value: &Value) -> Result<String, FastMcpError> {
    let canonical = canonicalize_json(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    Ok(format!("{:x}", hasher.finalize()))
}

/// RFC 2104 compliant HMAC-SHA256 computation.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> String {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let mut hasher = Sha256::new();
        hasher.update(key);
        let digest = hasher.finalize();
        k[..32].copy_from_slice(&digest);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner_hasher = Sha256::new();
    inner_hasher.update(ipad);
    inner_hasher.update(message);
    let inner_hash = inner_hasher.finalize();

    let mut outer_hasher = Sha256::new();
    outer_hasher.update(opad);
    outer_hasher.update(inner_hash);
    let outer_hash = outer_hasher.finalize();

    format!("{:x}", outer_hash)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Success,
    ToolError,
    PolicyVetoed,
    TimedOut,
}

/// Immutable record of a tool execution with cryptographic linkage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionReceipt {
    pub version: u32,
    pub sequence: u64,
    pub timestamp_utc: String,
    pub prev_receipt_hash: String,
    pub session_id: String,
    pub tool_name: String,
    pub tool_schema_hash: String,
    pub canonical_input_hash: String,
    pub canonical_output_hash: String,
    pub execution_duration_us: u64,
    pub exit_status: ReceiptStatus,
}

/// A receipt bundled with its SHA-256 hash and HMAC signature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedReceiptRecord {
    pub receipt: ExecutionReceipt,
    pub receipt_hash: String,
    pub signature_hex: String,
    pub signer_id: String,
}

impl ExecutionReceipt {
    /// Compute the SHA-256 hash of this receipt under RFC 8785 canonicalization.
    pub fn compute_hash(&self) -> Result<String, FastMcpError> {
        let val = serde_json::to_value(self).map_err(FastMcpError::Serialization)?;
        hash_canonical_json(&val)
    }

    /// Sign this receipt with a secret key and produce a `SignedReceiptRecord`.
    pub fn sign(
        &self,
        secret_key: &[u8],
        signer_id: &str,
    ) -> Result<SignedReceiptRecord, FastMcpError> {
        let receipt_hash = self.compute_hash()?;
        let signature_hex = hmac_sha256(secret_key, receipt_hash.as_bytes());

        Ok(SignedReceiptRecord {
            receipt: self.clone(),
            receipt_hash,
            signature_hex,
            signer_id: signer_id.to_string(),
        })
    }
}

impl SignedReceiptRecord {
    /// Verify that this record's `receipt_hash` and `signature_hex` are valid.
    pub fn verify(&self, secret_key: Option<&[u8]>) -> Result<bool, FastMcpError> {
        let computed_hash = self.receipt.compute_hash()?;
        if computed_hash != self.receipt_hash {
            return Ok(false);
        }

        if let Some(key) = secret_key {
            let expected_sig = hmac_sha256(key, self.receipt_hash.as_bytes());
            let matches: bool = self
                .signature_hex
                .as_bytes()
                .ct_eq(expected_sig.as_bytes())
                .into();
            if !matches {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

/// Stateful engine that signs tool executions and appends them to disk.
#[derive(Clone)]
pub struct ReceiptBook {
    inner: Arc<Mutex<ReceiptBookInner>>,
}

struct ReceiptBookInner {
    file_path: PathBuf,
    sequence: u64,
    prev_hash: String,
    secret_key: Vec<u8>,
    signer_id: String,
}

impl ReceiptBook {
    pub fn new(
        path: impl Into<PathBuf>,
        secret_key: &[u8],
        signer_id: &str,
    ) -> Result<Self, FastMcpError> {
        let file_path = path.into();
        let (sequence, prev_hash) = if file_path.exists() {
            let verified = verify_receipt_chain_file(&file_path, Some(secret_key))?;
            (verified.count as u64 + 1, verified.last_hash)
        } else {
            (
                1,
                "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            )
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(ReceiptBookInner {
                file_path,
                sequence,
                prev_hash,
                secret_key: secret_key.to_vec(),
                signer_id: signer_id.to_string(),
            })),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_execution(
        &self,
        session_id: &str,
        tool_name: &str,
        schema_hash: &str,
        input_args: &Value,
        output_res: &Value,
        duration_us: u64,
        status: ReceiptStatus,
    ) -> Result<SignedReceiptRecord, FastMcpError> {
        let canonical_input_hash = hash_canonical_json(input_args)?;
        let canonical_output_hash = hash_canonical_json(output_res)?;
        let now = chrono_or_fallback_timestamp();

        let mut guard = self.inner.lock();
        let receipt = ExecutionReceipt {
            version: 1,
            sequence: guard.sequence,
            timestamp_utc: now,
            prev_receipt_hash: guard.prev_hash.clone(),
            session_id: session_id.to_string(),
            tool_name: tool_name.to_string(),
            tool_schema_hash: schema_hash.to_string(),
            canonical_input_hash,
            canonical_output_hash,
            execution_duration_us: duration_us,
            exit_status: status,
        };

        let signed = receipt.sign(&guard.secret_key, &guard.signer_id)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&guard.file_path)?;

        let line = serde_json::to_string(&signed).map_err(FastMcpError::Serialization)?;
        writeln!(file, "{}", line)?;

        guard.prev_hash = signed.receipt_hash.clone();
        guard.sequence += 1;

        Ok(signed)
    }
}

fn chrono_or_fallback_timestamp() -> String {
    let now = std::time::SystemTime::now();
    match now.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("{}.{:03}Z", d.as_secs(), d.subsec_millis()),
        Err(_) => "1970-01-01T00:00:00.000Z".to_string(),
    }
}

/// Summary of receipt chain verification.
///
/// Note on Tail Truncation (SEC-09):
/// An adversary with file modification permissions could truncate trailing records from the end of the
/// receipt chain without invalidating the cryptographic validity of earlier records. To guard against
/// tail truncation, callers should cross-reference `count` or sequence numbers against external state
/// or periodic out-of-band audit checkpoints.
#[derive(Debug)]
pub struct VerificationSummary {
    pub count: usize,
    pub last_hash: String,
    pub signatures_verified: bool,
}

/// Verify an entire chain of signed receipts from a file.
pub fn verify_receipt_chain_file(
    path: &Path,
    secret_key: Option<&[u8]>,
) -> Result<VerificationSummary, FastMcpError> {
    let signatures_verified = secret_key.is_some();
    if !path.exists() {
        return Ok(VerificationSummary {
            count: 0,
            last_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            signatures_verified,
        });
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut expected_seq = 1u64;
    let mut expected_prev_hash =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let mut total_verified = 0;

    for (line_idx, line) in reader.lines().enumerate() {
        let line_str = line?;
        let trimmed = line_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        let record: SignedReceiptRecord = serde_json::from_str(trimmed).map_err(|e| {
            FastMcpError::InvalidRequest(format!(
                "Line {}: Corrupted receipt JSON: {}",
                line_idx + 1,
                e
            ))
        })?;

        if record.receipt.sequence != expected_seq {
            return Err(FastMcpError::SecurityViolation(format!(
                "Receipt sequence broken at line {}: expected #{}, found #{}",
                line_idx + 1,
                expected_seq,
                record.receipt.sequence
            )));
        }

        if record.receipt.prev_receipt_hash != expected_prev_hash {
            return Err(FastMcpError::SecurityViolation(format!(
                "Receipt hash chain broken at line {}: expected prev_hash '{}', found '{}'",
                line_idx + 1,
                expected_prev_hash,
                record.receipt.prev_receipt_hash
            )));
        }

        if !record.verify(secret_key)? {
            return Err(FastMcpError::SecurityViolation(format!(
                "Receipt cryptographic signature verification failed at line {} (sequence #{})",
                line_idx + 1,
                record.receipt.sequence
            )));
        }

        expected_prev_hash = record.receipt_hash.clone();
        expected_seq += 1;
        total_verified += 1;
    }

    Ok(VerificationSummary {
        count: total_verified,
        last_hash: expected_prev_hash,
        signatures_verified,
    })
}
