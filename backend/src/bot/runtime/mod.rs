mod api;
mod command_exec;
mod connection;
mod discord;
mod help_image;
mod message;
mod privacy;

pub use connection::{start_bot_connections, BotRuntime, GroupSendStatus};
pub use discord::start_discord_connections;
