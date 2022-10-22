use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub(crate) struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Add one or more stories to the archive.
    Add {
        /// The URLs of the story or stories to add.
        stories: Vec<String>,
    },

    /// Check for updates to stories in the archive.
    Update {
        /// Force a full refresh of stories (this is slower but will catch updates to
        /// existing chapters).
        #[arg(short = 'f', long = "force")]
        force_refresh: bool,
        /// Refresh only the story with the given name.
        story: Option<String>,
    },

    /// Delete a story from the archive by ID, title, or author name. If more than one story
    /// matches, none will be deleted.
    Delete {
        /// The ID, name, or author of the story to delete.,
        search: String,
    },

    /// Export a story in the archive to a file.
    Export {
        /// The name or ID of the story to export.
        story: String,
    },

    /// List all stories in the archive.
    List {},

    /// List all accepted sources.
    ListSources,
}
