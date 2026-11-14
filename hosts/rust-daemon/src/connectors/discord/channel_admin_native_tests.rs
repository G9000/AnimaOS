use super::*;
use crate::connectors::discord::channel_admin_model::{
    DiscordChannelAdminMethod as M, DiscordChannelAdminRequest, DiscordChannelAdminRoute as R,
};

fn request(method: M, route: R, body: Option<serde_json::Value>) -> DiscordChannelAdminRequest {
    DiscordChannelAdminRequest {
        method,
        route,
        body,
    }
}

#[tokio::test]
async fn native_channel_admin_legacy_edit_supports_forum_topics_and_other_guild_kinds() {
    for kind in [0, 2, 4, 5, 13, 15, 16] {
        let mut channel = guild_channel("42", kind);
        channel["name"] = "new-home".into();
        let topic = if matches!(kind, 15 | 16) {
            Some("a".repeat(4096))
        } else {
            None
        };
        if let Some(topic) = &topic {
            channel["topic"] = topic.clone().into();
        }
        let (client, capture, task) = fixture(vec![json(channel)]).await;
        let token = DiscordBotToken::parse("channel-admin-legacy-fixture").unwrap();
        let edited = client
            .edit_guild_channel(
                &token,
                "7",
                "42",
                &DiscordChannelEdit {
                    name: Some("new-home".into()),
                    topic,
                    parent_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(edited.kind, kind);
        assert_eq!(capture.methods.lock().unwrap().as_slice(), ["PATCH"]);
        task.abort();
    }
}

#[tokio::test]
async fn native_channel_admin_legacy_edit_rejects_oversized_topics_without_effects() {
    let (client, capture, task) = fixture(vec![]).await;
    let token = DiscordBotToken::parse("channel-admin-legacy-limit-fixture").unwrap();
    let result = client
        .edit_guild_channel(
            &token,
            "7",
            "42",
            &DiscordChannelEdit {
                name: None,
                topic: Some("a".repeat(4097)),
                parent_id: None,
            },
        )
        .await;
    assert_eq!(result.unwrap_err(), DiscordTransportError::InvalidInput);
    assert!(capture.calls.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn native_channel_admin_explicit_routes_methods_and_empty_acknowledgements() {
    let examples = [
        (
            request(
                M::Get,
                R::Channel {
                    channel_id: "42".into(),
                },
                None,
            ),
            "GET",
            "/channels/42",
            false,
        ),
        (
            request(
                M::Delete,
                R::Channel {
                    channel_id: "42".into(),
                },
                None,
            ),
            "DELETE",
            "/channels/42",
            false,
        ),
        (
            request(
                M::Patch,
                R::Channel {
                    channel_id: "42".into(),
                },
                Some(serde_json::json!({"name":"home"})),
            ),
            "PATCH",
            "/channels/42",
            false,
        ),
        (
            request(
                M::Post,
                R::GuildChannels {
                    guild_id: "7".into(),
                },
                Some(serde_json::json!({"name":"home","type":15})),
            ),
            "POST",
            "/guilds/7/channels",
            false,
        ),
        (
            request(
                M::Patch,
                R::GuildPositions {
                    guild_id: "7".into(),
                },
                Some(serde_json::json!([{"id":"42","position":1}])),
            ),
            "PATCH",
            "/guilds/7/channels",
            true,
        ),
        (
            request(
                M::Put,
                R::Permission {
                    channel_id: "42".into(),
                    overwrite_id: "99".into(),
                },
                Some(serde_json::json!({"type":0,"allow":"1024","deny":"0"})),
            ),
            "PUT",
            "/channels/42/permissions/99",
            true,
        ),
        (
            request(
                M::Delete,
                R::Permission {
                    channel_id: "42".into(),
                    overwrite_id: "99".into(),
                },
                None,
            ),
            "DELETE",
            "/channels/42/permissions/99",
            true,
        ),
        (
            request(
                M::Get,
                R::MemberPermissions {
                    channel_id: "42".into(),
                    user_id: "99".into(),
                },
                None,
            ),
            "GET",
            "/channels/42/permissions/member/99",
            false,
        ),
        (
            request(
                M::Get,
                R::RolePermissions {
                    channel_id: "42".into(),
                    role_id: "99".into(),
                },
                None,
            ),
            "GET",
            "/channels/42/permissions/role/99",
            false,
        ),
        (
            request(
                M::Post,
                R::ThreadStart {
                    channel_id: "42".into(),
                    message_id: Some("77".into()),
                },
                Some(serde_json::json!({"name":"planning"})),
            ),
            "POST",
            "/channels/42/messages/77/threads",
            false,
        ),
        (
            request(
                M::Post,
                R::ThreadStart {
                    channel_id: "42".into(),
                    message_id: None,
                },
                Some(serde_json::json!({"name":"planning","type":11})),
            ),
            "POST",
            "/channels/42/threads",
            false,
        ),
        (
            request(
                M::Get,
                R::GuildThreads {
                    guild_id: "7".into(),
                },
                None,
            ),
            "GET",
            "/guilds/7/threads/active",
            false,
        ),
        (
            request(
                M::Get,
                R::ArchivedThreads {
                    channel_id: "42".into(),
                    kind: "private".into(),
                    before: None,
                    limit: 25,
                },
                None,
            ),
            "GET",
            "/channels/42/threads/archived/private?limit=25",
            false,
        ),
        (
            request(
                M::Get,
                R::ArchivedThreads {
                    channel_id: "42".into(),
                    kind: "public".into(),
                    before: Some("2026-11-14T12:00:00Z".into()),
                    limit: 25,
                },
                None,
            ),
            "GET",
            "/channels/42/threads/archived/public?limit=25&before=2026-11-14T12%3A00%3A00Z",
            false,
        ),
        (
            request(
                M::Get,
                R::ArchivedThreads {
                    channel_id: "42".into(),
                    kind: "joined_private".into(),
                    before: Some("77".into()),
                    limit: 25,
                },
                None,
            ),
            "GET",
            "/channels/42/users/@me/threads/archived/private?limit=25&before=77",
            false,
        ),
        (
            request(
                M::Get,
                R::ThreadMembers {
                    channel_id: "42".into(),
                    user_id: None,
                    after: Some("77".into()),
                    limit: Some(25),
                },
                None,
            ),
            "GET",
            "/channels/42/thread-members?with_member=true&limit=25&after=77",
            false,
        ),
        (
            request(
                M::Get,
                R::ThreadMembers {
                    channel_id: "42".into(),
                    user_id: Some("99".into()),
                    after: None,
                    limit: None,
                },
                None,
            ),
            "GET",
            "/channels/42/thread-members/99?with_member=true",
            false,
        ),
        (
            request(
                M::Put,
                R::ThreadMembers {
                    channel_id: "42".into(),
                    user_id: Some("@me".into()),
                    after: None,
                    limit: None,
                },
                None,
            ),
            "PUT",
            "/channels/42/thread-members/@me",
            true,
        ),
        (
            request(
                M::Delete,
                R::ThreadMembers {
                    channel_id: "42".into(),
                    user_id: Some("99".into()),
                    after: None,
                    limit: None,
                },
                None,
            ),
            "DELETE",
            "/channels/42/thread-members/99",
            true,
        ),
        (
            request(
                M::Put,
                R::VoiceStatus {
                    channel_id: "42".into(),
                },
                Some(serde_json::json!({"status":"Here"})),
            ),
            "PUT",
            "/channels/42/voice-status",
            true,
        ),
        (
            request(
                M::Post,
                R::Follow {
                    channel_id: "42".into(),
                },
                Some(serde_json::json!({"webhook_channel_id":"77"})),
            ),
            "POST",
            "/channels/42/followers",
            false,
        ),
    ];
    for (request, method, path, empty) in examples {
        let response = if empty {
            (204, String::new(), vec![])
        } else {
            json(serde_json::json!({"ok":true}))
        };
        let (client, capture, task) = fixture(vec![response]).await;
        let token = DiscordBotToken::parse("channel-admin-route-fixture").unwrap();
        let result = client.channel_admin(&token, &request).await.unwrap();
        assert_eq!(result.is_null(), empty);
        assert_eq!(capture.methods.lock().unwrap().as_slice(), [method]);
        assert_eq!(
            capture.calls.lock().unwrap()[0].0,
            format!("/api/v10{path}")
        );
        task.abort();
    }
}

#[tokio::test]
async fn native_channel_admin_rejects_path_injection_bad_methods_and_query_bounds() {
    let (client, capture, task) = fixture(vec![]).await;
    let token = DiscordBotToken::parse("channel-admin-validation-fixture").unwrap();
    for request in [
        request(
            M::Get,
            R::Channel {
                channel_id: "7/evil".into(),
            },
            None,
        ),
        request(
            M::Post,
            R::Channel {
                channel_id: "42".into(),
            },
            Some(serde_json::json!({})),
        ),
        request(
            M::Get,
            R::Permission {
                channel_id: "42".into(),
                overwrite_id: "99".into(),
            },
            None,
        ),
        request(
            M::Get,
            R::ArchivedThreads {
                channel_id: "42".into(),
                kind: "../private".into(),
                before: None,
                limit: 25,
            },
            None,
        ),
        request(
            M::Get,
            R::ArchivedThreads {
                channel_id: "42".into(),
                kind: "public".into(),
                before: Some("now&limit=10000".into()),
                limit: 25,
            },
            None,
        ),
        request(
            M::Get,
            R::ArchivedThreads {
                channel_id: "42".into(),
                kind: "private".into(),
                before: None,
                limit: 101,
            },
            None,
        ),
        request(
            M::Get,
            R::ThreadMembers {
                channel_id: "42".into(),
                user_id: None,
                after: Some("0?bad=1".into()),
                limit: Some(25),
            },
            None,
        ),
        request(
            M::Put,
            R::ThreadMembers {
                channel_id: "42".into(),
                user_id: None,
                after: None,
                limit: None,
            },
            None,
        ),
    ] {
        assert_eq!(
            client.channel_admin(&token, &request).await.unwrap_err(),
            DiscordTransportError::InvalidInput
        );
    }
    assert!(capture.calls.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn native_channel_admin_starters_force_no_mentions() {
    let (client, capture, task) = fixture(vec![json(serde_json::json!({"id":"77"}))]).await;
    let token = DiscordBotToken::parse("channel-admin-mention-fixture").unwrap();
    let starter = "@everyone <@99> <@&88>";
    client.channel_admin(&token, &request(M::Post, R::ThreadStart { channel_id:"42".into(), message_id:None }, Some(serde_json::json!({
        "name":"planning", "message":{"content":starter,"allowed_mentions":{"parse":["everyone","users","roles"],"users":["99"],"roles":["88"],"replied_user":true}}
    })))).await.unwrap();
    let calls = capture.calls.lock().unwrap();
    assert_eq!(calls[0].2["message"]["content"], starter);
    assert_eq!(
        calls[0].2["message"]["allowed_mentions"],
        serde_json::json!({"parse":[],"replied_user":false})
    );
    task.abort();
}

#[tokio::test]
async fn native_channel_admin_put_delete_failures_are_uncertain_without_retry() {
    for request in [
        request(
            M::Put,
            R::VoiceStatus {
                channel_id: "42".into(),
            },
            Some(serde_json::json!({"status":"Here"})),
        ),
        request(
            M::Delete,
            R::Channel {
                channel_id: "42".into(),
            },
            None,
        ),
    ] {
        let (client, capture, task) =
            fixture(vec![(502, "provider unavailable".into(), vec![])]).await;
        let token = DiscordBotToken::parse("channel-admin-uncertainty-fixture").unwrap();
        assert_eq!(
            client.channel_admin(&token, &request).await.unwrap_err(),
            DiscordTransportError::Uncertain
        );
        assert_eq!(capture.calls.lock().unwrap().len(), 1);
        task.abort();
    }
}

#[tokio::test]
async fn native_channel_admin_empty_effects_require_actual_empty_204() {
    let (client, capture, task) = fixture(vec![(200, "null".into(), vec![])]).await;
    let token = DiscordBotToken::parse("channel-admin-empty-ack-fixture").unwrap();
    let request = request(
        M::Put,
        R::VoiceStatus {
            channel_id: "42".into(),
        },
        Some(serde_json::json!({"status":null})),
    );
    assert_eq!(
        client.channel_admin(&token, &request).await.unwrap_err(),
        DiscordTransportError::Uncertain
    );
    assert_eq!(capture.calls.lock().unwrap().len(), 1);
    task.abort();
}
