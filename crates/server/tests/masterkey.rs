//! The master key kept wrapped in Postgres, with a stand-in for KMS.
//!
//! Needs Postgres: set `HYPERBEAM_TEST_DATABASE_URL` as for the end-to-end
//! tests. Without it the tests are skipped.

use anyhow::{Result, bail};
use async_trait::async_trait;
use hyperbeam_server::crypto::SecretBox;
use hyperbeam_server::masterkey::{KeyWrapper, NewKey, Rotated, enveloped, rotate};
use hyperbeam_server::store;
use std::sync::atomic::{AtomicUsize, Ordering};

/// "Wraps" by prefixing its own name, and unwraps only what it wrapped.
struct FakeKms {
    name: &'static str,
    generated: AtomicUsize,
}

impl FakeKms {
    fn new(name: &'static str) -> Self {
        FakeKms { name, generated: AtomicUsize::new(0) }
    }
}

#[async_trait]
impl KeyWrapper for FakeKms {
    fn key_id(&self) -> &str {
        self.name
    }

    async fn generate(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        self.generated.fetch_add(1, Ordering::SeqCst);
        let key: [u8; 32] = rand::random();
        Ok((key.to_vec(), [self.name.as_bytes(), &key].concat()))
    }

    async fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>> {
        match wrapped.strip_prefix(self.name.as_bytes()) {
            Some(key) => Ok(key.to_vec()),
            None => bail!("not wrapped under {}", self.name),
        }
    }
}

async fn database() -> Option<sqlx::PgPool> {
    let Ok(admin_url) = std::env::var("HYPERBEAM_TEST_DATABASE_URL") else {
        eprintln!("skipping: HYPERBEAM_TEST_DATABASE_URL is not set");
        return None;
    };
    let name = format!("nm_test_{}", uuid::Uuid::new_v4().simple());
    let admin = sqlx::PgPool::connect(&admin_url).await.expect("connect to test Postgres");
    sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
    let mut url = url::Url::parse(&admin_url).unwrap();
    url.set_path(&name);
    let db = sqlx::PgPool::connect(url.as_str()).await.unwrap();
    sqlx::migrate!("./migrations").run(&db).await.unwrap();
    Some(db)
}

