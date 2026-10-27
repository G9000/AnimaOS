//! Owner-created, daemon-local destinations. DNS is resolved once at admission;
//! worker connections use only the resulting validated numeric pins.
use super::*;
use anima_service_adapters::browser::{
    public_hostname, public_ipv4_pin, BrowserInteractive, BrowserLimits, BrowserOrigin,
    BrowserRoute,
};
use reqwest::Url;

fn destination_urls(
    input: &CreateBrowserDestination,
) -> Result<(Url, Vec<String>), BrowserServiceError> {
    if input.start_url.len() > 2048 || input.allowed_origins.len() > 8 {
        return Err(BrowserServiceError::Invalid);
    }
    let start = Url::parse(&input.start_url).map_err(|_| BrowserServiceError::Invalid)?;
    if start.as_str() != input.start_url
        || !start.username().is_empty()
        || start.password().is_some()
        || start.fragment().is_some()
        || start.port().is_some()
    {
        return Err(BrowserServiceError::Invalid);
    }
    let primary = start.origin().ascii_serialization();
    public_hostname(&primary).map_err(|_| BrowserServiceError::Invalid)?;
    let mut origins = vec![primary];
    for value in &input.allowed_origins {
        public_hostname(value).map_err(|_| BrowserServiceError::Invalid)?;
        if !origins.contains(value) {
            origins.push(value.clone());
        }
    }
    if origins.len() > 8 {
        return Err(BrowserServiceError::Invalid);
    }
    origins.sort();
    Ok((start, origins))
}

fn destination_id(
    agent: &str,
    start: &Url,
    origins: &[String],
    authenticated: bool,
) -> Result<String, BrowserServiceError> {
    let logical = serde_json::to_vec(&(agent, start.as_str(), origins, authenticated))
        .map_err(|_| BrowserServiceError::Invalid)?;
    Ok(format!(
        "destination-{}",
        &hex::encode(Sha256::digest(logical))[..32]
    ))
}

impl BrowserService {
    pub(crate) async fn setup_status(
        &self,
        agent: &str,
    ) -> Result<BrowserSetupView, BrowserServiceError> {
        Self::owner(&*self.state()?.read().await, agent)?;
        let available = self.inner.factory.is_some() && self.inner.policies.installation.is_some();
        Ok(BrowserSetupView { available, reason: (!available).then(|| "Run browser setup, then restart AnimaOS to load the maintained full Chromium installation.".into()), setup_command: "bun run browser:setup".into() })
    }

    pub(crate) async fn create_destination(
        &self,
        agent: &str,
        input: CreateBrowserDestination,
    ) -> Result<BrowserPolicyView, BrowserServiceError> {
        Self::owner(&*self.state()?.read().await, agent)?;
        if self.inner.factory.is_none() || self.inner.policies.installation.is_none() {
            return Err(BrowserServiceError::Unavailable);
        }
        let (start, origins) = destination_urls(&input)?;
        let mut pins = Vec::new();
        for origin in origins {
            let hostname = origin
                .strip_prefix("https://")
                .ok_or(BrowserServiceError::Invalid)?;
            let resolved = tokio::time::timeout(
                Duration::from_secs(5),
                tokio::net::lookup_host((hostname, 443)),
            )
            .await
            .map_err(|_| BrowserServiceError::Unavailable)?
            .map_err(|_| BrowserServiceError::Unavailable)?;
            let mut addresses = resolved.take(33).collect::<Vec<_>>();
            if addresses.is_empty()
                || addresses.len() > 32
                || addresses.iter().any(|address| match address.ip() {
                    std::net::IpAddr::V4(v4) => public_ipv4_pin(&v4.to_string()).is_err(),
                    std::net::IpAddr::V6(v6) => {
                        v6.is_loopback()
                            || v6.is_unspecified()
                            || v6.is_unique_local()
                            || v6.is_unicast_link_local()
                            || v6.to_ipv4_mapped().is_some()
                            || v6.is_multicast()
                    }
                })
            {
                return Err(BrowserServiceError::Invalid);
            }
            addresses.sort();
            let pin = addresses
                .iter()
                .find_map(|address| match address.ip() {
                    std::net::IpAddr::V4(v4) => Some(v4.to_string()),
                    _ => None,
                })
                .ok_or(BrowserServiceError::Unavailable)?;
            pins.push(BrowserOrigin {
                origin,
                public_ipv4_pin: pin,
            });
        }
        self.publish_destination(agent, &input, start, pins).await
    }

