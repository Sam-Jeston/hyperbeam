//! Where the master key comes from: given directly, read from AWS Secrets
//! Manager, or generated once and kept wrapped under an AWS KMS key. Also
//! moving an installation from one key to another.

use crate::config::{MasterKeyArgs, RotateArgs};
use crate::crypto::SecretBox;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use sqlx::PgPool;

/// Binds the wrapped key to its purpose: KMS refuses to unwrap it for a
/// request that does not present the same context, and logs it in CloudTrail.
const KMS_CONTEXT: (&str, &str) = ("hyperbeam", "master-key");

/// Resolve the configured source into the box that encrypts stored secrets.
pub async fn load(args: &MasterKeyArgs, db: &PgPool) -> Result<SecretBox> {
    if let Some(key) = &args.master_key {
        return SecretBox::from_base64(key).context("HYPERBEAM_MASTER_KEY");
    }
    if let Some(secret_id) = &args.aws_secret {
        return from_secrets_manager(secret_id, None).await.with_context(|| format!("HYPERBEAM_MASTER_KEY_AWS_SECRET ({secret_id})"));
    }
    if let Some(key_id) = &args.aws_kms_key {
        let kms = AwsKms { client: aws_sdk_kms::Client::new(&aws(key_id).await), key_id: key_id.clone() };
        return enveloped(db, &kms).await.with_context(|| format!("HYPERBEAM_MASTER_KEY_AWS_KMS_KEY ({key_id})"));
    }
    bail!("no master key configured: set HYPERBEAM_MASTER_KEY, HYPERBEAM_MASTER_KEY_AWS_SECRET or HYPERBEAM_MASTER_KEY_AWS_KMS_KEY")
}

/// Credentials and region from the SDK's usual chain (environment, IRSA /
/// Pod Identity, instance profile, shared config). An ARN names its own
/// region, which wins over the ambient one.
async fn aws(resource: &str) -> SdkConfig {
    let mut loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(region) = arn_region(resource) {
        loader = loader.region(Region::new(region.to_string()));
    }
    loader.load().await
}

/// `arn:aws:kms:eu-west-1:111122223333:key/…` → `eu-west-1`.
fn arn_region(resource: &str) -> Option<&str> {
    let mut parts = resource.split(':');
    (parts.next() == Some("arn")).then_some(())?;
    parts.nth(2).filter(|region| !region.is_empty())
}

// ---------------------------------------------------------- Secrets Manager

/// `stage` picks a version of the secret by its staging label; the default
/// is the current one (`AWSCURRENT`).
async fn from_secrets_manager(secret_id: &str, stage: Option<&str>) -> Result<SecretBox> {
    let client = aws_sdk_secretsmanager::Client::new(&aws(secret_id).await);
    let out = client
        .get_secret_value()
        .secret_id(secret_id)
        .set_version_stage(stage.map(str::to_string))
        .send()
        .await
        .map_err(|e| anyhow!("could not read the secret: {}", aws_sdk_secretsmanager::error::DisplayErrorContext(&e)))?;
    key_from_secret(out.secret_string(), out.secret_binary().map(|b| b.as_ref()))
}

/// The secret holds the key in one of three shapes: the base64 text itself,
/// a JSON object with it under `master_key` (what the console's key/value
/// editor produces), or the 32 raw bytes as a binary secret.
fn key_from_secret(string: Option<&str>, binary: Option<&[u8]>) -> Result<SecretBox> {
    match (string, binary) {
        (Some(text), _) if text.trim_start().starts_with('{') => {
            let json: serde_json::Value = serde_json::from_str(text).context("the secret looks like JSON but does not parse")?;
            let key = json
                .get("master_key")
                .and_then(|v| v.as_str())
                .context("the secret is a JSON object without a `master_key` string")?;
            SecretBox::from_base64(key)
        }
        (Some(text), _) => SecretBox::from_base64(text),
        (None, Some(bytes)) if bytes.len() == 32 => SecretBox::from_bytes(bytes),
        (None, Some(bytes)) => {
            SecretBox::from_base64(std::str::from_utf8(bytes).ok().context("the binary secret is neither 32 bytes nor base64 text")?)
        }
        (None, None) => bail!("the secret has no value"),
    }
}

