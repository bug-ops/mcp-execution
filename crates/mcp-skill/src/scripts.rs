//! Saved scripts: model-written TypeScript kept next to a skill and run again later.
//!
//! A script lives at `<skill-dir>/scripts/<name>.ts` and starts with a header block (ADR-579)
//! that can be read without executing anything:
//!
//! ```text
//! /*! mcp-script
//! {"v":1,"name":"triage","description":"...","parameters":[],"tools":["github/list_issues"],"writes":"read_only"}
//! */
//! ```
//!
//! This module validates, stores, lists and checks scripts; it never runs one, and
//! `generate`/`skill` never touch the `scripts/` directory.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mcp_execution_core::metadata::ServerMetadata;
use mcp_execution_core::{
    ConfinementError, ConfinementTarget, ServerId, ToolName, resolve_confined_path,
};
use serde::Deserialize;
use tokio::io::AsyncWriteExt as _;

/// Longest saved script, in bytes.
pub const MAX_SCRIPT_BYTES: usize = 64 * 1024;

/// Longest header block, in bytes, from the opening marker to the closing `*/`.
pub const MAX_HEADER_BYTES: usize = 8 * 1024;

/// Most scripts one skill may hold.
pub const MAX_SCRIPTS_PER_SKILL: usize = 50;

/// Most parameters one header may declare.
pub const MAX_SCRIPT_PARAMETERS: usize = 16;

/// Most tools one header may reference.
pub const MAX_SCRIPT_TOOLS: usize = 64;

/// Longest [`RecipeName`], in characters.
pub const MAX_RECIPE_NAME_LENGTH: usize = 64;

/// Longest description or parameter name, in characters.
pub const MAX_HEADER_TEXT_CHARS: usize = 200;

/// Attempts at an unused staging name before `save` gives up.
const MAX_STAGING_ATTEMPTS: usize = 8;

const HEADER_OPEN: &str = "/*! mcp-script\n";
const HEADER_CLOSE: &str = "\n*/";
const HEADER_VERSION: u8 = 1;
const SCRIPTS_DIR: &str = "scripts";
const SCRIPT_EXTENSION: &str = "ts";

/// Validated name of a saved script, equal to its file stem.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::RecipeName;
///
/// let name = RecipeName::new("triage-issues").unwrap();
/// assert_eq!(name.as_str(), "triage-issues");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecipeName(String);

impl RecipeName {
    /// Validates `name` (1 to [`MAX_RECIPE_NAME_LENGTH`] lowercase letters, digits and hyphens,
    /// not starting with a hyphen) and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::InvalidName`] when `name` is empty, too long or has characters
    /// outside the allowed set.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::scripts::RecipeName;
    ///
    /// assert!(RecipeName::new("triage-issues").is_ok());
    /// assert!(RecipeName::new("../escape").is_err());
    /// ```
    pub fn new(name: &str) -> Result<Self, ScriptError> {
        let valid = !name.is_empty()
            && name.len() <= MAX_RECIPE_NAME_LENGTH
            && !name.starts_with('-')
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(ScriptError::InvalidName)
        }
    }

    /// Returns the name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::scripts::RecipeName;
    ///
    /// assert_eq!(RecipeName::new("a").unwrap().as_str(), "a");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A tool a saved script calls, as `server/tool`.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ServerId, ToolName};
/// use mcp_execution_skill::scripts::ToolRef;
///
/// let tool = ToolRef {
///     server: ServerId::new("github").unwrap(),
///     tool: ToolName::new("list_issues").unwrap(),
/// };
/// assert_eq!(tool.tool.as_str(), "list_issues");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolRef {
    /// Server the tool belongs to.
    pub server: ServerId,
    /// Tool name on that server.
    pub tool: ToolName,
}

/// Whether a script only reads or may change state on a server.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::WriteIntent;
///
/// assert_ne!(WriteIntent::ReadOnly, WriteIntent::Writes);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteIntent {
    /// The script only calls read tools.
    ReadOnly,
    /// The script may call tools that change state.
    Writes,
}

