//! Host-selected, bounded routes for owner-only channel administration.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiscordChannelAdminMethod {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiscordChannelAdminRoute {
    Channel {
        channel_id: String,
    },
    GuildChannels {
        guild_id: String,
    },
    GuildPositions {
        guild_id: String,
    },
    Permission {
        channel_id: String,
        overwrite_id: String,
    },
    MemberPermissions {
        channel_id: String,
        user_id: String,
    },
    RolePermissions {
        channel_id: String,
        role_id: String,
    },
    ThreadStart {
        channel_id: String,
        message_id: Option<String>,
    },
    GuildThreads {
        guild_id: String,
    },
    ArchivedThreads {
        channel_id: String,
        kind: String,
        before: Option<String>,
        limit: u16,
    },
    ThreadMembers {
        channel_id: String,
        user_id: Option<String>,
        after: Option<String>,
        limit: Option<u16>,
    },
    VoiceStatus {
        channel_id: String,
    },
    Follow {
        channel_id: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct DiscordChannelAdminRequest {
    pub(crate) method: DiscordChannelAdminMethod,
    pub(crate) route: DiscordChannelAdminRoute,
    pub(crate) body: Option<serde_json::Value>,
}
