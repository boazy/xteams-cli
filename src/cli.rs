//! Command-line surface (clap derive). Two-tier `{noun} {verb}` layout, plus the
//! simple top-level `auth`. A global `-j/--json` switches every command to JSON.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "xteams",
    author,
    version,
    about = "Unofficial Microsoft Teams CLI (uses the local desktop app's credentials)",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Override the Teams `Cookies` DB path (defaults to the signed-in work profile).
    #[arg(long, global = true)]
    pub cookies: Option<PathBuf>,

    /// Emit machine-readable JSON instead of human-readable output.
    #[arg(short = 'j', long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,

    /// Extra pandoc arguments collected from `--pandoc-<option>` flags (populated
    /// in `main` before clap parsing; not a user-facing clap argument).
    #[arg(skip)]
    pub pandoc_args: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Account/token status, device-code sign-in, and sign-out.
    Auth {
        #[command(subcommand)]
        verb: AuthVerb,
    },
    /// Chats (1:1 and group conversations).
    Chat {
        #[command(subcommand)]
        verb: ChatVerb,
    },
    /// Teams you belong to.
    Team {
        #[command(subcommand)]
        verb: TeamVerb,
    },
    /// Channels within teams.
    Channel {
        #[command(subcommand)]
        verb: ChannelVerb,
    },
    /// Threads (a root message and its replies).
    Thread {
        #[command(subcommand)]
        verb: ThreadVerb,
    },
    /// Messages.
    Message {
        #[command(subcommand)]
        verb: MessageVerb,
    },
    /// People / users.
    User {
        #[command(subcommand)]
        verb: UserVerb,
    },
    /// Your calendar (Microsoft Graph).
    Calendar {
        #[command(subcommand)]
        verb: CalendarVerb,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuthVerb {
    /// Show the signed-in account and status of every token (audience + expiry).
    Status(AuthStatusArgs),
    /// Sign in via device code (unlocks team, user, and calendar).
    Login,
    /// Remove the stored device-code sign-in.
    Logout,
    /// Seed another CLI's credential store from xteams' tokens.
    Seed {
        #[command(subcommand)]
        target: SeedTarget,
    },
}

#[derive(Debug, clap::Args)]
pub struct AuthStatusArgs {
    /// Include the actual secret token values in the output.
    #[arg(long)]
    pub include_tokens: bool,
}

#[derive(Debug, Subcommand)]
pub enum SeedTarget {
    /// Seed the m365 CLI (pnp/cli-microsoft365) so it can call Microsoft Graph.
    M365(SeedM365Args),
}

#[derive(Debug, clap::Args)]
pub struct SeedM365Args {
    /// What to seed: a refresh token (default; m365 self-renews) or a 1-hour access token.
    #[arg(short = 't', long, default_value = "refresh")]
    pub token_type: TokenType,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum TokenType {
    /// Inject a refresh token into m365's MSAL cache; m365 renews silently for ~90 days.
    Refresh,
    /// Inject only a ~1-hour Graph access token; re-run before it expires.
    Access,
}

#[derive(Debug, Subcommand)]
pub enum ChatVerb {
    /// List your recent chats/conversations.
    List(ChatListArgs),
}

#[derive(Debug, Subcommand)]
pub enum TeamVerb {
    /// List the teams you belong to.
    List,
    /// Join a team by id.
    Join(TeamRefArgs),
    /// Search teams by name.
    Search(QueryArgs),
}

#[derive(Debug, Subcommand)]
pub enum ChannelVerb {
    /// List channels (optionally within a given team).
    List(ChannelListArgs),
    /// Search channels by name.
    Search(QueryArgs),
}

#[derive(Debug, Subcommand)]
pub enum ThreadVerb {
    /// List threads in a conversation (the top-level message of each).
    List(ThreadListArgs),
    /// Read one thread (root + all replies) in chronological order.
    Read(ThreadReadArgs),
}

#[derive(Debug, Subcommand)]
pub enum MessageVerb {
    /// Post a new message (top-level, or --reply-to a thread root).
    New(MessageNewArgs),
    /// List the last N messages in a conversation/channel.
    List(MessageListArgs),
    /// Read a single message by id.
    Read(MessageRefArgs),
    /// Edit a message you sent.
    Edit(MessageEditArgs),
    /// React to a message with an emoji.
    React(MessageReactArgs),
}

#[derive(Debug, Subcommand)]
pub enum UserVerb {
    /// Search people by name or email.
    Search(QueryArgs),
}

#[derive(Debug, Subcommand)]
pub enum CalendarVerb {
    /// List upcoming events.
    Upcoming(CalendarUpcomingArgs),
}

#[derive(Debug, clap::Args)]
pub struct ChatListArgs {
    /// Number of recent chats to list.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: u32,
}

#[derive(Debug, clap::Args)]
pub struct ChannelListArgs {
    /// Optional team id (or name fragment) to scope the listing.
    pub team: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct TeamRefArgs {
    /// Team id.
    pub team: String,
}

#[derive(Debug, clap::Args)]
pub struct QueryArgs {
    /// Search query.
    pub query: String,
}

#[derive(Debug, clap::Args)]
pub struct CalendarUpcomingArgs {
    /// Number of days ahead to include.
    #[arg(short = 'd', long, default_value_t = 7)]
    pub days: i64,
}

#[derive(Debug, clap::Args)]
pub struct ThreadListArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Number of most-recent threads to list.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: u32,
    /// Include all replies for each thread (not just the top-level message).
    #[arg(short = 'a', long)]
    pub all_replies: bool,
    #[command(flatten)]
    pub content: ContentOutputArgs,
}

#[derive(Debug, clap::Args)]
pub struct ThreadReadArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Root (top-level) message id (optional if a message link supplies it).
    pub message: Option<String>,
    #[command(flatten)]
    pub content: ContentOutputArgs,
}

/// Content options for commands that write a message body (`message new/edit`).
/// The body comes from `--content`, or from stdin when it is omitted.
#[derive(Debug, clap::Args)]
pub struct ContentInputArgs {
    /// Message content. If omitted, the content is read from stdin.
    #[arg(long)]
    pub content: Option<String>,
    /// Input format for --content: markdown (default), plain, html, or
    /// pandoc:<fmt>.
    #[arg(short = 'I', long, value_name = "FORMAT", conflicts_with = "content_format")]
    pub content_input_format: Option<String>,
    /// Set both input and output content format at once (mutually exclusive with
    /// -I/-O).
    #[arg(short = 'f', long, value_name = "FORMAT")]
    pub content_format: Option<String>,
}

/// Content options for commands that render a message body (`message read/list`,
/// `thread read/list`).
#[derive(Debug, clap::Args)]
pub struct ContentOutputArgs {
    /// Output format for message content: markdown (default in text mode; JSON
    /// defaults to keep), plain, html, keep, or pandoc:<fmt>.
    #[arg(short = 'O', long, value_name = "FORMAT", conflicts_with = "content_format")]
    pub content_output_format: Option<String>,
    /// Set both input and output content format at once (mutually exclusive with
    /// -I/-O).
    #[arg(short = 'f', long, value_name = "FORMAT")]
    pub content_format: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct MessageNewArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Reply within the thread rooted at this message id (a message link fills
    /// this automatically).
    #[arg(long)]
    pub reply_to: Option<String>,
    #[command(flatten)]
    pub content: ContentInputArgs,
}

#[derive(Debug, clap::Args)]
pub struct MessageListArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Number of most-recent messages to show.
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: u32,
    #[command(flatten)]
    pub content: ContentOutputArgs,
}

#[derive(Debug, clap::Args)]
pub struct MessageRefArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Message id (optional if a message link supplies it).
    pub message: Option<String>,
    #[command(flatten)]
    pub content: ContentOutputArgs,
}

#[derive(Debug, clap::Args)]
pub struct MessageEditArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Message id to edit (optional if a message link supplies it).
    pub message: Option<String>,
    #[command(flatten)]
    pub content: ContentInputArgs,
}

#[derive(Debug, clap::Args)]
pub struct MessageReactArgs {
    /// Conversation / channel id, or a Teams deep link.
    pub conversation: String,
    /// Message id to react to (optional if a message link supplies it; then the
    /// next argument is the emoji).
    pub message: Option<String>,
    /// Emoji key (like, heart, laugh, surprised, sad, angry, …).
    pub emoji: Option<String>,
}