/// What `save` does when a script of the same name already exists.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::Overwrite;
///
/// assert_ne!(Overwrite::Refuse, Overwrite::Replace);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Fail with [`ScriptError::AlreadyExists`].
    Refuse,
    /// Replace the existing script atomically.
    Replace,
}

/// One declared script parameter.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::ScriptParameter;
///
/// let parameter = ScriptParameter {
///     name: "owner".to_string(),
///     description: "Repository owner".to_string(),
/// };
/// assert_eq!(parameter.name, "owner");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptParameter {
    /// Parameter name.
    pub name: String,
    /// What the parameter means.
    pub description: String,
}

/// The header block at the top of a saved script.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::{RecipeName, ScriptHeader, WriteIntent};
///
/// let header = ScriptHeader {
///     name: RecipeName::new("triage-issues").unwrap(),
///     description: "List and group open issues".to_string(),
///     parameters: vec![],
///     tools: vec![],
///     writes: WriteIntent::ReadOnly,
/// };
/// assert_eq!(header.writes, WriteIntent::ReadOnly);
/// assert!(header.server().is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptHeader {
    /// Script name; must equal the file stem.
    pub name: RecipeName,
    /// One-line description shown by `scripts list`.
    pub description: String,
    /// Declared parameters.
    pub parameters: Vec<ScriptParameter>,
    /// Tools the script calls, all on one server.
    pub tools: Vec<ToolRef>,
    /// Whether the script may change state.
    pub writes: WriteIntent,
}

impl ScriptHeader {
    /// The one server every referenced tool belongs to; `None` when the script references no
    /// tool. [`parse_header`] guarantees there is never more than one.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::scripts::parse_header;
    ///
    /// let text = "/*! mcp-script\n{\"v\":1,\"name\":\"a\",\"description\":\"d\",\"parameters\":[],\
    ///             \"tools\":[\"github/list_issues\"],\"writes\":\"read_only\"}\n*/\n";
    /// let header = parse_header(text).unwrap();
    /// assert_eq!(header.server().unwrap().as_str(), "github");
    /// ```
    #[must_use]
    pub fn server(&self) -> Option<&ServerId> {
        self.tools.first().map(|tool| &tool.server)
    }
}

/// Errors from saving, listing and checking saved scripts.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::ScriptError;
///
/// let err = ScriptError::InvalidHeader { reason: "missing name".to_string() };
/// assert!(err.to_string().contains("missing name"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The script name is not a valid [`RecipeName`].
    #[error("invalid script name")]
    InvalidName,
    /// The header is missing, malformed, out of bounds, or names a different script.
    #[error("invalid script header: {reason}")]
    InvalidHeader {
        /// What is wrong, without echoing script content.
        reason: String,
    },
    /// A script with this name exists and [`Overwrite::Refuse`] was requested.
    #[error("a script named {name} already exists")]
    AlreadyExists {
        /// Name of the existing script.
        name: String,
    },
    /// The script text exceeds its size bound.
    #[error("script exceeds the {limit} byte limit")]
    TooLarge {
        /// The bound that was exceeded, in bytes.
        limit: usize,
    },
    /// The skill already holds [`MAX_SCRIPTS_PER_SKILL`] scripts.
    #[error("the skill already holds {limit} scripts")]
    TooManyScripts {
        /// The bound that was reached.
        limit: usize,
    },
    /// The `scripts/` directory is a symlink or escapes the skill directory.
    #[error("scripts directory is not confined to the skill directory")]
    NotConfined,
    /// The filesystem holding `scripts/` cannot create hard links, which an atomic
    /// no-overwrite save needs; saving with [`Overwrite::Replace`] works there.
    #[error(
        "the filesystem does not support hard links, so a script cannot be saved without \
         overwrite; use overwrite or another skills directory"
    )]
    HardLinksUnsupported,
    /// Reading or writing failed.
    #[error("script I/O failed")]
    Io(#[source] std::io::Error),
}

