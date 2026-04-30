//! Tombstones for deleted [`super::service_endpoints`] rows.
//!
//! See ADR-011 and migration
//! `m20260429_000001_add_dns_endpoint_tombstones`. Every delete in
//! [`crate::service_endpoints`] writes a tombstone here so the
//! resolver long-poll can include the removed id in its diff. Without
//! this, resolvers retain rows whose primary keys no longer exist in
//! `service_endpoints` (because `replace_endpoints_for_owner` allocates
//! fresh ids on every reconcile) and answer sets grow unboundedly.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use temps_core::DBDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "dns_endpoint_tombstones")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// The `service_endpoints.id` that was deleted. Resolvers match on
    /// this to drop their cached row.
    pub original_id: i64,
    /// The cluster-wide DNS generation at which the deletion happened.
    /// Same monotonic counter as `service_endpoints.generation`. The
    /// long-poll filters `WHERE deleted_at_generation > $since`.
    pub deleted_at_generation: i64,
    pub deleted_at: DBDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
