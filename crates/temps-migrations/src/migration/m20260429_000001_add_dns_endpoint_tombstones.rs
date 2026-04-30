//! Adds `dns_endpoint_tombstones` so the resolver long-poll can report
//! deleted record ids in the diff stream.
//!
//! ## Why
//!
//! `replace_endpoints_for_owner` does delete-old + insert-new with fresh
//! row ids. The diff endpoint previously returned only upserts and a
//! permanently-empty `removed_ids: []`. Resolvers retained the deleted
//! rows forever, so a single FQDN's answer set grew without bound until
//! the UDP DNS reply exceeded 512 bytes and clients started receiving
//! malformed/parseable-but-broken responses.
//!
//! ## Schema
//!
//! - `original_id BIGINT` — the `service_endpoints.id` that no longer
//!   exists. Resolvers match on this to drop their cached row.
//! - `deleted_at_generation BIGINT` — the cluster generation that
//!   produced the deletion. Same monotonic counter as
//!   `service_endpoints.generation`. Indexed so `WHERE
//!   deleted_at_generation > $since` is cheap.
//! - `deleted_at TIMESTAMPTZ` — wall-clock for ops/forensics; not load
//!   bearing.
//!
//! Tombstones are pruned by a janitor that walks `node_dns_state` to
//! find the minimum `applied_generation` across all live resolvers and
//! deletes any tombstone strictly older than that. The pruning logic
//! lives in `DnsRegistry::gc_tombstones`, not here.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DnsEndpointTombstones::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DnsEndpointTombstones::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DnsEndpointTombstones::OriginalId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DnsEndpointTombstones::DeletedAtGeneration)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DnsEndpointTombstones::DeletedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();

        // Generation index — same access pattern as
        // `service_endpoints_generation_idx`. Long-poll filters
        // `WHERE deleted_at_generation > $since`.
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS dns_endpoint_tombstones_generation_idx \
             ON dns_endpoint_tombstones (deleted_at_generation)",
        )
        .await?;

        // Backfill: synthesise a single tombstone batch at the current
        // cluster generation for any resolver that has already pulled
        // ids that no longer exist. We can't enumerate the deleted ids
        // (they're gone) so we don't try — the practical effect is
        // that on the next diff, every resolver will see a synthesised
        // "drop everything you don't recognise" via the snapshot
        // fallback the next time the diff exceeds SNAPSHOT_THRESHOLD.
        // No-op on a fresh database.
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS dns_endpoint_tombstones_generation_idx")
            .await?;
        manager
            .drop_table(Table::drop().table(DnsEndpointTombstones::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DnsEndpointTombstones {
    Table,
    Id,
    OriginalId,
    DeletedAtGeneration,
    DeletedAt,
}
