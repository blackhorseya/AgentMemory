//! Assets, fixed agent bindings and ACL.

use std::collections::HashMap;

use async_trait::async_trait;
use memory_domain::error::{DomainError, DomainResult};
use memory_domain::meta::access::{can_bind, check_asset_permission, AccessRequest, Decision};
use memory_domain::meta::{
    chat_memory_asset_id, AclEntry, AgentBinding, Asset, AssetStatus, AssetType, Effect,
    InjectionMode, Permission, SubjectType, Visibility,
};
use memory_domain::scope::InstanceId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{denied, page_of, require_non_empty, Caller, MetaPage, MetaService, Pagination};
use crate::common::BatchDeleteResult;
use crate::ports::meta::AssetFilter;

/// Wipes the memory content (L0–L3) of a team + agent. Implemented by the
/// chat-memory use case; invoked when an agent is archived.
#[async_trait]
pub trait ChatMemoryCleaner: Send + Sync {
    async fn clear_chat_memory(
        &self,
        instance: &InstanceId,
        team_id: &str,
        agent_id: &str,
    ) -> DomainResult<()>;
}

#[derive(Debug, Deserialize)]
pub struct CreateAssetInput {
    pub asset_id: String,
    pub team_id: String,
    pub asset_type: String,
    pub name: String,
    pub owner_user_id: String,
    pub source_type: String,
    pub description: Option<String>,
    pub source_ref: Option<String>,
    pub visibility: Option<String>,
    pub status: Option<String>,
    pub confidence: Option<f64>,
    pub expires_at: Option<String>,
    pub content_ref: Option<String>,
    pub metadata_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAssetInput {
    pub asset_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
    pub status: Option<String>,
    pub confidence: Option<f64>,
    pub expires_at: Option<String>,
    pub content_ref: Option<String>,
    pub metadata_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ListAssetsInput {
    pub team_id: String,
    pub asset_type: Option<String>,
    pub status: Option<String>,
    pub owner_user_id: Option<String>,
    pub visibility: Option<String>,
    #[serde(flatten)]
    pub pagination: Pagination,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
pub struct ListAccessibleInput {
    pub team_id: Option<String>,
    pub action: Option<String>,
    pub asset_type: Option<String>,
    pub agent_id: Option<String>,
    pub visibility: Option<OneOrMany>,
    #[serde(flatten)]
    pub pagination: Pagination,
}

#[derive(Debug, Deserialize)]
pub struct BindingInput {
    pub asset_id: String,
    pub asset_type: String,
    pub injection_mode: Option<String>,
    pub priority: Option<i64>,
    pub created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GrantInput {
    pub asset_id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub permission: String,
    pub effect: Option<String>,
    pub granted_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BindingDetail {
    #[serde(flatten)]
    pub binding: AgentBinding,
    pub asset: Asset,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAssetSummary {
    pub agent_id: String,
    pub counts: HashMap<&'static str, usize>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub allowed: bool,
    pub reason: String,
}

impl MetaService {
    async fn upsert_auto_asset(
        &self,
        instance: &InstanceId,
        asset: Asset,
        agent_id: &str,
        mode: InjectionMode,
    ) -> DomainResult<Asset> {
        let stored = match self.store.get_asset(instance, &asset.asset_id).await? {
            Some(existing) => existing,
            None => {
                self.store.insert_asset(instance, &asset).await?;
                asset
            }
        };
        let binding = AgentBinding {
            id: self.entity_id("bind"),
            agent_id: agent_id.to_owned(),
            asset_id: stored.asset_id.clone(),
            asset_type: stored.asset_type,
            injection_mode: mode,
            priority: 50,
            created_by: stored.owner_user_id.clone(),
            created_at: self.now(),
        };
        self.store.ensure_binding(instance, &binding).await?;
        Ok(stored)
    }

    fn auto_asset(
        &self,
        id: String,
        team_id: &str,
        kind: AssetType,
        name: String,
        owner: &str,
        source: &str,
    ) -> Asset {
        let now = self.now();
        Asset {
            asset_id: id,
            team_id: team_id.to_owned(),
            asset_type: kind,
            name,
            description: None,
            owner_user_id: owner.to_owned(),
            source_type: source.to_owned(),
            source_ref: None,
            version: 1,
            visibility: Visibility::Private,
            status: AssetStatus::Active,
            confidence: None,
            expires_at: None,
            last_used_at: None,
            usage_count: 0,
            content_ref: None,
            created_at: now.clone(),
            updated_at: now,
            metadata_json: json!({}),
        }
    }

    /// One private `chat_memory-{team}-{agent}` asset per agent, bound to it.
    pub async fn ensure_chat_memory_asset(
        &self,
        instance: &InstanceId,
        team_id: &str,
        agent_id: &str,
    ) -> DomainResult<Asset> {
        let agent = self.get_agent(instance, agent_id).await?;
        if agent.team_id != team_id {
            return Err(DomainError::permission_denied("agent_team_mismatch"));
        }
        let asset = self.auto_asset(
            chat_memory_asset_id(team_id, agent_id),
            team_id,
            AssetType::ChatMemory,
            format!("Memory of {}", agent.name),
            &agent.owner_user_id,
            "auto",
        );
        self.upsert_auto_asset(instance, asset, agent_id, InjectionMode::Summary)
            .await
    }

    /// Registers a skill as a private asset bound to its owner agent. Skills
    /// written for an unknown agent are stored but not registered.
    pub async fn ensure_skill_asset(
        &self,
        instance: &InstanceId,
        skill_id: &str,
        name: &str,
        team_id: &str,
        agent_id: &str,
        owner_user_id: &str,
    ) -> DomainResult<Option<Asset>> {
        if self.store.get_agent(instance, agent_id).await?.is_none() {
            return Ok(None);
        }
        let asset = self.auto_asset(
            skill_id.to_owned(),
            team_id,
            AssetType::Skill,
            name.to_owned(),
            owner_user_id,
            "extracted",
        );
        self.upsert_auto_asset(instance, asset, agent_id, InjectionMode::Reference)
            .await
            .map(Some)
    }

    pub async fn remove_assets(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult> {
        self.store.delete_assets(instance, ids).await
    }

    pub async fn get_asset_unchecked(
        &self,
        instance: &InstanceId,
        asset_id: &str,
    ) -> DomainResult<Option<Asset>> {
        self.store.get_asset(instance, asset_id).await
    }

    async fn decide(
        &self,
        instance: &InstanceId,
        asset: Option<&Asset>,
        user_id: &str,
        agent_id: Option<&str>,
        action: Permission,
    ) -> DomainResult<Decision> {
        let membership = match asset {
            Some(a) => {
                self.active_membership(instance, &a.team_id, user_id)
                    .await?
            }
            None => None,
        };
        let acl = match asset {
            Some(a) => self.store.list_acl(instance, &a.asset_id).await?,
            None => Vec::new(),
        };
        Ok(check_asset_permission(&AccessRequest {
            asset,
            user_id,
            agent_id,
            membership: membership.as_ref(),
            action,
            acl: &acl,
        }))
    }

    pub async fn create_asset(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: CreateAssetInput,
    ) -> DomainResult<Asset> {
        require_non_empty("asset_id", &input.asset_id)?;
        require_non_empty("name", &input.name)?;
        if input.owner_user_id != caller.user_id {
            return Err(denied("owner must be the caller"));
        }
        self.require_member(instance, &input.team_id, caller)
            .await?;
        if self
            .store
            .get_asset(instance, &input.asset_id)
            .await?
            .is_some()
        {
            return Err(DomainError::conflict(
                "duplicate_entry",
                format!("asset exists: {}", input.asset_id),
            ));
        }
        let now = self.now();
        let asset = Asset {
            asset_id: input.asset_id,
            team_id: input.team_id,
            asset_type: AssetType::parse(&input.asset_type)?,
            name: input.name,
            description: input.description,
            owner_user_id: input.owner_user_id,
            source_type: input.source_type,
            source_ref: input.source_ref,
            version: 1,
            visibility: input
                .visibility
                .as_deref()
                .map(Visibility::parse)
                .transpose()?
                .unwrap_or(Visibility::Team),
            status: input
                .status
                .as_deref()
                .map(AssetStatus::parse)
                .transpose()?
                .unwrap_or(AssetStatus::Draft),
            confidence: input.confidence,
            expires_at: input.expires_at,
            last_used_at: None,
            usage_count: 0,
            content_ref: input.content_ref,
            created_at: now.clone(),
            updated_at: now,
            metadata_json: input.metadata_json.unwrap_or_else(|| json!({})),
        };
        self.store.insert_asset(instance, &asset).await?;
        Ok(asset)
    }

    /// 404 rather than 403 so callers cannot probe for existence.
    pub async fn get_asset(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        asset_id: &str,
    ) -> DomainResult<Asset> {
        let asset = self.store.get_asset(instance, asset_id).await?;
        let visible = match &asset {
            Some(a) if caller.is_system_admin || a.owner_user_id == caller.user_id => true,
            Some(a) => self
                .decide(instance, Some(a), &caller.user_id, None, Permission::Read)
                .await?
                .is_allowed(),
            None => false,
        };
        asset
            .filter(|_| visible)
            .ok_or_else(|| DomainError::not_found("asset", asset_id))
    }

    async fn owned_asset(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        asset_id: &str,
    ) -> DomainResult<Asset> {
        let asset = self
            .store
            .get_asset(instance, asset_id)
            .await?
            .ok_or_else(|| DomainError::not_found("asset", asset_id))?;
        if asset.owner_user_id != caller.user_id && !caller.is_system_admin {
            return Err(denied("only the asset owner can do this"));
        }
        Ok(asset)
    }

    pub async fn update_asset(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: UpdateAssetInput,
    ) -> DomainResult<Asset> {
        let mut asset = self.owned_asset(instance, caller, &input.asset_id).await?;
        if let Some(n) = input.name {
            require_non_empty("name", &n)?;
            asset.name = n;
        }
        if input.description.is_some() {
            asset.description = input.description;
        }
        if let Some(v) = input.visibility {
            asset.visibility = Visibility::parse(&v)?;
        }
        if let Some(s) = input.status {
            asset.status = AssetStatus::parse(&s)?;
        }
        if input.confidence.is_some() {
            asset.confidence = input.confidence;
        }
        if input.expires_at.is_some() {
            asset.expires_at = input.expires_at;
        }
        if input.content_ref.is_some() {
            asset.content_ref = input.content_ref;
        }
        if let Some(m) = input.metadata_json {
            asset.metadata_json = m;
        }
        asset.version += 1;
        asset.updated_at = self.now();
        self.store.update_asset(instance, &asset).await?;
        Ok(asset)
    }

    pub async fn delete_assets(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult> {
        let mut allowed = Vec::new();
        let mut failed = Vec::new();
        for id in ids {
            match self.owned_asset(instance, caller, id).await {
                Ok(_) => allowed.push(id.clone()),
                Err(e) if e.kind == memory_domain::error::ErrorKind::NotFound => {
                    allowed.push(id.clone())
                }
                Err(e) => failed.push(crate::common::FailedId {
                    id: id.clone(),
                    reason: e.code,
                }),
            }
        }
        let mut result = self.store.delete_assets(instance, &allowed).await?;
        result.failed.extend(failed);
        Ok(result)
    }

    pub async fn touch_asset(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        asset_id: &str,
    ) -> DomainResult<Asset> {
        self.owned_asset(instance, caller, asset_id).await?;
        self.store
            .touch_asset(instance, asset_id, &self.now())
            .await?;
        self.owned_asset(instance, caller, asset_id).await
    }

    pub async fn list_assets(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: ListAssetsInput,
    ) -> DomainResult<MetaPage<Asset>> {
        let page = input.pagination.page();
        let filter = AssetFilter {
            team_id: Some(input.team_id.clone()),
            asset_type: input
                .asset_type
                .as_deref()
                .map(AssetType::parse)
                .transpose()?,
            status: input
                .status
                .as_deref()
                .map(AssetStatus::parse)
                .transpose()?,
            owner_user_id: input.owner_user_id,
            visibility: input
                .visibility
                .as_deref()
                .map(Visibility::parse)
                .transpose()?
                .map(|v| vec![v]),
        };
        let all = self.store.list_assets(instance, &filter).await?;
        let mut visible = Vec::new();
        for asset in all {
            let ok = caller.is_system_admin
                || asset.owner_user_id == caller.user_id
                || self
                    .decide(
                        instance,
                        Some(&asset),
                        &caller.user_id,
                        None,
                        Permission::Read,
                    )
                    .await?
                    .is_allowed();
            if ok {
                visible.push(asset);
            }
        }
        Ok(paginate(visible, page))
    }

    /// Every asset the user may perform `action` on, across their teams.
    pub async fn list_accessible(
        &self,
        instance: &InstanceId,
        user_id: &str,
        input: ListAccessibleInput,
    ) -> DomainResult<MetaPage<Asset>> {
        let page = input.pagination.page();
        let action = input
            .action
            .as_deref()
            .map(Permission::parse)
            .transpose()?
            .unwrap_or(Permission::Read);
        let teams: Vec<String> = match &input.team_id {
            Some(t) => self
                .active_membership(instance, t, user_id)
                .await?
                .map(|m| vec![m.team_id])
                .unwrap_or_default(),
            None => self
                .store
                .list_memberships(instance, user_id)
                .await?
                .into_iter()
                .filter(|m| m.is_active())
                .map(|m| m.team_id)
                .collect(),
        };
        let visibility = match input.visibility {
            Some(OneOrMany::One(v)) => Some(vec![Visibility::parse(&v)?]),
            Some(OneOrMany::Many(vs)) => Some(
                vs.iter()
                    .map(|v| Visibility::parse(v))
                    .collect::<DomainResult<_>>()?,
            ),
            None => None,
        };
        let mut out = Vec::new();
        for team in teams {
            let filter = AssetFilter {
                team_id: Some(team),
                asset_type: input
                    .asset_type
                    .as_deref()
                    .map(AssetType::parse)
                    .transpose()?,
                visibility: visibility.clone(),
                ..Default::default()
            };
            for asset in self.store.list_assets(instance, &filter).await? {
                if asset.status.is_retired() {
                    continue;
                }
                if self
                    .decide(
                        instance,
                        Some(&asset),
                        user_id,
                        input.agent_id.as_deref(),
                        action,
                    )
                    .await?
                    .is_allowed()
                {
                    out.push(asset);
                }
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.dedup_by(|a, b| a.asset_id == b.asset_id);
        Ok(paginate(out, page))
    }

    pub async fn set_bindings(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        agent_id: &str,
        inputs: Vec<BindingInput>,
    ) -> DomainResult<Vec<AgentBinding>> {
        let agent = self.get_agent(instance, agent_id).await?;
        if agent.owner_user_id != caller.user_id && !caller.is_system_admin {
            self.require_team_admin(instance, &agent.team_id, caller)
                .await?;
        }
        let mut bindings = Vec::new();
        for input in inputs {
            let asset = self
                .store
                .get_asset(instance, &input.asset_id)
                .await?
                .ok_or_else(|| DomainError::not_found("asset", &input.asset_id))?;
            if !can_bind(&asset, &agent.team_id, &agent.owner_user_id) {
                return Err(DomainError::conflict(
                    "asset_not_bindable",
                    format!("asset {} cannot be bound", asset.asset_id),
                ));
            }
            bindings.push(AgentBinding {
                id: self.entity_id("bind"),
                agent_id: agent_id.to_owned(),
                asset_id: asset.asset_id,
                asset_type: AssetType::parse(&input.asset_type)?,
                injection_mode: input
                    .injection_mode
                    .as_deref()
                    .map(InjectionMode::parse)
                    .transpose()?
                    .unwrap_or(InjectionMode::Summary),
                priority: input.priority.unwrap_or(50),
                created_by: input.created_by.unwrap_or_else(|| caller.user_id.clone()),
                created_at: self.now(),
            });
        }
        self.store
            .replace_bindings(instance, agent_id, &bindings)
            .await?;
        Ok(bindings)
    }

    pub async fn list_bindings(
        &self,
        instance: &InstanceId,
        agent_id: &str,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<AgentBinding>> {
        let page = pagination.page();
        Ok(paginate(
            self.store.list_bindings(instance, agent_id).await?,
            page,
        ))
    }

    pub async fn list_bindings_with_detail(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        agent_id: &str,
        apply_visibility_filter: bool,
        touch_usage: bool,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<BindingDetail>> {
        // Only the agent's owner, team admins and system admins see the
        // unfiltered loadout or bump usage counters.
        let agent = self.read_agent(instance, caller, agent_id).await?;
        let privileged = caller.is_system_admin
            || agent.owner_user_id == caller.user_id
            || self
                .require_team_admin(instance, &agent.team_id, caller)
                .await
                .is_ok();
        let apply_visibility_filter = apply_visibility_filter || !privileged;
        let touch_usage = touch_usage && privileged;
        let page = pagination.page();
        let mut details = Vec::new();
        for binding in self.store.list_bindings(instance, agent_id).await? {
            let Some(asset) = self.store.get_asset(instance, &binding.asset_id).await? else {
                continue;
            };
            if apply_visibility_filter
                && asset.owner_user_id != caller.user_id
                && !self
                    .decide(
                        instance,
                        Some(&asset),
                        &caller.user_id,
                        Some(agent_id),
                        Permission::Read,
                    )
                    .await?
                    .is_allowed()
            {
                continue;
            }
            if touch_usage {
                self.store
                    .touch_asset(instance, &asset.asset_id, &self.now())
                    .await?;
            }
            details.push(BindingDetail { binding, asset });
        }
        Ok(paginate(details, page))
    }

    pub async fn binding_summary(
        &self,
        instance: &InstanceId,
        agent_ids: &[String],
        asset_id: Option<&str>,
    ) -> DomainResult<MetaPage<AgentAssetSummary>> {
        let mut items = Vec::new();
        for agent_id in agent_ids {
            let mut counts: HashMap<&'static str, usize> = AssetTypeKeys::zeroed();
            let mut total = 0;
            for b in self.store.list_bindings(instance, agent_id).await? {
                if asset_id.is_some_and(|a| a != b.asset_id) {
                    continue;
                }
                *counts.entry(b.asset_type.as_str()).or_default() += 1;
                total += 1;
            }
            items.push(AgentAssetSummary {
                agent_id: agent_id.clone(),
                counts,
                total,
            });
        }
        let total = items.len();
        Ok(MetaPage {
            items,
            total,
            limit: total,
            offset: 0,
        })
    }

    pub async fn grant(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: GrantInput,
    ) -> DomainResult<AclEntry> {
        self.owned_asset(instance, caller, &input.asset_id).await?;
        if input
            .granted_by
            .as_deref()
            .is_some_and(|g| g != caller.user_id)
        {
            return Err(denied("granted_by must be the caller"));
        }
        let now = self.now();
        let entry = AclEntry {
            id: self.entity_id("acl"),
            asset_id: input.asset_id,
            subject_type: SubjectType::parse(&input.subject_type)?,
            subject_id: input.subject_id,
            permission: Permission::parse(&input.permission)?,
            effect: input
                .effect
                .as_deref()
                .map(Effect::parse)
                .transpose()?
                .unwrap_or(Effect::Allow),
            granted_by: caller.user_id.clone(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.grant(instance, &entry).await
    }

    pub async fn revoke(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        id: &str,
    ) -> DomainResult<AclEntry> {
        let entry = self
            .store
            .get_acl(instance, id)
            .await?
            .ok_or_else(|| DomainError::not_found("acl", id))?;
        self.owned_asset(instance, caller, &entry.asset_id).await?;
        self.store
            .revoke(instance, id)
            .await?
            .ok_or_else(|| DomainError::not_found("acl", id))
    }

    pub async fn list_acl(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        asset_id: &str,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<AclEntry>> {
        let asset = self
            .store
            .get_asset(instance, asset_id)
            .await?
            .ok_or_else(|| DomainError::not_found("asset", asset_id))?;
        if asset.owner_user_id != caller.user_id && !caller.is_system_admin {
            self.require_team_admin(instance, &asset.team_id, caller)
                .await?;
        }
        Ok(paginate(
            self.store.list_acl(instance, asset_id).await?,
            pagination.page(),
        ))
    }

    pub async fn check(
        &self,
        instance: &InstanceId,
        asset_id: &str,
        action: &str,
        user_id: &str,
        agent_id: Option<&str>,
    ) -> DomainResult<CheckResult> {
        let asset = self.store.get_asset(instance, asset_id).await?;
        let decision = self
            .decide(
                instance,
                asset.as_ref(),
                user_id,
                agent_id,
                Permission::parse(action)?,
            )
            .await?;
        Ok(CheckResult {
            allowed: decision.is_allowed(),
            reason: decision.reason().to_owned(),
        })
    }
}

struct AssetTypeKeys;

impl AssetTypeKeys {
    fn zeroed() -> HashMap<&'static str, usize> {
        [
            AssetType::Skill,
            AssetType::CodeGraph,
            AssetType::LlmWiki,
            AssetType::ChatMemory,
        ]
        .into_iter()
        .map(|t| (t.as_str(), 0))
        .collect()
    }
}

fn paginate<T>(items: Vec<T>, page: crate::common::Page) -> MetaPage<T> {
    let total = items.len();
    let items = items
        .into_iter()
        .skip(page.offset)
        .take(page.limit)
        .collect();
    page_of(items, total, page)
}
