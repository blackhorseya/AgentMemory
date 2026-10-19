//! Metadata-plane storage ports, one per aggregate.

use async_trait::async_trait;
use memory_domain::error::DomainResult;
use memory_domain::meta::{
    AclEntry, Agent, AgentBinding, AgentStatus, Asset, AssetStatus, AssetType, ParticipationLog,
    Task, TaskAgent, TaskStatus, Team, TeamMember, User, UserKey, UserStatus, UserType, Visibility,
};
use memory_domain::scope::InstanceId;
use serde::{Deserialize, Serialize};

use crate::common::{BatchDeleteResult, Page};

#[derive(Debug, Clone, Default)]
pub struct UserFilter {
    pub team_id: Option<String>,
    pub user_ids: Option<Vec<String>>,
    pub username: Option<String>,
    pub user_type: Option<UserType>,
    pub status: Option<UserStatus>,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Inserts the user and its default key atomically.
    async fn create_user(
        &self,
        instance: &InstanceId,
        user: &User,
        default_key: &UserKey,
    ) -> DomainResult<()>;
    async fn get_user(&self, instance: &InstanceId, user_id: &str) -> DomainResult<Option<User>>;
    async fn find_user_by_name(
        &self,
        instance: &InstanceId,
        username: &str,
    ) -> DomainResult<Option<User>>;
    async fn list_users(
        &self,
        instance: &InstanceId,
        filter: &UserFilter,
        page: Page,
    ) -> DomainResult<(Vec<User>, usize)>;
    async fn count_users(&self, instance: &InstanceId) -> DomainResult<usize>;
    /// Cascades keys, memberships and user ACL rows.
    async fn delete_users(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult>;
}

#[async_trait]
pub trait UserKeyRepository: Send + Sync {
    async fn insert_key(&self, instance: &InstanceId, key: &UserKey) -> DomainResult<()>;
    async fn find_key_by_value(
        &self,
        instance: &InstanceId,
        value: &str,
    ) -> DomainResult<Option<UserKey>>;
    async fn get_key(&self, instance: &InstanceId, key_id: &str) -> DomainResult<Option<UserKey>>;
    async fn list_keys(
        &self,
        instance: &InstanceId,
        user_id: &str,
        page: Page,
    ) -> DomainResult<(Vec<UserKey>, usize)>;
    async fn update_key(&self, instance: &InstanceId, key: &UserKey) -> DomainResult<()>;
    /// Physically deletes the key; promotes the oldest remaining active key
    /// to default when the default was removed.
    async fn delete_key(&self, instance: &InstanceId, key_id: &str) -> DomainResult<()>;
    async fn touch_key(&self, instance: &InstanceId, key_id: &str, at: &str) -> DomainResult<()>;
}

#[async_trait]
pub trait TeamRepository: Send + Sync {
    /// Inserts the team and makes the owner an admin member.
    async fn create_team(
        &self,
        instance: &InstanceId,
        team: &Team,
        owner_membership: &TeamMember,
    ) -> DomainResult<()>;
    async fn get_team(&self, instance: &InstanceId, team_id: &str) -> DomainResult<Option<Team>>;
    async fn update_team(&self, instance: &InstanceId, team: &Team) -> DomainResult<()>;
    /// Teams `user_id` is an active member of; `None` lists every team.
    async fn list_teams_of_user(
        &self,
        instance: &InstanceId,
        user_id: Option<&str>,
        name: Option<&str>,
        page: Page,
    ) -> DomainResult<(Vec<Team>, usize)>;
    async fn count_teams(&self, instance: &InstanceId) -> DomainResult<usize>;
    /// Cascades members, agents, tasks, assets, bindings and ACL rows.
    async fn delete_teams(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult>;
}

#[async_trait]
pub trait MemberRepository: Send + Sync {
    async fn upsert_member(&self, instance: &InstanceId, member: &TeamMember) -> DomainResult<()>;
    async fn get_member(
        &self,
        instance: &InstanceId,
        team_id: &str,
        user_id: &str,
    ) -> DomainResult<Option<TeamMember>>;
    async fn list_members(
        &self,
        instance: &InstanceId,
        team_id: &str,
        page: Page,
    ) -> DomainResult<(Vec<TeamMember>, usize)>;
    async fn list_memberships(
        &self,
        instance: &InstanceId,
        user_id: &str,
    ) -> DomainResult<Vec<TeamMember>>;
    async fn remove_member(
        &self,
        instance: &InstanceId,
        team_id: &str,
        user_id: &str,
    ) -> DomainResult<bool>;
}

#[derive(Debug, Clone, Default)]
pub struct AgentFilter {
    pub team_id: Option<String>,
    pub owner_user_id: Option<String>,
    pub status: Option<AgentStatus>,
    pub name: Option<String>,
}

#[async_trait]
pub trait AgentRepository: Send + Sync {
    async fn insert_agent(&self, instance: &InstanceId, agent: &Agent) -> DomainResult<()>;
    async fn get_agent(&self, instance: &InstanceId, agent_id: &str)
        -> DomainResult<Option<Agent>>;
    async fn update_agent(&self, instance: &InstanceId, agent: &Agent) -> DomainResult<()>;
    async fn list_agents(
        &self,
        instance: &InstanceId,
        filter: &AgentFilter,
        page: Page,
    ) -> DomainResult<(Vec<Agent>, usize)>;
    /// Cascades task links, bindings and the agent's chat-memory asset.
    async fn delete_agents(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult>;
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub team_id: Option<String>,
    pub creator_user_id: Option<String>,
    pub status: Option<TaskStatus>,
}

#[derive(Debug, Clone, Default)]
pub struct ParticipationFilter {
    pub team_id: String,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub created_after: Option<String>,
    pub created_before: Option<String>,
}

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn insert_task(
        &self,
        instance: &InstanceId,
        task: &Task,
        links: &[TaskAgent],
    ) -> DomainResult<()>;
    async fn get_task(&self, instance: &InstanceId, task_id: &str) -> DomainResult<Option<Task>>;
    async fn update_task(&self, instance: &InstanceId, task: &Task) -> DomainResult<()>;
    async fn list_tasks(
        &self,
        instance: &InstanceId,
        filter: &TaskFilter,
        page: Page,
    ) -> DomainResult<(Vec<Task>, usize)>;
    async fn delete_tasks(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult>;
    async fn link_agent(&self, instance: &InstanceId, link: &TaskAgent) -> DomainResult<()>;
    async fn unlink_agent(
        &self,
        instance: &InstanceId,
        task_id: &str,
        agent_id: &str,
    ) -> DomainResult<bool>;
    async fn list_task_agents(
        &self,
        instance: &InstanceId,
        task_id: &str,
        page: Page,
    ) -> DomainResult<(Vec<TaskAgent>, usize)>;
    async fn append_participation(
        &self,
        instance: &InstanceId,
        log: &ParticipationLog,
    ) -> DomainResult<()>;
    async fn list_participation(
        &self,
        instance: &InstanceId,
        filter: &ParticipationFilter,
        page: Page,
    ) -> DomainResult<(Vec<ParticipationLog>, usize)>;
}

#[derive(Debug, Clone, Default)]
pub struct AssetFilter {
    pub team_id: Option<String>,
    pub asset_type: Option<AssetType>,
    pub status: Option<AssetStatus>,
    pub owner_user_id: Option<String>,
    pub visibility: Option<Vec<Visibility>>,
}

#[async_trait]
pub trait AssetRepository: Send + Sync {
    async fn insert_asset(&self, instance: &InstanceId, asset: &Asset) -> DomainResult<()>;
    async fn get_asset(&self, instance: &InstanceId, asset_id: &str)
        -> DomainResult<Option<Asset>>;
    async fn update_asset(&self, instance: &InstanceId, asset: &Asset) -> DomainResult<()>;
    /// All matching assets, newest first (filtering by permission happens
    /// in the use case).
    async fn list_assets(
        &self,
        instance: &InstanceId,
        filter: &AssetFilter,
    ) -> DomainResult<Vec<Asset>>;
    /// Idempotent; cascades bindings and ACL rows.
    async fn delete_assets(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult>;
    async fn touch_asset(
        &self,
        instance: &InstanceId,
        asset_id: &str,
        at: &str,
    ) -> DomainResult<()>;
}

#[async_trait]
pub trait BindingRepository: Send + Sync {
    /// Replaces every binding of the agent.
    async fn replace_bindings(
        &self,
        instance: &InstanceId,
        agent_id: &str,
        bindings: &[AgentBinding],
    ) -> DomainResult<()>;
    /// Adds a binding unless one already exists for (agent, asset).
    async fn ensure_binding(
        &self,
        instance: &InstanceId,
        binding: &AgentBinding,
    ) -> DomainResult<()>;
    async fn list_bindings(
        &self,
        instance: &InstanceId,
        agent_id: &str,
    ) -> DomainResult<Vec<AgentBinding>>;
}

#[async_trait]
pub trait AclRepository: Send + Sync {
    /// Upserts on (asset, subject, permission).
    async fn grant(&self, instance: &InstanceId, entry: &AclEntry) -> DomainResult<AclEntry>;
    async fn revoke(&self, instance: &InstanceId, id: &str) -> DomainResult<Option<AclEntry>>;
    async fn get_acl(&self, instance: &InstanceId, id: &str) -> DomainResult<Option<AclEntry>>;
    async fn list_acl(&self, instance: &InstanceId, asset_id: &str) -> DomainResult<Vec<AclEntry>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigParam {
    pub module: String,
    pub param_name: String,
    pub param_value: String,
    /// `global` or `user`.
    pub scope: String,
}

#[async_trait]
pub trait ConfigRepository: Send + Sync {
    /// Effective params: user values override global ones.
    async fn get_params(
        &self,
        instance: &InstanceId,
        user_id: Option<&str>,
        module: &str,
    ) -> DomainResult<Vec<ConfigParam>>;
    async fn set_user_params(
        &self,
        instance: &InstanceId,
        user_id: &str,
        module: &str,
        params: &[(String, String)],
    ) -> DomainResult<()>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEntity {
    pub knowledge_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub service_url: String,
    pub name: String,
    pub summary: Option<String>,
    pub team_id: String,
    pub user_id: Option<String>,
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[async_trait]
pub trait KnowledgeRepository: Send + Sync {
    async fn upsert_knowledge(
        &self,
        instance: &InstanceId,
        entity: &KnowledgeEntity,
    ) -> DomainResult<()>;
    async fn get_knowledge(
        &self,
        instance: &InstanceId,
        id: &str,
    ) -> DomainResult<Option<KnowledgeEntity>>;
    async fn delete_knowledge(
        &self,
        instance: &InstanceId,
        ids: &[String],
    ) -> DomainResult<BatchDeleteResult>;
    async fn list_knowledge(
        &self,
        instance: &InstanceId,
        team_id: &str,
        kind: Option<&str>,
        ids: Option<&[String]>,
        page: Page,
    ) -> DomainResult<(Vec<KnowledgeEntity>, usize)>;
}

/// Everything the metadata use cases need, as one injectable bundle.
pub trait MetaStore:
    UserRepository
    + UserKeyRepository
    + TeamRepository
    + MemberRepository
    + AgentRepository
    + TaskRepository
    + AssetRepository
    + BindingRepository
    + AclRepository
    + ConfigRepository
    + KnowledgeRepository
{
}

impl<T> MetaStore for T where
    T: UserRepository
        + UserKeyRepository
        + TeamRepository
        + MemberRepository
        + AgentRepository
        + TaskRepository
        + AssetRepository
        + BindingRepository
        + AclRepository
        + ConfigRepository
        + KnowledgeRepository
{
}
