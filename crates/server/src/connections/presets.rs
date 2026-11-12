//! First-class connections: hosted MCP servers Hyperbeam knows how to reach,
//! authenticate to, and sort into read and write tools without an admin
//! classifying every tool by hand.

use super::client::Endpoint;
use serde_json::{Value, json};

/// How a preset's tools are sorted into read and write.
pub enum Classify {
    /// The vendor serves a read-only variant of the server. A tool the
    /// variant also lists is a read tool, anything else is a write tool, and
    /// read calls go through the variant so the vendor enforces it too.
    ReadOnlyVariant(Variant),
    /// Sort by tool-name prefix (`-` and `_` are treated alike). A tool that
    /// matches neither list waits for an admin.
    Prefixes { read: &'static [&'static str], write: &'static [&'static str] },
}

pub enum Variant {
    /// Appended to the server's URL path.
    PathSuffix(&'static str),
    /// Sent as an extra header.
    Header(&'static str, &'static str),
}

pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    /// What the credential is called, and where to get one.
    pub secret_label: &'static str,
    pub secret_help: &'static str,
    pub auth_header: &'static str,
    pub auth_prefix: &'static str,
    /// The credential is a token from an OAuth sign-in rather than a key an
    /// admin pastes.
    pub oauth: bool,
    /// A prefix the vendor puts on every tool name, which agents do not see.
    pub tool_prefix: &'static str,
    /// Headers always sent.
    pub headers: &'static [(&'static str, &'static str)],
    pub read_label: &'static str,
    pub write_label: &'static str,
    pub classify: Classify,
    /// Options an admin may set, shown in the connect dialog.
    pub options: fn() -> Value,
    /// The server URL for the chosen options.
    pub url: fn(&Value) -> String,
}