#[tokio::test]
async fn the_key_is_generated_once_and_unwrapped_on_every_later_start() {
    let Some(db) = database().await else { return };

    let first = FakeKms::new("kms-a");
    let secrets = enveloped(&db, &first).await.unwrap();
    store::put_secret(&db, &secrets, "integration/github", "ghp_secret").await.unwrap();

    // A restart: nothing in memory carries over, only the wrapped key in Postgres.
    let restarted = FakeKms::new("kms-a");
    let secrets = enveloped(&db, &restarted).await.unwrap();
    assert_eq!(store::get_secret(&db, &secrets, "integration/github").await.unwrap().as_deref(), Some("ghp_secret"));
    assert_eq!((first.generated.load(Ordering::SeqCst), restarted.generated.load(Ordering::SeqCst)), (1, 0));

    // The plaintext key is nowhere in the database.
    let (provider, key_id, wrapped): (String, String, Vec<u8>) =
        sqlx::query_as("SELECT provider, key_id, wrapped FROM master_key").fetch_one(&db).await.unwrap();
    assert_eq!((provider.as_str(), key_id.as_str()), ("aws-kms", "kms-a"));
    assert!(wrapped.starts_with(b"kms-a"));

    // A different KMS key cannot unwrap it, and must not quietly start a new one.
    let other = FakeKms::new("kms-b");
    let err = enveloped(&db, &other).await.err().expect("must fail");
    assert!(format!("{err:#}").contains("could not unwrap"), "{err:#}");
    assert_eq!(other.generated.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_installation_with_credentials_under_a_direct_key_is_not_rekeyed() {
    let Some(db) = database().await else { return };
    let direct = hyperbeam_server::crypto::SecretBox::from_base64(&hyperbeam_server::crypto::SecretBox::generate_key()).unwrap();
    store::put_secret(&db, &direct, "integration/github", "ghp_secret").await.unwrap();

    let kms = FakeKms::new("kms-a");
    let err = enveloped(&db, &kms).await.err().expect("must fail");
    assert!(format!("{err:#}").contains("already holds 1 credential"), "{err:#}");
    assert_eq!(kms.generated.load(Ordering::SeqCst), 0);
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM master_key").fetch_one(&db).await.unwrap();
    assert_eq!(rows, 0);
}

fn direct_key() -> SecretBox {
    SecretBox::from_base64(&SecretBox::generate_key()).unwrap()
}

#[tokio::test]
async fn rotating_a_direct_key_moves_every_credential_and_can_be_run_again() {
    let Some(db) = database().await else { return };
    let (old, new) = (direct_key(), direct_key());
    store::put_secret(&db, &old, "integration/github", "ghp_secret").await.unwrap();
    store::put_secret(&db, &old, "integration/notion", "ntn_secret").await.unwrap();

    assert_eq!(rotate(&db, &old, NewKey::Supplied(&new)).await.unwrap(), Rotated { moved: 2, already: 0 });
    assert_eq!(store::get_secret(&db, &new, "integration/github").await.unwrap().as_deref(), Some("ghp_secret"));
    assert_eq!(store::get_secret(&db, &new, "integration/notion").await.unwrap().as_deref(), Some("ntn_secret"));
    assert!(store::get_secret(&db, &old, "integration/github").await.is_err());

    // Running it again finds the work done rather than failing.
    assert_eq!(rotate(&db, &old, NewKey::Supplied(&new)).await.unwrap(), Rotated { moved: 0, already: 2 });

    let err = rotate(&db, &new, NewKey::Supplied(&new)).await.expect_err("must fail");
    assert!(format!("{err:#}").contains("same as the current one"), "{err:#}");
}

#[tokio::test]
async fn a_rotation_that_cannot_read_everything_changes_nothing() {
    let Some(db) = database().await else { return };
    let (old, stray, new) = (direct_key(), direct_key(), direct_key());
    store::put_secret(&db, &old, "integration/github", "ghp_secret").await.unwrap();
    store::put_secret(&db, &stray, "integration/notion", "ntn_secret").await.unwrap();

    let err = rotate(&db, &old, NewKey::Supplied(&new)).await.expect_err("must fail");
    let message = format!("{err:#}");
    assert!(message.contains("cannot read 1 of 2") && message.contains("integration/notion"), "{message}");
    assert_eq!(store::get_secret(&db, &old, "integration/github").await.unwrap().as_deref(), Some("ghp_secret"));
    assert!(store::get_secret(&db, &new, "integration/github").await.is_err());
}

#[tokio::test]
async fn rotation_moves_between_a_direct_key_and_wrapped_ones() {
    let Some(db) = database().await else { return };
    let direct = direct_key();
    store::put_secret(&db, &direct, "integration/github", "ghp_secret").await.unwrap();
    let wrapped_under = |db: sqlx::PgPool| async move {
        sqlx::query_as::<_, (String, Vec<u8>)>("SELECT key_id, wrapped FROM master_key").fetch_all(&db).await.unwrap()
    };
    let read = |secrets: SecretBox, db: sqlx::PgPool| async move {
        store::get_secret(&db, &secrets, "integration/github").await.unwrap().unwrap()
    };

    // Direct → KMS, which a plain start refuses to do.
    let kms_a = FakeKms::new("kms-a");
    assert_eq!(rotate(&db, &direct, NewKey::Enveloped(&kms_a)).await.unwrap(), Rotated { moved: 1, already: 0 });
    let first = wrapped_under(db.clone()).await;
    assert_eq!(first[0].0, "kms-a");
    let on_a = enveloped(&db, &kms_a).await.unwrap();
    assert_eq!(read(on_a.clone(), db.clone()).await, "ghp_secret");

    // A new data key under the same KMS key.
    rotate(&db, &on_a, NewKey::Enveloped(&kms_a)).await.unwrap();
    let second = wrapped_under(db.clone()).await;
    assert_eq!((second.len(), second[0].0.as_str()), (1, "kms-a"));
    assert_ne!(first[0].1, second[0].1);
    assert!(store::get_secret(&db, &on_a, "integration/github").await.is_err());
    let on_a = enveloped(&db, &kms_a).await.unwrap();
    assert_eq!(read(on_a.clone(), db.clone()).await, "ghp_secret");

    // Onto another KMS key.
    let kms_b = FakeKms::new("kms-b");
    rotate(&db, &on_a, NewKey::Enveloped(&kms_b)).await.unwrap();
    assert!(enveloped(&db, &kms_a).await.is_err());
    let on_b = enveloped(&db, &kms_b).await.unwrap();
    assert_eq!(read(on_b.clone(), db.clone()).await, "ghp_secret");

    // And back to a key held directly: nothing wrapped is left behind.
    let direct = direct_key();
    rotate(&db, &on_b, NewKey::Supplied(&direct)).await.unwrap();
    assert!(wrapped_under(db.clone()).await.is_empty());
    assert_eq!(read(direct, db.clone()).await, "ghp_secret");
}
