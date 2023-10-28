use std::string::ToString;

use lazy_static::lazy_static;
use poise::serenity_prelude::{CacheHttp, GuildId, Http, RoleId};
use strum_macros::Display;

use self::AppRole::*;

lazy_static! {
    static ref ROLE_DB: RoleDb = RoleDb {
        renamer_roles: sled::open("renamer_roles").unwrap(),
        allow_roles: sled::open("allow_roles").unwrap()
    };
}

struct RoleDb {
    renamer_roles: sled::Db,
    allow_roles: sled::Db,
}

impl RoleDb {
    fn get(&self, app_role: AppRole, key: &GuildId) -> Result<Option<String>, Error> {
        let bytes = key.0.to_ne_bytes();
        let result = self.get_db(app_role).get(bytes)?;
        let result_mapped = result.map(|val| String::from_utf8(val.to_vec()).unwrap());
        Ok(result_mapped)
    }

    fn insert(
        &self,
        app_role: AppRole,
        key: &GuildId,
        value: &str,
    ) -> Result<Option<String>, Error> {
        let key_bytes = key.0.to_ne_bytes();
        let value_bytes = value.as_bytes();
        let prev_val = self.get_db(app_role).insert(key_bytes, value_bytes)?;
        let prev_val_mapped = prev_val.map(|val| String::from_utf8(val.to_vec()).unwrap());
        Ok(prev_val_mapped)
    }