    /// Publish only after DNS pinning. The shared profile gate prevents a new
    /// authorization from overtaking revocation's credential and worker cleanup.
    async fn publish_destination(
        &self,
        agent: &str,
        input: &CreateBrowserDestination,
        start: Url,
        pins: Vec<BrowserOrigin>,
    ) -> Result<BrowserPolicyView, BrowserServiceError> {
        let (expected_start, origins) = destination_urls(input)?;
        if start != expected_start
            || pins.iter().map(|pin| &pin.origin).collect::<Vec<_>>()
                != origins.iter().collect::<Vec<_>>()
        {
            return Err(BrowserServiceError::Invalid);
        }
        let id = destination_id(agent, &start, &origins, input.authenticated)?;
        let origin = start.origin().ascii_serialization();
        let path = format!(
            "{}{}",
            start.path(),
            start.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        let primary_pin = pins
            .iter()
            .find(|pin| pin.origin == origin)
            .ok_or(BrowserServiceError::Invalid)?
            .public_ipv4_pin
            .clone();
        let _profiles = self.inner.profile_mutations.lock().await;
        {
            let _transaction = self.inner.transactions.lock().await;
            Self::owner(&*self.state()?.read().await, agent)?;
            let mut policies = self
                .inner
                .destinations
                .lock()
                .map_err(|_| BrowserServiceError::Unavailable)?;
            if policies.contains_key(&(agent.into(), id.clone())) {
                return Err(BrowserServiceError::Conflict);
            }
            if policies.len() >= 64 {
                return Err(BrowserServiceError::Busy);
            }
            let previous = self
                .inner
                .destination_sequence
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |version| {
                    version
                        .checked_add(1)
                        .filter(|next| *next <= MAX_BROWSER_SAFE_INTEGER)
                })
                .map_err(|_| BrowserServiceError::Conflict)?;
            let template = BrowserScope {
                id: "owner_destination".into(),
                revision: previous + 1,
                origin: origin.clone(),
                public_ipv4_pin: Some(primary_pin),
                authenticated: input.authenticated,
                interactive: Some(BrowserInteractive {
                    origins: pins,
                    resource_bytes: 4 * 1024 * 1024,
                    network_bytes: 32 * 1024 * 1024,
                }),
                routes: vec![BrowserRoute {
                    id: "start".into(),
                    method: BrowserMethod::Get,
                    path,
                }],
                input_ids: vec![],
                limits: BrowserLimits {
                    actions: 40,
                    requests: 80,
                    scope_ms: 120000,
                    frame_bytes: 65536,
                    wire_bytes: 1048576,
                    observation_nodes: 128,
                    observation_text_bytes: 8192,
                    input_bytes: 16384,
                    output_bytes: 4 * 1024 * 1024,
                    screenshot_bytes: 262144,
                },
            };
            anima_service_adapters::browser::validate_scope(&template)
                .map_err(|_| BrowserServiceError::Invalid)?;
            policies.insert(
                (agent.into(), id.clone()),
                InstalledBrowserPolicy {
                    id: id.clone(),
                    template,
                },
            );
        }
        self.policies(agent)
            .await?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(BrowserServiceError::Unavailable)
    }

    pub(crate) async fn revoke_destination(
        &self,
        agent: &str,
        id: &str,
        version: u64,
    ) -> Result<(), BrowserServiceError> {
        let service = self.clone();
        let (agent, id) = (agent.to_owned(), id.to_owned());
        // Once revocation starts, caller cancellation cannot abandon another
        // profile or physical owner half way through the cleanup sequence.
        tokio::spawn(async move { service.revoke_destination_owned(&agent, &id, version).await })
            .await
            .map_err(|_| BrowserServiceError::Unavailable)?
    }