/// A script as `scripts list` shows it: signature only, never the body.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::{RecipeName, ScriptListing, WriteIntent};
///
/// let listing = ScriptListing {
///     name: RecipeName::new("a").unwrap(),
///     description: String::new(),
///     tools: vec![],
///     writes: WriteIntent::ReadOnly,
/// };
/// assert_eq!(listing.writes, WriteIntent::ReadOnly);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptListing {
    /// Script name.
    pub name: RecipeName,
    /// One-line description.
    pub description: String,
    /// Tools the script calls.
    pub tools: Vec<ToolRef>,
    /// Whether the script may change state.
    pub writes: WriteIntent,
}

/// A saved script that references a tool the server no longer has.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::{ServerId, ToolName};
/// use mcp_execution_skill::scripts::{MissingTool, RecipeName, ToolRef};
///
/// let missing = MissingTool {
///     script: RecipeName::new("a").unwrap(),
///     tool: ToolRef {
///         server: ServerId::new("github").unwrap(),
///         tool: ToolName::new("gone").unwrap(),
///     },
/// };
/// assert_eq!(missing.script.as_str(), "a");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingTool {
    /// Script that references the tool.
    pub script: RecipeName,
    /// The tool that no longer exists.
    pub tool: ToolRef,
}

/// Outcome of [`check`].
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::CheckReport;
///
/// let report = CheckReport::default();
/// assert!(report.is_clean());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Scripts referencing tools missing from the server metadata.
    pub missing_tools: Vec<MissingTool>,
    /// File names whose header could not be read or validated.
    pub invalid_scripts: Vec<String>,
}

impl CheckReport {
    /// Returns `true` when the check found nothing to report.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::scripts::CheckReport;
    ///
    /// assert!(CheckReport::default().is_clean());
    /// ```
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.missing_tools.is_empty() && self.invalid_scripts.is_empty()
    }
}

/// Wire form of the header JSON; converted to [`ScriptHeader`] by [`parse_header`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireHeader {
    v: u8,
    name: String,
    description: String,
    #[serde(default)]
    parameters: Vec<ScriptParameter>,
    tools: Vec<String>,
    writes: WriteIntent,
}

fn invalid(reason: &str) -> ScriptError {
    ScriptError::InvalidHeader {
        reason: reason.to_owned(),
    }
}

fn check_text(text: &str, what: &str) -> Result<(), ScriptError> {
    if text.chars().count() > MAX_HEADER_TEXT_CHARS || text.chars().any(char::is_control) {
        return Err(invalid(&format!(
            "{what} is too long or contains control characters"
        )));
    }
    Ok(())
}

fn parse_tool_ref(raw: &str) -> Result<ToolRef, ScriptError> {
    let (server, tool) = raw
        .split_once('/')
        .ok_or_else(|| invalid("a tool must be written as server/tool"))?;
    Ok(ToolRef {
        server: ServerId::new(server).map_err(|_| invalid("a tool names an invalid server"))?,
        tool: ToolName::new(tool).map_err(|_| invalid("a tool has an invalid name"))?,
    })
}

