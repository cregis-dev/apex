//! The `/admin/*` surface: CRUD over the runtime config, one module per
//! entity. All writes funnel through `super::config_store::commit_config`.

mod api_keys;
mod channels;
mod routers;
mod teams;

pub(super) use api_keys::{
    handle_admin_channels_api_keys, handle_admin_team_reveal_api_key, handle_admin_teams_api_keys,
};
pub(super) use channels::{
    handle_admin_channels, handle_admin_create_channel, handle_admin_delete_channel,
    handle_admin_update_channel,
};
pub(super) use routers::{
    handle_admin_create_router, handle_admin_delete_router, handle_admin_routers,
    handle_admin_update_router,
};
pub(super) use teams::{
    handle_admin_create_team, handle_admin_delete_team, handle_admin_teams,
    handle_admin_update_team,
};
