use std::io::{self, Write as _};

use crate::{
    PermissionStoreError as Error,
    codec::Change,
    tests::{Fixture, add, grant},
};

#[test]
fn pgr_4_failed_partial_and_unknown_writes_publish_no_success() {
    for fault in ["before", "partial", "after_sync"] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let initial = add(&store, "first");
        let mut transaction = store.transaction(&|| false).expect("transaction");
        let result = transaction.append_with(
            &initial.revision,
            Change::Revoke {
                id: grant("first").id,
            },
            &|| false,
            |file, bytes| {
                match fault {
                    "before" => {}
                    "partial" => {
                        file.write_all(&bytes[..bytes.len() / 2])?;
                        file.sync_all()?;
                    }
                    "after_sync" => {
                        file.write_all(bytes)?;
                        file.sync_all()?;
                    }
                    _ => unreachable!(),
                }
                Err(io::Error::other("injected acknowledgement failure"))
            },
        );
        assert_eq!(result, Err(Error::WriteUncertain));
        // Public mutation consumes the transaction; only a new, fully validated read is possible.
        drop(transaction);
        match fault {
            "before" => assert_eq!(store.read(&|| false).expect("prior source"), initial),
            "partial" => assert_eq!(store.read(&|| false), Err(Error::Corrupt)),
            "after_sync" => assert!(
                store
                    .read(&|| false)
                    .expect("committed revoke")
                    .grants
                    .is_empty()
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
fn pgr_4_lost_grant_ack_retains_the_durable_grant_without_reporting_success() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut transaction = store.transaction(&|| false).expect("transaction");
    let result = transaction.append_with(
        &plexmaton_core::ProjectPermissionRevision::Absent,
        Change::Grant {
            grant: grant("saved"),
        },
        &|| false,
        |file, bytes| {
            file.write_all(bytes)?;
            file.sync_all()?;
            Err(io::Error::other("lost acknowledgement"))
        },
    );
    assert_eq!(result, Err(Error::WriteUncertain));
    drop(transaction);
    assert_eq!(
        store.read(&|| false).expect("fresh read").grants,
        [grant("saved")]
    );
}