/// Reads and validates the header block at the start of `text` without executing anything.
///
/// Only the header is inspected: the closing `*/` must come within [`MAX_HEADER_BYTES`], the
/// JSON must have the shape of ADR-579 (version 1, unknown keys refused), the name must be a
/// valid [`RecipeName`], the text fields bounded, and every referenced tool on one server.
///
/// # Errors
///
/// Returns [`ScriptError::InvalidHeader`], whose reason never echoes script content.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::scripts::{WriteIntent, parse_header};
///
/// let text = "/*! mcp-script\n{\"v\":1,\"name\":\"triage\",\"description\":\"d\",\
///             \"parameters\":[],\"tools\":[],\"writes\":\"writes\"}\n*/\nexport {};\n";
/// let header = parse_header(text).unwrap();
/// assert_eq!(header.name.as_str(), "triage");
/// assert_eq!(header.writes, WriteIntent::Writes);
/// assert!(parse_header("console.log(1);").is_err());
/// ```
pub fn parse_header(text: &str) -> Result<ScriptHeader, ScriptError> {
    let rest = text
        .strip_prefix(HEADER_OPEN)
        .ok_or_else(|| invalid("the script must start with an mcp-script header block"))?;
    let end = rest
        .find(HEADER_CLOSE)
        .filter(|end| *end <= MAX_HEADER_BYTES)
        .ok_or_else(|| invalid("the header block is not closed within its size bound"))?;
    let wire: WireHeader = serde_json::from_str(&rest[..end])
        .map_err(|_| invalid("the header is not valid JSON of the expected shape"))?;

    if wire.v != HEADER_VERSION {
        return Err(invalid("unsupported header version"));
    }
    let name = RecipeName::new(&wire.name).map_err(|_| invalid("the header name is invalid"))?;
    if wire.description.is_empty() {
        return Err(invalid("the description is empty"));
    }
    check_text(&wire.description, "the description")?;
    if wire.parameters.len() > MAX_SCRIPT_PARAMETERS {
        return Err(invalid("too many parameters"));
    }
    for parameter in &wire.parameters {
        if parameter.name.is_empty() {
            return Err(invalid("a parameter has no name"));
        }
        check_text(&parameter.name, "a parameter name")?;
        check_text(&parameter.description, "a parameter description")?;
    }
    if wire.tools.len() > MAX_SCRIPT_TOOLS {
        return Err(invalid("too many tools"));
    }
    let tools = wire
        .tools
        .iter()
        .map(|raw| parse_tool_ref(raw))
        .collect::<Result<Vec<_>, _>>()?;
    if tools.iter().any(|tool| tool.server != tools[0].server) {
        return Err(invalid("the tools belong to more than one server"));
    }

    Ok(ScriptHeader {
        name,
        description: wire.description,
        parameters: wire.parameters,
        tools,
        writes: wire.writes,
    })
}

fn confinement_error(error: ConfinementError) -> ScriptError {
    match error {
        ConfinementError::Io(source) | ConfinementError::CreateDir { source, .. } => {
            ScriptError::Io(source)
        }
        ConfinementError::InvalidSegment { .. }
        | ConfinementError::SegmentIsSymlink { .. }
        | ConfinementError::Escape { .. }
        | ConfinementError::NotADirectory { .. }
        | ConfinementError::WrongTargetKind { .. } => ScriptError::NotConfined,
    }
}

fn file_name_of(name: &RecipeName) -> String {
    format!("{}.{SCRIPT_EXTENSION}", name.as_str())
}

/// `<skill_dir>/scripts` when it exists as a real directory; `None` when it is absent.
async fn existing_scripts_dir(skill_dir: &Path) -> Result<Option<PathBuf>, ScriptError> {
    for dir in [skill_dir.to_path_buf(), skill_dir.join(SCRIPTS_DIR)] {
        match tokio::fs::symlink_metadata(&dir).await {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(ScriptError::NotConfined),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(ScriptError::Io(e)),
        }
    }
    Ok(Some(skill_dir.join(SCRIPTS_DIR)))
}