fn opt<'a>(options: &'a Value, key: &str) -> Option<&'a str> {
    options.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "linear",
        title: "Linear",
        blurb: "Lets agents find, create and update issues, projects and comments.",
        secret_label: "API key",
        secret_help: "A Linear API key (Linear: Settings → Security & access → Personal API keys). Agents act as this key’s user. \
                      A key with only the Read permission limits every agent to reading, whatever their grants.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: false,
        tool_prefix: "",
        headers: &[],
        read_label: "Read issues, projects and comments",
        write_label: "Create and update issues, projects and comments",
        classify: Classify::ReadOnlyVariant(Variant::PathSuffix("/readonly")),
        options: || json!([]),
        url: |_| "https://mcp.linear.app/mcp".into(),
    },
    Preset {
        id: "posthog",
        title: "PostHog",
        blurb: "Lets agents query analytics, and read and manage insights, feature flags, experiments and error tracking.",
        secret_label: "Personal API key",
        secret_help: "A PostHog personal API key created with the “MCP Server” preset (PostHog: Settings → Personal API keys). \
                      Agents act as this key’s user, within the projects and scopes it allows.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: false,
        tool_prefix: "",
        // One tool per operation rather than a single catch-all `exec` tool,
        // so each can be classified and granted on its own.
        headers: &[("x-posthog-mcp-mode", "tools")],
        read_label: "Query data; read insights, flags, experiments and errors",
        write_label: "Create and change insights, flags, experiments and more",
        classify: Classify::ReadOnlyVariant(Variant::Header("x-posthog-read-only", "true")),
        options: || {
            json!([
                { "key": "region", "label": "Region", "choices": [{ "value": "us", "label": "US Cloud" }, { "value": "eu", "label": "EU Cloud" }], "default": "us" },
                { "key": "features", "label": "Only these features", "placeholder": "flags, insights, error-tracking",
                  "hint": "Optional, comma-separated. PostHog offers hundreds of tools; limiting them keeps agents focused." },
            ])
        },
        url: |options| {
            let host = if opt(options, "region") == Some("eu") { "mcp-eu.posthog.com" } else { "mcp.posthog.com" };
            match opt(options, "features") {
                Some(features) => {
                    let list: Vec<String> = features.split(',').map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty()).collect();
                    format!("https://{host}/mcp?features={}", url::form_urlencoded::byte_serialize(list.join(",").as_bytes()).collect::<String>())
                }
                None => format!("https://{host}/mcp"),
            }
        },
    },
    Preset {
        id: "sentry",
        title: "Sentry",
        blurb: "Lets agents search and read issues, events, traces and releases, and act on them.",
        secret_label: "User auth token",
        secret_help: "A Sentry user auth token (Sentry: User settings → Personal tokens) with org:read, project:read and event:read, \
                      plus project:write and event:write if agents may change things. Agents act as this token’s user. \
                      sentry.io only; self-hosted Sentry is not supported yet.",
        auth_header: "Authorization",
        // Sentry reserves `Bearer` for its own OAuth tokens.
        auth_prefix: "Sentry-Bearer ",
        oauth: false,
        tool_prefix: "",
        headers: &[],
        read_label: "Search and read issues, events, traces and releases",
        write_label: "Update issues, add notes, and create or change projects and monitors",
        classify: Classify::Prefixes {
            read: &["get_", "find_", "search_", "list_", "whoami"],
            write: &["create_", "update_", "add_", "delete_", "remove_"],
        },
        options: || {
            json!([{ "key": "organization", "label": "Organization slug", "placeholder": "acme",
                     "hint": "Optional. Limits agents to one organization." }])
        },
        url: |options| match opt(options, "organization") {
            Some(org) => format!("https://mcp.sentry.dev/mcp/{}", url::form_urlencoded::byte_serialize(org.as_bytes()).collect::<String>()),
            None => "https://mcp.sentry.dev/mcp".into(),
        },
    },
    Preset {
        id: "notion",
        title: "Notion",
        blurb: "Lets agents search, read, create and edit pages, databases and comments.",
        secret_label: "",
        secret_help: "You will be sent to Notion to sign in and choose what to share. Agents act as the account you sign in with, \
                      and reach what that account can, so consider a dedicated account that is given only the pages agents should see.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: true,
        // Every tool is `notion-…`; agents see `notion_search`, not `notion_notion-search`.
        tool_prefix: "notion-",
        headers: &[],
        read_label: "Search and read pages, databases and comments",
        write_label: "Create, edit, move and delete pages, databases and comments",
        classify: Classify::Prefixes {
            read: &[
                "search", "fetch", "get_", "list_", "query_", "view_", "read_", "download_", "wait_",
                "ai_search", "connector_search", "memory_search", "calendar_list_",
            ],
            write: &[
                "create_", "update_", "delete_", "move_", "duplicate_", "restore_", "resolve_", "set_", "send_",
                "spawn_", "stop_", "upload_", "convert_", "calendar_create_", "calendar_update_", "calendar_cancel_",
            ],
        },
        options: || json!([]),
        url: |_| "https://mcp.notion.com/mcp".into(),
    },
];

pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

impl Preset {
    /// The read-only variant of `full`, if this preset has one.
    pub fn read_only(&self, full: &Endpoint) -> Option<Endpoint> {
        let Classify::ReadOnlyVariant(variant) = &self.classify else { return None };
        let mut endpoint = full.clone();
        match variant {
            Variant::PathSuffix(suffix) => {
                let mut url = url::Url::parse(&full.url).ok()?;
                let path = format!("{}{suffix}", url.path().trim_end_matches('/'));
                url.set_path(&path);
                endpoint.url = url.to_string();
            }
            Variant::Header(name, value) => endpoint.headers.push((name.to_string(), value.to_string())),
        }
        endpoint.label = format!("{} (read-only)", full.label);
        Some(endpoint)
    }

    /// Classify by name, for presets that sort by prefix.
    pub fn by_prefix(&self, tool: &str, annotations: &Value) -> Option<super::ToolLevel> {
        let Classify::Prefixes { read, write } = &self.classify else { return None };
        let name = self.short_name(tool).to_ascii_lowercase().replace('-', "_");
        let marked_writing = annotations.get("readOnlyHint") == Some(&Value::Bool(false))
            || annotations.get("destructiveHint") == Some(&Value::Bool(true));
        if write.iter().any(|p| name.starts_with(p)) {
            Some(super::ToolLevel::Write)
        } else if read.iter().any(|p| name.starts_with(p)) {
            // A read-sounding name the server itself says writes is a write.
            Some(if marked_writing { super::ToolLevel::Write } else { super::ToolLevel::Read })
        } else {
            None
        }
    }