    fn get_db(&self, app_role: AppRole) -> &sled::Db {
        match app_role {
            Renamer => &self.renamer_roles,
            Allow => &self.allow_roles,
        }
    }
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Data {}

type Error = Box<dyn std::error::Error + Send + Sync>;

type Context<'a> = poise::Context<'a, Data, Error>;

macro_rules! role_by_name {
    ($guild_id:expr, $http:expr, $name:expr) => {{
        let guild_id: &GuildId = &$guild_id;
        let name_: &str = &$name;
        let http_: &Http = $http;
        guild_id
            .roles(http_)
            .await
            .unwrap()
            .values()
            .find(|role| name_ == role.name)
    }};
}

async fn check_set_up(ctx: &Context<'_>, app_role: AppRole) -> Result<Option<RoleId>, Error> {
    let guild_id = ctx.guild_id().unwrap();
    let http = ctx.http();

    let role_name = ROLE_DB.get(app_role, &guild_id)?;

    let result = if let Some(ref name) = role_name {
        if let Some(role) = role_by_name!(guild_id, http, name) {
            // match app_role {
            //     Renamer => {
            //         if role.has_permission(Permissions::MANAGE_NICKNAMES) {
            //             Ok(role.id)
            //         } else {
            //             Err(format!("{} role does not have the right permissions", app_role))
            //         }
            //     }
            //     Allow => Ok(role.id)
            // }
            Ok(role.id)
        } else {
            Err(format!("{} role does not exist in this server", app_role))
        }
    } else {
        Err(format!("{} role not known for this server", app_role))
    };

    match result {
        Ok(role_id) => Ok(Some(role_id)),
        Err(msg_text) => {
            ctx.send(|m| {
                m.ephemeral(true).content(format!(
                    "{}. Have an admin set up the app with /renamer admin set_roles.",
                    msg_text
                ))
            })
            .await?;
            Ok(None)
        }
    }
}

fn is_valid_nickname(nickname: &str) -> bool {
    // "Names can contain most valid unicode characters.
    //  We limit some zero-width and non-rendering characters."
    // TODO: Maybe eventually...

    // "Nicknames must be between 1 and 32 characters long."
    // Trims leading and trailing whitespace but does not trim internal whitespace
    if matches!(nickname.trim().len(), 0 | 33..) {
        return false;
    }

    true
}

#[poise::command(slash_command, required_bot_permissions = "MANAGE_NICKNAMES")]
pub(crate) async fn rename(
    ctx: Context<'_>,
    username: String,
    nickname: String,
) -> Result<(), Error> {
    let mut member_cow = ctx.author_member().await.ok_or::<Error>("foo".into())?;
    let member = member_cow.to_mut();
    let guild_id = ctx.guild_id().unwrap();
    let http = ctx.http();

    if let Some(renamer_role_id) = check_set_up(&ctx, Renamer).await? {
        let (msg, ephemeral) = if member
            .user
            .has_role(http, guild_id, renamer_role_id)
            .await?
        {
            if is_valid_nickname(&nickname) {
                // Get target user
                let target_members_vec = ctx
                    .guild_id()
                    .unwrap()
                    .search_members(http, &username, None)
                    .await?;

                match target_members_vec.len() {
                    0 => {
                        (format!("Search for '{}' found no users.", username), true)
                    }
                    1 => {
                        let target_member = target_members_vec.first().unwrap();
                        target_member.edit(http, |u| u
                            .nickname(&nickname)
                        ).await?;
                        (format!("{} set {}'s nickname to {}.", member.user.name, target_member.user.name, nickname), false)
                    }
                    _ => {
                        (format!("Search for '{}' found too many users. Specify exactly one user for `username`.", username), true)
                    }
                }
            } else {
                (format!("{} is not a valid nickname.", nickname), true)
            }
        } else {
            (
                "You do not have permission to use this command.".into(),
                true,
            )
        };
        ctx.send(|m| m.ephemeral(ephemeral).content(msg)).await?;
    }

    Ok(())
}

#[poise::command(slash_command, subcommands("help", "allow", "disallow", "admin"))]
pub(crate) async fn renamer(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"] command: Option<String>,
) -> Result<(), Error> {
    let extra_text = format!(
        "\
renamer version {}

Type /renamer help <command> for more info on a command.
You can edit your message to the bot and the bot will edit its response.",
        VERSION
    );
    let config = poise::builtins::HelpConfiguration {
        extra_text_at_bottom: &extra_text,
        ..Default::default()
    };
    poise::builtins::help(ctx, command.as_deref(), config).await?;
    Ok(())
}

#[poise::command(slash_command, required_bot_permissions = "MANAGE_ROLES")]
async fn allow(ctx: Context<'_>) -> Result<(), Error> {
    let mut member_cow = ctx.author_member().await.ok_or::<Error>("foo".into())?;
    let member = member_cow.to_mut();
    let guild_id = ctx.guild_id().unwrap();
    let http = ctx.http();

    if let Some(allow_role_id) = check_set_up(&ctx, Allow).await? {
        let msg = if !member.user.has_role(http, guild_id, allow_role_id).await? {
            member.add_role(http, allow_role_id).await?;
            "Successfully allowed nickname changes."
        } else {
            "You are already allowing nickname changes."
        };
        ctx.send(|m| m.ephemeral(true).content(msg)).await?;
    }

    Ok(())
}

#[poise::command(slash_command, required_bot_permissions = "MANAGE_ROLES")]
async fn disallow(ctx: Context<'_>) -> Result<(), Error> {
    let mut member_cow = ctx.author_member().await.ok_or::<Error>("foo".into())?;
    let member = member_cow.to_mut();
    let guild_id = ctx.guild_id().unwrap();
    let http = ctx.http();

    if let Some(allow_role_id) = check_set_up(&ctx, Allow).await? {
        let msg = if member.user.has_role(http, guild_id, allow_role_id).await? {
            member.remove_role(http, allow_role_id).await?;
            "Successfully disallowed nickname changes."
        } else {
            "You are already disallowing nickname changes."
        };
        ctx.send(|m| m.ephemeral(true).content(msg)).await?;
    }

    Ok(())
}

#[poise::command(
    slash_command,
    required_permissions = "ADMINISTRATOR",
    subcommands("set_roles")
)]
async fn admin(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[derive(Display, Clone, Copy)]
enum AppRole {
    Renamer,
    Allow,
}

async fn set_role(app_role: AppRole, ctx: &Context<'_>, role_name: &str) -> Result<String, Error> {
    let guild_id = ctx.guild_id().unwrap();
    let http = ctx.http();

    // Role name DB operations
    let db_msg = match ROLE_DB.get(app_role, &guild_id)? {
        Some(stored_role) if stored_role == role_name => {
            format!(
                "{} role is already set to {}; no change made.",
                app_role, role_name
            )
        }
        _ => {
            if let Some(previous_role) = ROLE_DB.insert(app_role, &guild_id, role_name)? {
                format!(
                    "{} role was changed from {} to {}.",
                    app_role, previous_role, role_name
                )
            } else {
                format!("{} role was set to {}.", app_role, role_name)
            }
        }
    };

    // Check for existing role in server; create new one if absent
    let (role_set_msg, role_id) = match role_by_name!(guild_id, http, role_name) {
        Some(role) => (
            format!("Using existing server role {}.", role_name),
            role.id,
        ),
        None => {
            let new_role_id = guild_id
                .create_role(http, |r| r.name(&role_name).mentionable(false))
                .await?
                .id;
            (
                format!("Created new server role {}.", role_name),
                new_role_id,
            )
        }
    };

    // // Set visibility of /rename command for renamer role
    // if matches!(app_role, Renamer) {
    //     guild_id.edit_role(
    //         http,
    //         role_id,
    //         |r| r
    //             .hoist(true)
    //             .permissions(Permissions::MANAGE_NICKNAMES)
    //     ).await?;
    // }

    // Compose message
    let msg = format!("{}\n{}", db_msg, role_set_msg);

    Ok(msg)
}

#[poise::command(slash_command, required_bot_permissions = "MANAGE_ROLES")]
async fn set_roles(
    ctx: Context<'_>,
    renamer_role: String,
    allow_role: String,
) -> Result<(), Error> {
    let renamer_msg = set_role(Renamer, &ctx, &renamer_role).await?;
    let allow_msg = set_role(Allow, &ctx, &allow_role).await?;

    ctx.send(|m| {
        m.ephemeral(true).embed(|e| {
            e.title("set_roles")
                .field("Renamer role", renamer_msg, false)
                .field("Allow role", allow_msg, false)
        })
    })
    .await?;

    Ok(())
}