    async fn revoke_destination_owned(
        &self,
        agent: &str,
        id: &str,
        version: u64,
    ) -> Result<(), BrowserServiceError> {
        // Sign-in startup holds this gate until its opened owner is visible.
        // Waiting here joins that startup before enumerating any cleanup work.
        let _profiles = self.inner.profile_mutations.lock().await;
        let workspace = self.profile_workspace(agent).await?;
        let policy = self
            .inner
            .destinations
            .lock()
            .map_err(|_| BrowserServiceError::Unavailable)?
            .get(&(agent.into(), id.into()))
            .cloned()
            .ok_or(BrowserServiceError::NotFound)?;
        if version != policy.template.revision {
            return Err(BrowserServiceError::Conflict);
        }
        let mut failed = false;
        let mut profiles = if policy.template.authenticated {
            let store = self.inner.profile_store.clone();
            let root = workspace.clone();
            let agent = agent.to_owned();
            match tokio::task::spawn_blocking(move || store.list(&root, &agent)).await {
                Ok(Ok(profiles)) => profiles
                    .into_iter()
                    .filter(|profile| profile.policy_id == id)
                    .map(|profile| (profile.id.clone(), profile))
                    .collect::<BTreeMap<_, _>>(),
                _ => {
                    failed = true;
                    BTreeMap::new()
                }
            }
        } else {
            BTreeMap::new()
        };
        for profile in self
            .inner
            .profile_versions
            .lock()
            .map_err(|_| BrowserServiceError::Unavailable)?
            .values()
            .filter(|profile| profile.agent_id == agent && profile.policy_id == id)
        {
            profiles
                .entry(profile.id.clone())
                .or_insert_with(|| profile.clone());
        }
        for setup in self
            .inner
            .profile_setups
            .lock()
            .map_err(|_| BrowserServiceError::Unavailable)?
            .values()
            .filter(|setup| setup.view.agent_id == agent && setup.view.policy_id == id)
        {
            profiles
                .entry(setup.view.id.clone())
                .or_insert_with(|| setup.view.clone());
        }
        let runs = {
            let _transaction = self.inner.transactions.lock().await;
            Self::owner(&*self.state()?.read().await, agent)?;
            let mut policies = self
                .inner
                .destinations
                .lock()
                .map_err(|_| BrowserServiceError::Unavailable)?;
            if policies
                .get(&(agent.into(), id.into()))
                .is_none_or(|current| current.template.revision != version)
            {
                return Err(BrowserServiceError::Conflict);
            }
            policies.remove(&(agent.into(), id.into()));
            drop(policies);
            let mut scopes = self
                .inner
                .scopes
                .lock()
                .map_err(|_| BrowserServiceError::Unavailable)?;
            let runs = scopes
                .iter_mut()
                .filter(|(_, row)| row.view.agent_id == agent && row.view.policy_id == id)
                .map(|(run, row)| {
                    if let Some(profile) = &row.profile {
                        profiles
                            .entry(profile.id.clone())
                            .or_insert_with(|| profile.clone());
                    }
                    Self::close_record(row, Some("Browser destination revoked"));
                    run.clone()
                })
                .collect::<Vec<_>>();
            for profile in profiles.values() {
                let mut fence = profile.clone();
                fence.revoked = true;
                fence.ready = false;
                fence.revision = fence.revision.saturating_add(1);
                self.remember_profile(&fence);
            }
            runs
        };
        // Persist every profile revocation before waiting for closure; failure
        // still attempts all remaining physical cleanup and never restores the grant.
        for profile in profiles.values() {
            let store = self.inner.profile_store.clone();
            let root = workspace.clone();
            let profile = profile.clone();
            match tokio::task::spawn_blocking(move || store.revoke(&root, &profile)).await {
                Ok(Ok(revoked)) => self.remember_profile(&revoked),
                _ => failed = true,
            }
        }
        for run in runs {
            self.retire_run(&run).await;
        }
        for profile in profiles.values() {
            if !self.retire_profile(&profile.id).await {
                failed = true;
            }
            let store = self.inner.profile_store.clone();
            let root = workspace.clone();
            let id = profile.id.clone();
            if !matches!(
                tokio::task::spawn_blocking(move || store.purge_sealed(&root, &id)).await,
                Ok(Ok(()))
            ) {
                failed = true;
            }
        }
        if self
            .inner
            .scopes
            .lock()
            .map_err(|_| BrowserServiceError::Unavailable)?
            .values()
            .any(|row| {
                row.view.agent_id == agent
                    && row.view.policy_id == id
                    && row.view.physical_lease_held
            })
        {
            failed = true;
        }
        if failed {
            Err(BrowserServiceError::Unavailable)
        } else {
            Ok(())
        }
    }