// ---------------------------------------------------------------------- KMS

/// A service that hands out data keys and unwraps them again.
#[async_trait]
pub trait KeyWrapper: Send + Sync {
    /// Recorded next to the wrapped key.
    fn key_id(&self) -> &str;
    /// A new 32-byte key, as `(plaintext, wrapped)`.
    async fn generate(&self) -> Result<(Vec<u8>, Vec<u8>)>;
    async fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>>;
}

struct AwsKms {
    client: aws_sdk_kms::Client,
    key_id: String,
}

#[async_trait]
impl KeyWrapper for AwsKms {
    fn key_id(&self) -> &str {
        &self.key_id
    }

    async fn generate(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let out = self
            .client
            .generate_data_key()
            .key_id(&self.key_id)
            .key_spec(aws_sdk_kms::types::DataKeySpec::Aes256)
            .encryption_context(KMS_CONTEXT.0, KMS_CONTEXT.1)
            .send()
            .await
            .map_err(|e| anyhow!("kms:GenerateDataKey failed: {}", aws_sdk_kms::error::DisplayErrorContext(&e)))?;
        let plaintext = out.plaintext().context("KMS returned no plaintext key")?.as_ref().to_vec();
        let wrapped = out.ciphertext_blob().context("KMS returned no wrapped key")?.as_ref().to_vec();
        Ok((plaintext, wrapped))
    }

    async fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>> {
        let out = self
            .client
            .decrypt()
            // Naming the key makes KMS refuse a blob wrapped under any other.
            .key_id(&self.key_id)
            .ciphertext_blob(aws_sdk_kms::primitives::Blob::new(wrapped))
            .encryption_context(KMS_CONTEXT.0, KMS_CONTEXT.1)
            .send()
            .await
            .map_err(|e| anyhow!("kms:Decrypt failed: {}", aws_sdk_kms::error::DisplayErrorContext(&e)))?;
        Ok(out.plaintext().context("KMS returned no plaintext key")?.as_ref().to_vec())
    }
}

/// Envelope encryption: the master key is a data key that lives in Postgres
/// only in wrapped form. The first start creates it; every start unwraps it.
pub async fn enveloped(db: &PgPool, wrapper: &dyn KeyWrapper) -> Result<SecretBox> {
    if stored(db).await?.is_none() {
        // A fresh key cannot read what an earlier, directly supplied key wrote.
        let (existing,): (i64,) = sqlx::query_as("SELECT count(*) FROM secrets").fetch_one(db).await?;
        if existing > 0 {
            bail!(
                "this installation already holds {existing} credential(s) encrypted under a master key that was supplied directly. \
                 Keep using that key, or move them to KMS with `hyperbeam-server rotate-master-key --new-master-key-aws-kms-key`"
            );
        }
        let (_, wrapped) = wrapper.generate().await?;
        // If another instance got there first, its key stands and ours is dropped.
        sqlx::query("INSERT INTO master_key (provider, key_id, wrapped) VALUES ('aws-kms', $1, $2) ON CONFLICT DO NOTHING")
            .bind(wrapper.key_id())
            .bind(&wrapped)
            .execute(db)
            .await?;
        tracing::info!("generated a new master key, wrapped under {}", wrapper.key_id());
    }
    let wrapped = stored(db).await?.context("the wrapped master key disappeared from the database")?;
    let plaintext = wrapper.unwrap(&wrapped).await.context("could not unwrap the stored master key")?;
    SecretBox::from_bytes(&plaintext)
}

async fn stored(db: &PgPool) -> Result<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT wrapped FROM master_key").fetch_optional(db).await?;
    Ok(row.map(|r| r.0))
}

