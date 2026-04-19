use std::path::Path;

use anyhow::Result;

use super::pull::{apply_pull_error, apply_pull_response, prepare_sync_pull};
use super::push::{apply_push_error, apply_push_success, prepare_sync_push};
use super::remote::SyncRemote;
use super::state::SyncRuntimeConfig;
use crate::storage::localdb::open_con_at;

pub async fn run_sync_cycle<R: SyncRemote + ?Sized>(
    db_path: &Path,
    remote: &R,
    config: &SyncRuntimeConfig,
) -> Result<()> {
    if let Some(prepared_push) = {
        let conn = open_con_at(db_path)?;
        prepare_sync_push(&conn, config)?
    } {
        match remote.push_batch(prepared_push.batch).await {
            Ok(ack) => {
                let conn = open_con_at(db_path)?;
                apply_push_success(&conn, config, &prepared_push.batch_uuid, &ack)?;
            }
            Err(err) => {
                let conn = open_con_at(db_path)?;
                apply_push_error(&conn, config, &err)?;
            }
        }
    };

    {
        let prepared_pull = {
            let conn = open_con_at(db_path)?;
            prepare_sync_pull(&conn, config)?
        };

        match remote
            .pull_since(&config.own_source_uuid, prepared_pull.last_pulled_revision)
            .await
        {
            Ok(response) => {
                let mut conn = open_con_at(db_path)?;
                apply_pull_response(
                    &mut conn,
                    config,
                    prepared_pull.last_pulled_revision,
                    &response,
                )?;
            }
            Err(err) => {
                let conn = open_con_at(db_path)?;
                apply_pull_error(&conn, config, &err)?;
            }
        }
    }

    Ok(())
}