/// Names of the regular `*.ts` files in `dir`, sorted, at most `limit` of them.
async fn script_file_names(dir: &Path, limit: usize) -> Result<Vec<String>, ScriptError> {
    let mut entries = tokio::fs::read_dir(dir).await.map_err(ScriptError::Io)?;
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(ScriptError::Io)? {
        let is_script = Path::new(&entry.file_name()).extension() == Some(OsStr::new("ts"));
        let is_file = entry.file_type().await.is_ok_and(|kind| kind.is_file());
        if is_script
            && is_file
            && let Some(name) = entry.file_name().to_str()
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    names.truncate(limit);
    Ok(names)
}

/// Reads the header of the script file `file_name` in `dir`; a file larger than
/// [`MAX_SCRIPT_BYTES`], an unreadable one or a header whose name differs from the file stem is
/// invalid.
async fn read_header(dir: &Path, file_name: &str) -> Result<ScriptHeader, ScriptError> {
    let path = dir.join(file_name);
    let meta = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(ScriptError::Io)?;
    if !meta.is_file() || meta.len() > MAX_SCRIPT_BYTES as u64 {
        return Err(invalid(
            "the script is not a regular file within its size bound",
        ));
    }
    let bytes = tokio::fs::read(&path).await.map_err(ScriptError::Io)?;
    let text = String::from_utf8_lossy(&bytes);
    let header = parse_header(&text)?;
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    if header.name.as_str() != stem {
        return Err(invalid("the header name differs from the file name"));
    }
    Ok(header)
}

/// Writes `bytes` to a new hidden file in `dir` and returns its path.
///
/// The file is created with `create_new` (`O_EXCL`: an existing path of any kind, including a
/// planted symlink or hard link, is never opened) under a name that is not predictable, with
/// mode 0600 on Unix, so nothing already on disk can be written through.
async fn stage_exclusively(
    dir: &Path,
    name: &RecipeName,
    bytes: &[u8],
) -> Result<PathBuf, ScriptError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    for _ in 0..MAX_STAGING_ATTEMPTS {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = dir.join(format!(
            ".{}.{:x}{:x}{:x}.tmp",
            name.as_str(),
            std::process::id(),
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path).await {
            Ok(mut file) => {
                let written = match file.write_all(bytes).await {
                    Ok(()) => file.flush().await,
                    Err(e) => Err(e),
                };
                return match written {
                    Ok(()) => Ok(path),
                    Err(e) => {
                        tokio::fs::remove_file(&path).await.ok();
                        Err(ScriptError::Io(e))
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(ScriptError::Io(e)),
        }
    }
    Err(ScriptError::Io(std::io::ErrorKind::AlreadyExists.into()))
}

/// Saves `text` as `<skill_dir>/scripts/<name>.ts` with an atomic, confined write.
///
/// The header must be valid and its name equal to `name`; an existing script is refused unless
/// `overwrite` is [`Overwrite::Replace`]. Nothing is written on error and nothing is executed.
/// The text is staged in a hidden temporary file and then linked (refuse) or renamed (replace)
/// into place, so a reader never sees a partial script.
///
/// # Errors
///
/// Returns [`ScriptError`] for an invalid header, a name mismatch, an existing script, an
/// oversized script, a skill already at [`MAX_SCRIPTS_PER_SKILL`], a symlinked `scripts/`
/// directory, or an I/O failure.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::scripts::{Overwrite, RecipeName, save};
/// use std::path::Path;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let name = RecipeName::new("triage-issues")?;
/// let text = "/*! mcp-script\n{\"v\":1,\"name\":\"triage-issues\",\"description\":\"d\",\
///             \"parameters\":[],\"tools\":[],\"writes\":\"read_only\"}\n*/\nexport {};\n";
/// let path = save(Path::new("/skills/github"), &name, text, Overwrite::Refuse).await?;
/// println!("{}", path.display());
/// # Ok(())
/// # }
/// ```
pub async fn save(
    skill_dir: &Path,
    name: &RecipeName,
    text: &str,
    overwrite: Overwrite,
) -> Result<PathBuf, ScriptError> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(ScriptError::TooLarge {
            limit: MAX_SCRIPT_BYTES,
        });
    }
    if parse_header(text)?.name != *name {
        return Err(invalid("the header name differs from the requested name"));
    }

    let (base, segment) = skill_dir
        .parent()
        .zip(skill_dir.file_name().and_then(OsStr::to_str))
        .ok_or(ScriptError::NotConfined)?;
    if matches!(
        existing_scripts_dir(skill_dir).await,
        Err(ScriptError::NotConfined)
    ) {
        return Err(ScriptError::NotConfined);
    }
    let file_name = file_name_of(name);
    let target = resolve_confined_path(
        base,
        segment,
        Path::new(SCRIPTS_DIR),
        Some(ConfinementTarget::File(OsStr::new(&file_name))),
    )
    .await
    .map_err(confinement_error)?;
    let dir = target.parent().ok_or(ScriptError::NotConfined)?;

    let exists = tokio::fs::symlink_metadata(&target).await.is_ok();
    if exists && overwrite == Overwrite::Refuse {
        return Err(ScriptError::AlreadyExists {
            name: name.as_str().to_owned(),
        });
    }
    if !exists && script_file_names(dir, usize::MAX).await?.len() >= MAX_SCRIPTS_PER_SKILL {
        return Err(ScriptError::TooManyScripts {
            limit: MAX_SCRIPTS_PER_SKILL,
        });
    }

    let staging = stage_exclusively(dir, name, text.as_bytes()).await?;
    let published = match overwrite {
        Overwrite::Replace => tokio::fs::rename(&staging, &target).await,
        Overwrite::Refuse => tokio::fs::hard_link(&staging, &target).await,
    };
    if overwrite == Overwrite::Refuse || published.is_err() {
        // Best effort: the staging file is hidden and bounded, a leftover is harmless.
        tokio::fs::remove_file(&staging).await.ok();
    }
    published.map_err(|e| publish_error(overwrite, name, e))?;
    Ok(skill_dir.join(SCRIPTS_DIR).join(file_name))
}

/// Classifies a failed publish step. A refused hard link that is not "already exists" means the
/// filesystem has no hard links (`Unsupported`, or `PermissionDenied` as on FAT-like and some
/// network filesystems); an exclusive no-overwrite save is impossible there without risking a
/// visible partial script, so it fails with a classified error instead of falling back.
fn publish_error(overwrite: Overwrite, name: &RecipeName, error: std::io::Error) -> ScriptError {
    use std::io::ErrorKind;

    match (overwrite, error.kind()) {
        (_, ErrorKind::AlreadyExists) => ScriptError::AlreadyExists {
            name: name.as_str().to_owned(),
        },
        (Overwrite::Refuse, ErrorKind::Unsupported | ErrorKind::PermissionDenied) => {
            ScriptError::HardLinksUnsupported
        }
        _ => ScriptError::Io(error),
    }
}

/// Lists the saved scripts of a skill by signature, without their bodies.
///
/// A script whose header cannot be read, or whose header name differs from its file name, is
/// skipped. At most [`MAX_SCRIPTS_PER_SKILL`] scripts are read.
///
/// # Errors
///
/// Returns [`ScriptError::NotConfined`] for a symlinked `scripts/` directory and
/// [`ScriptError::Io`] when the directory cannot be read.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_skill::scripts::list;
/// use std::path::Path;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// for script in list(Path::new("/skills/github")).await? {
///     println!("{}: {}", script.name.as_str(), script.description);
/// }
/// # Ok(())
/// # }
/// ```
pub async fn list(skill_dir: &Path) -> Result<Vec<ScriptListing>, ScriptError> {
    let Some(dir) = existing_scripts_dir(skill_dir).await? else {
        return Ok(Vec::new());
    };
    let mut listings = Vec::new();
    for file_name in script_file_names(&dir, MAX_SCRIPTS_PER_SKILL).await? {
        if let Ok(header) = read_header(&dir, &file_name).await {
            listings.push(ScriptListing {
                name: header.name,
                description: header.description,
                tools: header.tools,
                writes: header.writes,
            });
        }
    }
    Ok(listings)
}

/// Checks saved scripts against the server's current tools.
///
/// Never modifies anything; reports scripts that reference tools missing from `metadata` (a
/// tool of another server counts as missing) and scripts whose header is invalid. A `scripts/`
/// directory that cannot be inspected is reported as one invalid entry.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_core::metadata::ServerMetadata;
/// use mcp_execution_skill::scripts::check;
/// use std::path::Path;
///
/// # async fn example(metadata: &ServerMetadata) {
/// let report = check(Path::new("/skills/github"), metadata).await;
/// assert!(report.is_clean());
/// # }
/// ```
pub async fn check(skill_dir: &Path, metadata: &ServerMetadata) -> CheckReport {
    let mut report = CheckReport::default();
    let dir = match existing_scripts_dir(skill_dir).await {
        Ok(Some(dir)) => dir,
        Ok(None) => return report,
        Err(_) => {
            report.invalid_scripts.push(SCRIPTS_DIR.to_owned());
            return report;
        }
    };
    let Ok(file_names) = script_file_names(&dir, MAX_SCRIPTS_PER_SKILL).await else {
        report.invalid_scripts.push(SCRIPTS_DIR.to_owned());
        return report;
    };
    for file_name in file_names {
        match read_header(&dir, &file_name).await {
            Ok(header) => {
                for tool in header.tools {
                    let known = tool.server == metadata.server_id
                        && metadata.tools.iter().any(|t| t.name == tool.tool);
                    if !known {
                        report.missing_tools.push(MissingTool {
                            script: header.name.clone(),
                            tool,
                        });
                    }
                }
            }
            Err(_) => report.invalid_scripts.push(file_name),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    fn header_text(extra: &str) -> String {
        format!(
            "/*! mcp-script\n{{\"v\":1,\"name\":\"a\",\"description\":\"d\",\"parameters\":[],\
             \"tools\":[],\"writes\":\"read_only\"{extra}}}\n*/\n"
        )
    }

    #[test]
    fn header_refuses_unknown_keys_versions_and_mixed_servers() {
        assert!(parse_header(&header_text("")).is_ok());
        assert!(parse_header(&header_text(",\"extra\":1")).is_err());
        assert!(parse_header(&header_text("").replace("\"v\":1", "\"v\":2")).is_err());
        let mixed = header_text("").replace("\"tools\":[]", "\"tools\":[\"a/x\",\"b/y\"]");
        assert!(parse_header(&mixed).is_err());
    }

    #[test]
    fn header_reason_never_echoes_content() {
        let hostile = "/*! mcp-script\n{\"v\":1,\"SECRET-KEY-77\":1}\n*/\n";
        let message = parse_header(hostile).unwrap_err().to_string();
        assert!(!message.contains("SECRET-KEY-77"), "{message}");
    }

    #[test]
    fn header_must_close_within_its_bound() {
        let padding = "x".repeat(MAX_HEADER_BYTES + 1);
        let text = format!("/*! mcp-script\n{{\"v\":1,\"name\":\"{padding}\"}}\n*/\n");
        assert!(parse_header(&text).is_err());
    }

    #[test]
    fn header_refuses_control_characters_in_text_fields() {
        let text = header_text("").replace("\"d\"", "\"line\\nbreak\"");
        assert!(parse_header(&text).is_err());
    }

    #[test]
    fn server_is_the_single_server_of_the_tools() {
        let text = header_text("").replace("\"tools\":[]", "\"tools\":[\"github/x\",\"github/y\"]");
        let header = parse_header(&text).unwrap();
        assert_eq!(header.server().unwrap().as_str(), "github");
    }

    #[test]
    fn server_is_none_without_tools() {
        assert!(parse_header(&header_text("")).unwrap().server().is_none());
    }

    #[tokio::test]
    async fn save_never_writes_through_a_planted_link_at_the_old_staging_name() {
        let skill = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim.txt");
        std::fs::write(&victim, "precious").unwrap();
        let scripts = skill.path().join(SCRIPTS_DIR);
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::hard_link(&victim, scripts.join(".a.tmp")).unwrap();

        let name = RecipeName::new("a").unwrap();
        save(skill.path(), &name, &header_text(""), Overwrite::Refuse)
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");
        assert_eq!(
            std::fs::read_to_string(scripts.join("a.ts")).unwrap(),
            header_text("")
        );
    }

    #[test]
    fn a_refused_hard_link_is_reported_as_a_filesystem_limitation() {
        use std::io::{Error, ErrorKind};

        let name = RecipeName::new("a").unwrap();

        for kind in [ErrorKind::Unsupported, ErrorKind::PermissionDenied] {
            assert_matches!(
                publish_error(Overwrite::Refuse, &name, Error::from(kind)),
                ScriptError::HardLinksUnsupported
            );
            assert_matches!(
                publish_error(Overwrite::Replace, &name, Error::from(kind)),
                ScriptError::Io(_)
            );
        }
        assert_matches!(
            publish_error(Overwrite::Refuse, &name, ErrorKind::AlreadyExists.into()),
            ScriptError::AlreadyExists { .. }
        );
    }

    #[tokio::test]
    async fn save_rechecks_the_header_name_itself() {
        let skill = tempfile::tempdir().unwrap();
        let other = RecipeName::new("other").unwrap();

        let result = save(skill.path(), &other, &header_text(""), Overwrite::Refuse).await;

        assert_matches!(result, Err(ScriptError::InvalidHeader { .. }));
        assert!(!skill.path().join(SCRIPTS_DIR).exists());
    }
}
