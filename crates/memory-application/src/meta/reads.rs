//! Caller-checked reads: team-scoped data is visible to active members of
//! that team (and system admins) only.

use memory_domain::error::DomainResult;
use memory_domain::meta::{Agent, AgentBinding, Task, TaskAgent, Team};
use memory_domain::scope::InstanceId;

use super::{
    denied, AgentAssetSummary, Caller, ListAgentsInput, ListTasksInput, MetaPage, MetaService,
    Pagination,
};

impl MetaService {
    pub(crate) async fn require_reader(
        &self,
        instance: &InstanceId,
        team_id: &str,
        caller: &Caller,
    ) -> DomainResult<()> {
        if caller.is_system_admin {
            return Ok(());
        }
        self.require_member(instance, team_id, caller)
            .await
            .map(|_| ())
    }

    fn require_self(caller: &Caller, user_id: &str) -> DomainResult<()> {
        if caller.is_system_admin || caller.user_id == user_id {
            Ok(())
        } else {
            Err(denied("can only list your own items outside a team"))
        }
    }

    pub async fn read_team(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        team_id: &str,
    ) -> DomainResult<Team> {
        let team = self.get_team(instance, team_id).await?;
        self.require_reader(instance, &team.team_id, caller).await?;
        Ok(team)
    }

    pub async fn read_agent(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        agent_id: &str,
    ) -> DomainResult<Agent> {
        let agent = self.get_agent(instance, agent_id).await?;
        self.require_reader(instance, &agent.team_id, caller)
            .await?;
        Ok(agent)
    }

    pub async fn read_task(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        task_id: &str,
    ) -> DomainResult<Task> {
        let task = self.get_task(instance, task_id).await?;
        self.require_reader(instance, &task.team_id, caller).await?;
        Ok(task)
    }

    pub async fn read_agents(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: ListAgentsInput,
    ) -> DomainResult<MetaPage<Agent>> {
        match (&input.team_id, &input.owner_user_id) {
            (Some(team), _) => self.require_reader(instance, team, caller).await?,
            (None, Some(owner)) => Self::require_self(caller, owner)?,
            (None, None) => {}
        }
        self.list_agents(instance, input).await
    }

    pub async fn read_tasks(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        input: ListTasksInput,
    ) -> DomainResult<MetaPage<Task>> {
        match (&input.team_id, &input.creator_user_id) {
            (Some(team), _) => self.require_reader(instance, team, caller).await?,
            (None, Some(creator)) => Self::require_self(caller, creator)?,
            (None, None) => {}
        }
        self.list_tasks(instance, input).await
    }

    pub async fn read_task_agents(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        task_id: &str,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<TaskAgent>> {
        self.read_task(instance, caller, task_id).await?;
        self.list_task_agents(instance, task_id, pagination).await
    }

    pub async fn read_bindings(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        agent_id: &str,
        pagination: Pagination,
    ) -> DomainResult<MetaPage<AgentBinding>> {
        self.read_agent(instance, caller, agent_id).await?;
        self.list_bindings(instance, agent_id, pagination).await
    }

    pub async fn read_binding_summary(
        &self,
        instance: &InstanceId,
        caller: &Caller,
        agent_ids: &[String],
        asset_id: Option<&str>,
    ) -> DomainResult<MetaPage<AgentAssetSummary>> {
        for id in agent_ids {
            self.read_agent(instance, caller, id).await?;
        }
        self.binding_summary(instance, agent_ids, asset_id).await
    }
}