// ----------------------------------------------------------------- Rotation

/// The key a rotation moves the installation to.
pub enum NewKey<'a> {
    /// One the operator holds, directly or in a secret store. Nothing about
    /// it is kept in the database.
    Supplied(&'a SecretBox),
    /// A fresh data key from this service, kept in the database wrapped.
    Enveloped(&'a dyn KeyWrapper),
}

/// What a rotation did.
#[derive(Debug, PartialEq, Eq)]
pub struct Rotated {
    /// Credentials re-encrypted under the new key.
    pub moved: usize,
    /// Credentials the new key could already read: an earlier run got there.
    pub already: usize,
}

impl std::fmt::Display for Rotated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "re-encrypted {} credential(s) under the new master key", self.moved)?;
        if self.already > 0 {
            write!(f, "; {} were already under it", self.already)?;
        }
        write!(f, ". Restart the server with the new key now: until then it cannot read them, and credentials saved through it are lost")
    }
}

/// Re-encrypt every stored credential from `current` to `new`, and make the
/// wrapped key in the database match. All in one transaction: either the
/// whole installation is on the new key afterwards or nothing has changed.
///
/// A server that is still running keeps the old key in memory, so it has to
/// be restarted (with the new source, if that changed) straight afterwards.
pub async fn rotate(db: &PgPool, current: &SecretBox, new: NewKey<'_>) -> Result<Rotated> {
    // The KMS call comes first: a transaction should not wait on the network.
    // If the rotation then fails, the generated key was never used.
    let (target, wrapped) = match new {
        NewKey::Supplied(target) => {
            // Rotating to the key already in use would only look like a rotation.
            let (nonce, probe) = target.seal("rotation-probe", "")?;
            if current.open("rotation-probe", &nonce, &probe).is_ok() {
                bail!("the new master key is the same as the current one");
            }
            (target.clone(), None)
        }
        NewKey::Enveloped(wrapper) => {
            let (plaintext, wrapped) = wrapper.generate().await?;
            (SecretBox::from_bytes(&plaintext)?, Some((wrapper.key_id().to_string(), wrapped)))
        }
    };

    let mut tx = db.begin().await?;
    // Readers carry on; a credential saved meanwhile waits, and so cannot
    // slip in under the old key behind the rotation's back.
    sqlx::query("LOCK TABLE secrets, master_key IN EXCLUSIVE MODE").execute(&mut *tx).await?;
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> =
        sqlx::query_as("SELECT key, nonce, ciphertext FROM secrets ORDER BY key").fetch_all(&mut *tx).await?;

    let mut outcome = Rotated { moved: 0, already: 0 };
    let mut unreadable = Vec::new();
    for (key, nonce, ciphertext) in &rows {
        let Ok(plaintext) = current.open(key, nonce, ciphertext) else {
            match target.open(key, nonce, ciphertext) {
                Ok(_) => outcome.already += 1,
                Err(_) => unreadable.push(key.as_str()),
            }
            continue;
        };
        let (nonce, ciphertext) = target.seal(key, &plaintext)?;
        // `updated_at` stays: the credential itself has not changed.
        sqlx::query("UPDATE secrets SET nonce = $2, ciphertext = $3 WHERE key = $1")
            .bind(key)
            .bind(nonce)
            .bind(ciphertext)
            .execute(&mut *tx)
            .await?;
        outcome.moved += 1;
    }
    if !unreadable.is_empty() {
        bail!(
            "nothing was changed: the current master key cannot read {} of {} stored credential(s) ({}).              Check that the current key is the one the server runs with; credentials saved under a lost key              have to be removed or entered again first",
            unreadable.len(),
            rows.len(),
            unreadable.join(", ")
        );
    }

    sqlx::query("DELETE FROM master_key").execute(&mut *tx).await?;
    if let Some((key_id, wrapped)) = &wrapped {
        sqlx::query("INSERT INTO master_key (provider, key_id, wrapped) VALUES ('aws-kms', $1, $2)")
            .bind(key_id)
            .bind(wrapped)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(outcome)
}

/// `rotate-master-key`: resolve both keys from the command line and rotate.
pub async fn rotate_configured(args: &RotateArgs) -> Result<Rotated> {
    let db = PgPool::connect(&args.database_url).await.context("could not connect to Postgres (HYPERBEAM_DATABASE_URL)")?;

    // The same secret on both sides means it was given a new value: the key
    // to move away from is then the version Secrets Manager kept as previous.
    let in_place = args.current.aws_secret.as_deref().filter(|id| args.new.new_aws_secret.as_deref() == Some(id));
    let current = match in_place {
        Some(secret_id) => from_secrets_manager(secret_id, Some("AWSPREVIOUS")).await.with_context(|| {
            format!("could not read the previous value (AWSPREVIOUS) of {secret_id}: put the new key in the secret before rotating")
        })?,
        None => load(&args.current, &db).await.context("could not load the current master key")?,
    };

    if let Some(key) = &args.new.new_master_key {
        let new = SecretBox::from_base64(key).context("HYPERBEAM_NEW_MASTER_KEY")?;
        return rotate(&db, &current, NewKey::Supplied(&new)).await;
    }
    if let Some(secret_id) = &args.new.new_aws_secret {
        let new = from_secrets_manager(secret_id, None)
            .await
            .with_context(|| format!("HYPERBEAM_NEW_MASTER_KEY_AWS_SECRET ({secret_id})"))?;
        return rotate(&db, &current, NewKey::Supplied(&new)).await;
    }
    if let Some(key_id) = &args.new.new_aws_kms_key {
        let kms = AwsKms { client: aws_sdk_kms::Client::new(&aws(key_id).await), key_id: key_id.clone() };
        return rotate(&db, &current, NewKey::Enveloped(&kms))
            .await
            .with_context(|| format!("HYPERBEAM_NEW_MASTER_KEY_AWS_KMS_KEY ({key_id})"));
    }
    bail!("no new master key given: set --new-master-key, --new-master-key-aws-secret or --new-master-key-aws-kms-key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn region_comes_from_an_arn_only() {
        assert_eq!(arn_region("arn:aws:kms:eu-west-1:111122223333:key/abc"), Some("eu-west-1"));
        assert_eq!(arn_region("arn:aws-cn:secretsmanager:cn-north-1:111122223333:secret:hb-AbCdEf"), Some("cn-north-1"));
        assert_eq!(arn_region("alias/hyperbeam"), None);
        assert_eq!(arn_region("1234abcd-12ab-34cd-56ef-1234567890ab"), None);
        assert_eq!(arn_region("hyperbeam/master-key"), None);
    }

    #[test]
    fn secret_values_are_read_in_every_supported_shape() {
        let raw = [7u8; 32];
        let b64 = STANDARD.encode(raw);
        let reference = SecretBox::from_bytes(&raw).unwrap();
        let (nonce, ct) = reference.seal("k", "v").unwrap();
        let reads = |sb: SecretBox| sb.open("k", &nonce, &ct).unwrap() == "v";

        assert!(reads(key_from_secret(Some(&format!("{b64}\n")), None).unwrap()));
        assert!(reads(key_from_secret(Some(&format!(r#"{{"master_key": "{b64}"}}"#)), None).unwrap()));
        assert!(reads(key_from_secret(None, Some(&raw)).unwrap()));
        assert!(reads(key_from_secret(None, Some(b64.as_bytes())).unwrap()));

        assert!(key_from_secret(Some(r#"{"other": "x"}"#), None).is_err());
        assert!(key_from_secret(Some("c2hvcnQ="), None).is_err());
        assert!(key_from_secret(None, Some(&[1u8; 16])).is_err());
        assert!(key_from_secret(None, None).is_err());
    }
}