    /// A tool's name without the vendor's prefix.
    pub fn short_name<'a>(&self, tool: &'a str) -> &'a str {
        tool.strip_prefix(self.tool_prefix).filter(|rest| !rest.is_empty()).unwrap_or(tool)
    }

    pub fn describe(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "blurb": self.blurb,
            "secret_label": self.secret_label,
            "secret_help": self.secret_help,
            "auth": if self.oauth { "oauth" } else { "key" },
            "options": (self.options)(),
            "classified_by": match self.classify {
                Classify::ReadOnlyVariant(_) => "vendor",
                Classify::Prefixes { .. } => "name",
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::ToolLevel;

    fn endpoint(url: &str) -> Endpoint {
        Endpoint { label: "x".into(), url: url.into(), headers: vec![("Authorization".into(), "Bearer k".into())] }
    }

    #[test]
    fn read_only_variants() {
        let linear = find("linear").unwrap().read_only(&endpoint("https://mcp.linear.app/mcp")).unwrap();
        assert_eq!(linear.url, "https://mcp.linear.app/mcp/readonly");
        let posthog = find("posthog").unwrap().read_only(&endpoint("https://mcp.posthog.com/mcp?features=flags")).unwrap();
        assert_eq!(posthog.url, "https://mcp.posthog.com/mcp?features=flags");
        assert!(posthog.headers.contains(&("x-posthog-read-only".into(), "true".into())));
        assert!(find("sentry").unwrap().read_only(&endpoint("https://mcp.sentry.dev/mcp")).is_none());
    }

    #[test]
    fn urls_follow_options() {
        let posthog = find("posthog").unwrap();
        assert_eq!((posthog.url)(&json!({})), "https://mcp.posthog.com/mcp");
        assert_eq!((posthog.url)(&json!({ "region": "eu", "features": "Flags, insights ," })), "https://mcp-eu.posthog.com/mcp?features=flags%2Cinsights");
        assert_eq!((find("sentry").unwrap().url)(&json!({ "organization": "acme" })), "https://mcp.sentry.dev/mcp/acme");
    }

    #[test]
    fn sentry_sorts_by_prefix_and_respects_annotations() {
        let sentry = find("sentry").unwrap();
        assert_eq!(sentry.by_prefix("search_issues", &json!({})), Some(ToolLevel::Read));
        assert_eq!(sentry.by_prefix("get-issue-details", &json!({ "readOnlyHint": true })), Some(ToolLevel::Read));
        assert_eq!(sentry.by_prefix("get_or_create_thing", &json!({ "readOnlyHint": false })), Some(ToolLevel::Write));
        assert_eq!(sentry.by_prefix("update_issue", &json!({ "readOnlyHint": true })), Some(ToolLevel::Write));
        assert_eq!(sentry.by_prefix("analyze_issue_with_seer", &json!({})), None);
    }

    #[test]
    fn notion_tools_lose_their_prefix_and_sort_by_what_follows() {
        let notion = find("notion").unwrap();
        assert_eq!(notion.short_name("notion-search"), "search");
        assert_eq!(notion.short_name("search"), "search");
        assert_eq!(find("sentry").unwrap().short_name("notion-search"), "notion-search");
        for read in ["notion-search", "notion-fetch", "notion-get-comments", "notion-query-data-sources", "notion-calendar-list-events", "notion-list-recent-pages"] {
            assert_eq!(notion.by_prefix(read, &json!({})), Some(ToolLevel::Read), "{read}");
        }
        for write in ["notion-create-pages", "notion-update-page", "notion-delete-pages", "notion-move-pages", "notion-calendar-cancel-events", "notion-spawn-session"] {
            assert_eq!(notion.by_prefix(write, &json!({})), Some(ToolLevel::Write), "{write}");
        }
        assert_eq!(notion.by_prefix("notion-get-or-make", &json!({ "readOnlyHint": false })), Some(ToolLevel::Write));
        assert_eq!(notion.by_prefix("notion-something-new", &json!({})), None);
    }
}
