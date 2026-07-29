//! Storage for a project's remote access token.
//!
//! Like [`crate::discord`] and [`crate::environment`], this module never sees an
//! encryption key. A token arrives as ciphertext and leaves as ciphertext, so the
//! code that can turn a stored blob back into a usable credential lives outside
//! the layer that talks to SQLite.
//!
//! There is deliberately no "list all credentials" query. Nothing needs one, and
//! its only use would be building the report a compromise wants.

use sqlx::Row;

use crate::error::Result;
use crate::time;
use crate::Database;

/// A token for a project's remote, encrypted.
///
/// `Debug` is derived, which is safe precisely because there is no field here a
/// plaintext token could occupy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCredentialRecord {
    pub project_id: String,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// Store or replace a project's token.
///
/// Replacing rather than failing: re-entering a token is what a user does when
/// the old one expired, and that must not require deleting anything first.
pub async fn save_source_credential(
    db: &Database,
    credential: &SourceCredentialRecord,
) -> Result<()> {
    let now = time::now();
    sqlx::query(
        "INSERT INTO project_source_credentials (project_id, ciphertext, nonce,
                                                 created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(project_id) DO UPDATE SET
             ciphertext = excluded.ciphertext,
             nonce      = excluded.nonce,
             updated_at = excluded.updated_at",
    )
    .bind(&credential.project_id)
    .bind(&credential.ciphertext)
    .bind(&credential.nonce)
    .bind(&now)
    .bind(&now)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn load_source_credential(
    db: &Database,
    project_id: &str,
) -> Result<Option<SourceCredentialRecord>> {
    let row = sqlx::query(
        "SELECT ciphertext, nonce FROM project_source_credentials WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(db.pool())
    .await?;

    Ok(row.map(|row| SourceCredentialRecord {
        project_id: project_id.to_string(),
        ciphertext: row.get("ciphertext"),
        nonce: row.get("nonce"),
    }))
}

/// Whether a project has a stored token.
///
/// What the API answers with. A caller asking "is there a credential" must not
/// have to load one to find out, because loading is the operation worth keeping
/// rare.
pub async fn has_source_credential(db: &Database, project_id: &str) -> Result<bool> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_source_credentials WHERE project_id = ?")
            .bind(project_id)
            .fetch_one(db.pool())
            .await?;
    Ok(count > 0)
}

/// Forget a project's token.
///
/// The row goes rather than its columns being blanked, so there is no window in
/// which a zero-length ciphertext looks like a credential. The schema would
/// refuse that row anyway; this makes the intent explicit at both layers.
pub async fn forget_source_credential(db: &Database, project_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM project_source_credentials WHERE project_id = ?")
        .bind(project_id)
        .execute(db.pool())
        .await?;
    Ok(())
}
