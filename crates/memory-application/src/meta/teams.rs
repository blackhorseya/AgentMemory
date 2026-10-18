//! Teams and memberships.

use memory_domain::error::{DomainError, DomainResult};
use memory_domain::meta::{MemberStatus, Team, TeamMember, TeamRole, TeamStatus};
use memory_domain::scope::InstanceId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    denied, limit_exceeded, page_of, require_non_empty, Caller, MetaPage, MetaService, Pagination,
};
use crate::common::BatchDeleteResult;

#[derive(Debug, Deserialize)]
pub struct CreateTeamInput {
    pub name: String,
    pub owner_user_id: String,
    pub description: Option<String>,
    pub metadata_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTeamInput {
    pub team_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub metadata_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberInput {
    pub team_id: String,
    pub user_id: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamMemberView {
    #[serde(flatten)]
    pub member: TeamMember,
    pub username: Option<String>,
}

impl MetaService {
    pub async fn create_team(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: CreateTeamInput,
    ) -> DomainResult<Team> {
        require_non_empty("name", &input.name)?;
        if input.owner_user_id != caller.user_id && !caller.is_system_admin {
            return Err(denied("owner must be the caller"));
        }
        self.store
            .get_user(instance, &input.owner_user_id)
            .await?
            .ok_or_else(|| DomainError::not_found("user", &input.owner_user_id))?;
        if self.store.count_teams(instance).await? >= self.settings.max_teams_per_instance {
            return Err(limit_exceeded(
                "team_limit_exceeded",
                "team limit reached for this instance",
            ));
        }
        let now = self.now();
        let team = Team {
            team_id: self.entity_id("team"),
            name: input.name,
            description: input.description,
            owner_user_id: input.owner_user_id.clone(),
            status: TeamStatus::Active,
            created_at: now.clone(),
            updated_at: now.clone(),
            metadata_json: input.metadata_json.unwrap_or_else(|| json!({})),
        };
        let owner = TeamMember {
            id: self.entity_id("tm"),
            team_id: team.team_id.clone(),
            user_id: input.owner_user_id,
            role: TeamRole::Admin,
            status: MemberStatus::Active,
            joined_at: now,
        };
        self.store.create_team(instance, &team, &owner).await?;
        Ok(team)
    }

    pub async fn get_team(&self, instance: &InstanceId, team_id: &str) -> DomainResult<Team> {
        self.store
            .get_team(instance, team_id)
            .await?
            .ok_or_else(|| DomainError::not_found("team", team_id))
    }

    async fn require_team_manager(
        &self,
        instance: &InstanceId,
        team: &Team,
        caller: &Caller,
    ) -> DomainResult<()> {
        if team.owner_user_id == caller.user_id || caller.is_system_admin {
            return Ok(());
        }
        self.require_team_admin(instance, &team.team_id, caller)
            .await
            .map(|_| ())
    }

    /// The owner cannot be changed through update.
    pub async fn update_team(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: UpdateTeamInput,
    ) -> DomainResult<Team> {
        let mut team = self.get_team(instance, &input.team_id).await?;
        self.require_team_manager(instance, &team, caller).await?;
        if let Some(name) = input.name {
            require_non_empty("name", &name)?;
            team.name = name;
        }
        if input.description.is_some() {
            team.description = input.description;
        }
        if let Some(status) = input.status {
            team.status = TeamStatus::parse(&status)?;
        }
        if let Some(meta) = input.metadata_json {
            team.metadata_json = meta;
        }
        team.updated_at = self.now();
        self.store.update_team(instance, &team).await?;
        Ok(team)
    }

    pub async fn delete_teams(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult> {
        let mut allowed = Vec::new();
        let mut result = BatchDeleteResult::default();
        for id in ids {
            match self.store.get_team(instance, id).await? {
                Some(team)
                    if self
                        .require_team_manager(instance, &team, caller)
                        .await
                        .is_ok() =>
                {
                    allowed.push(id.clone())
                }
                Some(_) => result.failed.push(crate::common::FailedId {
                    id: id.clone(),
                    reason: "permission_denied".into(),
                }),
                None => result.failed.push(crate::common::FailedId {
                    id: id.clone(),
                    reason: "not_found".into(),
                }),
            }
        }
        let deleted = self.store.delete_teams(instance, &allowed).await?;
        result.deleted_ids = deleted.deleted_ids;
        result.failed.extend(deleted.failed);
        Ok(result)
    }

    pub async fn list_teams(
        &self,
        instance: &InstanceId,
        user_id: &str,
        name: Option<&str>,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<Team>> {
        let page = pagination.page();
        let (teams, total) = self
            .store
            .list_teams_of_user(instance, user_id, name, page)
            .await?;
        Ok(page_of(teams, total, page))
    }

    /// Team admins add members; you cannot add yourself and the owner
    /// cannot be demoted.
    pub async fn add_member(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: AddMemberInput,
    ) -> DomainResult<TeamMember> {
        let team = self.get_team(instance, &input.team_id).await?;
        if !caller.is_system_admin {
            self.require_team_admin(instance, &team.team_id, caller)
                .await?;
        }
        if input.user_id == caller.user_id {
            return Err(denied("cannot add yourself"));
        }
        self.store
            .get_user(instance, &input.user_id)
            .await?
            .ok_or_else(|| DomainError::not_found("user", &input.user_id))?;
        let role = input
            .role
            .as_deref()
            .map(TeamRole::parse)
            .transpose()?
            .unwrap_or(TeamRole::Member);
        if input.user_id == team.owner_user_id && role != TeamRole::Admin {
            return Err(denied("the team owner cannot be demoted"));
        }
        let existing = self
            .store
            .get_member(instance, &team.team_id, &input.user_id)
            .await?;
        if existing
            .as_ref()
            .is_some_and(|m| m.is_active() && m.role == role)
        {
            return Err(DomainError::conflict(
                "member_already_exists",
                "member already has this role",
            ));
        }
        let member = TeamMember {
            id: existing
                .map(|m| m.id)
                .unwrap_or_else(|| self.entity_id("tm")),
            team_id: team.team_id,
            user_id: input.user_id,
            role,
            status: MemberStatus::Active,
            joined_at: self.now(),
        };
        self.store.upsert_member(instance, &member).await?;
        Ok(member)
    }

    pub async fn remove_member(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        team_id: &str,
        user_id: &str,
    ) -> DomainResult<bool> {
        let team = self.get_team(instance, team_id).await?;
        if !caller.is_system_admin {
            self.require_team_admin(instance, team_id, caller).await?;
        }
        if team.owner_user_id == user_id {
            return Err(denied("the team owner cannot be removed"));
        }
        if !self.store.remove_member(instance, team_id, user_id).await? {
            return Err(DomainError::not_found("member", user_id));
        }
        Ok(true)
    }

    async fn with_username(
        &self,
        instance: &InstanceId,
        member: TeamMember,
    ) -> DomainResult<TeamMemberView> {
        let username = self
            .store
            .get_user(instance, &member.user_id)
            .await?
            .map(|u| u.username);
        Ok(TeamMemberView { member, username })
    }

    pub async fn list_members(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        team_id: &str,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<TeamMemberView>> {
        if !caller.is_system_admin {
            self.require_member(instance, team_id, caller).await?;
        }
        let page = pagination.page();
        let (members, total) = self.store.list_members(instance, team_id, page).await?;
        let mut views = Vec::with_capacity(members.len());
        for m in members {
            views.push(self.with_username(instance, m).await?);
        }
        Ok(page_of(views, total, page))
    }

    pub async fn get_member(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        team_id: &str,
        user_id: &str,
    ) -> DomainResult<TeamMemberView> {
        if !caller.is_system_admin {
            self.require_member(instance, team_id, caller).await?;
        }
        let member = self
            .store
            .get_member(instance, team_id, user_id)
            .await?
            .ok_or_else(|| DomainError::not_found("member", user_id))?;
        self.with_username(instance, member).await
    }
}
