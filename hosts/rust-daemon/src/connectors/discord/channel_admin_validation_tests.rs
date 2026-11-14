use super::*;

#[test]
fn model_integer_numbers_are_normalized_before_rest_and_validation() {
    let normalized=normalize(json!({"limit":100.0,"settings":{"rate_limit_per_user":5.0},"positions":[{"position":2.0}],"allow":"1024"})).unwrap();
    assert_eq!(normalized["limit"].as_u64(), Some(100));
    assert_eq!(
        normalized["settings"]["rate_limit_per_user"].as_u64(),
        Some(5)
    );
    assert_eq!(normalized["positions"][0]["position"].as_u64(), Some(2));
    assert_eq!(normalized["allow"].as_str(), Some("1024"));
    assert!(normalize(json!({"limit":1.5})).is_err());
    assert!(normalize(json!({"limit":9007199254740992.0})).is_err());
}

fn inventory() -> Vec<DiscordGuildChannel> {
    [
        ("22", 0),
        ("55", 4),
        ("77", 2),
        ("88", 15),
        ("89", 16),
        ("90", 5),
        ("91", 13),
    ]
    .into_iter()
    .map(|(id, kind)| DiscordGuildChannel {
        id: id.into(),
        guild_id: "33".into(),
        name: format!("channel-{id}"),
        kind,
        topic: None,
        parent_id: None,
        position: 0,
    })
    .collect()
}

#[test]
fn complete_channel_settings_validate_type_specific_payloads() {
    let channels = inventory();
    for (kind, settings) in [
        (
            "text",
            json!({"nsfw":false,"rate_limit_per_user":21600,"position":1,"default_auto_archive_duration":1440,"default_thread_rate_limit_per_user":3,"flags":2097152}),
        ),
        (
            "voice",
            json!({"bitrate":384000,"user_limit":99,"rtc_region":null,"video_quality_mode":2,"rate_limit_per_user":0}),
        ),
        (
            "stage",
            json!({"bitrate":64000,"user_limit":10000,"rtc_region":"singapore","video_quality_mode":1}),
        ),
        (
            "forum",
            json!({"available_tags":[{"name":"projects","moderated":false,"emoji_name":"💜","emoji_id":null}],"default_reaction_emoji":{"emoji_name":"💜"},"default_sort_order":null,"default_forum_layout":2,"flags":16}),
        ),
        (
            "media",
            json!({"flags":32768,"default_sort_order":1,"default_reaction_emoji":null}),
        ),
        (
            "category",
            json!({"position":0,"permission_overwrites":[{"id":"33","type":"role","allow":"1024","deny":"0"}]}),
        ),
        (
            "announcement",
            json!({"nsfw":false,"default_auto_archive_duration":1440}),
        ),
    ] {
        let prepared = prepare(
            "create_discord_channel",
            &json!({"name":"new-room","type":kind,"settings":settings}),
            "33",
            &channels,
            None,
        )
        .unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert!(prepared.mutation());
    }
}

#[test]
fn complete_channel_settings_reject_invalid_fields_before_effects() {
    let channels = inventory();
    for (kind, settings) in [
        ("stage", json!({"bitrate":64001})),
        ("voice", json!({"user_limit":100})),
        ("text", json!({"bitrate":8000})),
        ("category", json!({"nsfw":true})),
        ("text", json!({"rate_limit_per_user":21601})),
        ("text", json!({"default_auto_archive_duration":5})),
        ("text", json!({"guild_id":"999"})),
        ("text", json!({"flags":8})),
        (
            "forum",
            json!({"available_tags":[{"name":"bad","emoji_id":"999","emoji_name":"x"}]}),
        ),
        (
            "text",
            json!({"permission_overwrites":[{"id":"33","type":"role","allow":"8","deny":"8"}]}),
        ),
    ] {
        assert!(
            prepare(
                "create_discord_channel",
                &json!({"name":"new-room","type":kind,"settings":settings}),
                "33",
                &channels,
                None
            )
            .is_err(),
            "{kind}"
        );
    }
}

#[test]
fn thread_post_only_fields_require_forum_parent() {
    let channels = inventory();
    for parent_id in ["22", "88"] {
        let target =
            json!({"id":"111","guild_id":"33","type":11,"parent_id":parent_id,"name":"post"});
        for fields in [
            json!({"channel_id":"111","flags":2}),
            json!({"channel_id":"111","applied_tags":["777"]}),
        ] {
            assert_eq!(
                prepare(
                    "update_discord_thread",
                    &fields,
                    "33",
                    &channels,
                    Some(&target)
                )
                .is_ok(),
                parent_id == "88"
            );
        }
    }
}

#[test]
fn channel_acknowledgements_handle_sets_and_require_forum_starter_identity() {
    let channels = inventory();
    let target = json!({"id":"22","guild_id":"33","type":0,"name":"text"});
    let plan=prepare("update_discord_channel",&json!({"channel_id":"22","settings":{"permission_overwrites":[{"id":"33","type":"role","allow":"1024","deny":"0"},{"id":"44","type":"member","allow":"0","deny":"2048"}]}}),"33",&channels,Some(&target)).unwrap();
    let ack = json!({"id":"22","guild_id":"33","type":0,"name":"text","permission_overwrites":[{"id":"44","type":1,"allow":"0","deny":"2048"},{"id":"33","type":0,"allow":"1024","deny":"0"}]});
    validate_ack(&plan, &ack, "33", &channels).unwrap();
    let post = prepare(
        "create_discord_thread",
        &json!({"channel_id":"88","name":"post","starter_message":"Hello"}),
        "33",
        &channels,
        Some(&json!({"id":"88","guild_id":"33","type":15})),
    )
    .unwrap();
    let mut ack = json!({"id":"111","guild_id":"33","parent_id":"88","type":11,"name":"post"});
    assert!(validate_ack(&post, &ack, "33", &channels).is_err());
    ack["message"] = json!({"id":"112","channel_id":"111","content":"Hello"});
    validate_ack(&post, &ack, "33", &channels).unwrap();
    ack["message"]["channel_id"] = json!("999");
    assert!(validate_ack(&post, &ack, "33", &channels).is_err());
}