    pub(crate) async fn owner_control(
        &self,
        agent: &str,
        run: &str,
        version: u64,
        enabled: bool,
    ) -> Result<BrowserScopeView, BrowserServiceError> {
        Self::owner(&*self.state()?.read().await, agent)?;
        let (action, task, cancel, deadline) = {
            let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            let row = scopes
                .get_mut(run)
                .filter(|r| r.view.agent_id == agent)
                .ok_or(BrowserServiceError::NotFound)?;
            Self::expire(row);
            if row.view.version != version
                || !row.view.enabled
                || row._scope.interactive.is_none()
                || row.view.owner_control == enabled
            {
                return Err(BrowserServiceError::Conflict);
            }
            (
                row.action.clone(),
                row.task.clone().ok_or(BrowserServiceError::Conflict)?,
                row.cancel.clone(),
                row.deadline,
            )
        };
        // An effect already in flight must settle its receipt. The owner can
        // inspect status and retry control after that boundary; effects are never retried.
        let _action = action
            .try_lock_owned()
            .map_err(|_| BrowserServiceError::Busy)?;
        {
            let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
            let row = scopes.get_mut(run).ok_or(BrowserServiceError::NotFound)?;
            if row.view.version != version || row.cancel.is_cancelled() {
                return Err(BrowserServiceError::Conflict);
            }
            row.view.version += 1;
            row.view.owner_control = true; // Fence both transitions until acknowledged.
            row.view.state = BrowserScopeState::OwnerControl;
            row.view.document = None;
        }
        let result = task
            .handle
            .execute_admitted(
                BrowserOperation::OwnerControl { enabled },
                None,
                BrowserControl { cancel, deadline },
            )
            .await;
        if result.is_err() {
            self.retire_run(run).await;
            return Err(BrowserServiceError::Unavailable);
        }
        let mut scopes = self.inner.scopes.lock().unwrap_or_else(|p| p.into_inner());
        let row = scopes.get_mut(run).ok_or(BrowserServiceError::NotFound)?;
        if row.view.version != version + 1 || row.cancel.is_cancelled() {
            return Err(BrowserServiceError::Conflict);
        }
        Self::merge_known_counters(row, &task.handle.snapshot());
        row.view.owner_control = enabled;
        row.view.state = if enabled {
            BrowserScopeState::OwnerControl
        } else {
            BrowserScopeState::Active
        };
        self.inner.control_changed.notify_waiters();
        Ok(row.view.clone())
    }
}

#[cfg(test)]
#[path = "service_destinations_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_destination_admission_refuses_ambient_authority_and_normalizes_no_origins() {
        let valid = || CreateBrowserDestination {
            start_url: "https://example.com/search?q=one".into(),
            allowed_origins: vec!["https://cdn.example.com".into()],
            authenticated: false,
        };
        assert_eq!(destination_urls(&valid()).unwrap().1.len(), 2);
        for url in [
            "http://example.com/",
            "https://owner:password@example.com/",
            "https://example.com/#token",
            "https://127.0.0.1/",
            "https://localhost/",
            "https://example.com:8443/",
        ] {
            let mut input = valid();
            input.start_url = url.into();
            assert!(destination_urls(&input).is_err(), "{url}");
        }
    }
}
