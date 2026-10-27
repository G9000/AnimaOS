use super::*;
impl CalendarManager {
    pub(crate) async fn search_documents(
        &self,
        agent: &str,
        id: &str,
        expected_generation: &str,
    ) -> Result<Vec<crate::connected_search::IndexedDocument>, CalendarError> {
        let capture = self
            .connector_for_agent(agent)
            .await
            .filter(|r| r.id == id && r.is_active() && !r.reauth_required)
            .ok_or(CalendarError::NotConnected)?;
        if capture.calendar_ids.is_empty() || capture.calendar_ids.len() > 16 {
            return Err(CalendarError::Conflict);
        }
        let now = chrono::Utc::now();
        let min = (now - chrono::Duration::days(30)).to_rfc3339();
        let max = (now + chrono::Duration::days(180)).to_rfc3339();
        let access = self.fresh_access_token(&capture).await?;
        {
            let shared = self.shared_state()?;
            let s = shared.read().await;
            if !crate::connected_search::source_authority(&s, agent, id)
                .is_some_and(|a| a.enabled && a.generation == expected_generation)
            {
                return Err(CalendarError::NotConnected);
            }
        }
        let mut docs = vec![];
        for calendar in &capture.calendar_ids {
            validate_calendar_id(calendar)?;
            docs.extend(
                match self
                    .transport
                    .search_events(&access, calendar, &min, &max)
                    .await
                    .map_err(map_transport_error)
                {
                    Ok(events) => events,
                    Err(error) => {
                        if error == CalendarError::ReauthRequired {
                            self.flag_reauth_required(id).await;
                        }
                        return Err(error);
                    }
                },
            );
            if docs.len() > 500 {
                return Err(CalendarError::Conflict);
            }
        }
        let current = self
            .connector_for_agent(agent)
            .await
            .filter(|r| {
                r.id == id
                    && r.is_active()
                    && !r.reauth_required
                    && r.authorization_generation == capture.authorization_generation
                    && r.calendar_ids == capture.calendar_ids
            })
            .ok_or(CalendarError::NotConnected)?;
        drop(current);
        Ok(docs)
    }
}
