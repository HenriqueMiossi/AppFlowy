use std::sync::Arc;

use collab::core::collab::MutexCollab;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab_document::document::Document;
use collab_document::document_data::default_document_data;
use collab_folder::{Folder, View};
use tracing::{event, instrument};

use collab_integrate::{PersistenceError, RocksCollabDB, YrsDocAction};
use flowy_error::{internal_error, FlowyError, FlowyResult};

use crate::migrations::migration::UserDataMigration;
use crate::migrations::util::load_collab;
use crate::services::entities::Session;

/// Migrate the first level documents of the workspace by inserting documents
pub struct HistoricalEmptyDocumentMigration;

impl UserDataMigration for HistoricalEmptyDocumentMigration {
  fn name(&self) -> &str {
    "historical_empty_document"
  }

  #[instrument(name = "HistoricalEmptyDocumentMigration", skip_all, err)]
  fn run(&self, session: &Session, collab_db: &Arc<RocksCollabDB>) -> FlowyResult<()> {
    let write_txn = collab_db.write_txn();
    let origin = CollabOrigin::Client(CollabClient::new(session.user_id, "phantom"));
    let folder_collab = match load_collab(session.user_id, &write_txn, &session.user_workspace.id) {
      Ok(fc) => fc,
      Err(_) => return Ok(()),
    };

    let folder = Folder::open(session.user_id, folder_collab, None)?;
    let migration_views = folder.get_workspace_views();

    // For historical reasons, the first level documents are empty. So migrate them by inserting
    // the default document data.
    for view in migration_views {
      if let Err(_) = migrate_empty_document(&write_txn, &origin, &view, session.user_id) {
        event!(
          tracing::Level::ERROR,
          "Failed to migrate document {}",
          view.id
        );
      }
    }

    event!(tracing::Level::INFO, "Save all migrated documents");
    write_txn.commit_transaction().map_err(internal_error)?;
    Ok(())
  }
}

fn migrate_empty_document<'a, W>(
  write_txn: &W,
  origin: &CollabOrigin,
  view: &View,
  user_id: i64,
) -> Result<(), FlowyError>
where
  W: YrsDocAction<'a>,
  PersistenceError: From<W::Error>,
{
  if load_collab(user_id, write_txn, &view.id).is_err() {
    let collab = Arc::new(MutexCollab::new(origin.clone(), &view.id, vec![]));
    let document = Document::create_with_data(collab, default_document_data())?;
    let encode = document.get_collab().encode_collab_v1();
    write_txn.flush_doc_with(user_id, &view.id, &encode.doc_state, &encode.state_vector)?;
    event!(
      tracing::Level::INFO,
      "Did migrate empty document {}",
      view.id
    );
  }

  Ok(())
}
