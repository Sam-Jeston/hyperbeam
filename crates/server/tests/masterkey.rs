//! The master key kept wrapped in Postgres, with a stand-in for KMS.
//!
//! Needs Postgres: set `HYPERBEAM_TEST_DATABASE_URL` as for the end-to-end
//! tests. Without it the tests are skipped.

use anyhow::{Result, bail};
use async_trait::async_trait;
use hyperbeam_server::masterkey::{KeyWrapper, enveloped};
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
