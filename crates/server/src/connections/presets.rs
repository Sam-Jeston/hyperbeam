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
    /// The vendor does not let clients register themselves: an admin creates
    /// an OAuth client with it and enters the client's ID and secret.
    pub oauth_client: bool,
    /// Where an admin creates that client, shown beside the redirect URL.
    pub oauth_client_help: &'static str,
    /// A prefix the vendor puts on every tool name, which agents do not see.
    pub tool_prefix: &'static str,
    /// Headers always sent.
    pub headers: &'static [(&'static str, &'static str)],
    pub read_label: &'static str,
    pub write_label: &'static str,
    pub classify: Classify,
    /// A web search: URLs in what its read tools return are ones an agent
    /// may go on to read with `web_fetch`.
    pub search: bool,
    /// Options an admin may set, shown in the connect dialog.
    pub options: fn() -> Value,
    /// The server URL for the chosen options.
    pub url: fn(&Value) -> String,
}

fn opt<'a>(options: &'a Value, key: &str) -> Option<&'a str> {
    options.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn csv(raw: &str) -> String {
    let list: Vec<String> = raw.split(',').map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty()).collect();
    url::form_urlencoded::byte_serialize(list.join(",").as_bytes()).collect()
}

/// A comma-separated list an admin typed, tidied for a query string.
/// Datadog serves each site from its own host.
const DATADOG_SITES: &[(&str, &str, &str)] = &[
    ("us1", "US1 (datadoghq.com)", "mcp.datadoghq.com"),
    ("us3", "US3 (us3.datadoghq.com)", "mcp.us3.datadoghq.com"),
    ("us5", "US5 (us5.datadoghq.com)", "mcp.us5.datadoghq.com"),
    ("eu1", "EU1 (datadoghq.eu)", "mcp.datadoghq.eu"),
    ("ap1", "AP1 (ap1.datadoghq.com)", "mcp.ap1.datadoghq.com"),
    ("ap2", "AP2 (ap2.datadoghq.com)", "mcp.ap2.datadoghq.com"),
    ("uk1", "UK1 (uk1.datadoghq.com)", "mcp.uk1.datadoghq.com"),
];

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
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Read issues, projects and comments",
        write_label: "Create and update issues, projects and comments",
        classify: Classify::ReadOnlyVariant(Variant::PathSuffix("/readonly")),
        search: false,
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
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        // One tool per operation rather than a single catch-all `exec` tool,
        // so each can be classified and granted on its own.
        headers: &[("x-posthog-mcp-mode", "tools")],
        read_label: "Query data; read insights, flags, experiments and errors",
        write_label: "Create and change insights, flags, experiments and more",
        classify: Classify::ReadOnlyVariant(Variant::Header("x-posthog-read-only", "true")),
        search: false,
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
                Some(features) => format!("https://{host}/mcp?features={}", csv(features)),
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
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Search and read issues, events, traces and releases",
        write_label: "Update issues, add notes, and create or change projects and monitors",
        classify: Classify::Prefixes {
            read: &["get_", "find_", "search_", "list_", "whoami"],
            write: &["create_", "update_", "add_", "delete_", "remove_"],
        },
        search: false,
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
        oauth_client: false,
        oauth_client_help: "",
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
        search: false,
        options: || json!([]),
        url: |_| "https://mcp.notion.com/mcp".into(),
    },
    Preset {
        id: "exa",
        title: "Exa web search",
        blurb: "Lets agents search the web. What a search returns, they can go on to read with web_fetch.",
        secret_label: "API key",
        secret_help: "An Exa API key (dashboard.exa.ai → API keys). Searches are billed to this key. \
                      What agents search for is sent to Exa.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: false,
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Search the web",
        write_label: "Use tools an admin has marked as writing",
        // Only search is cleared by default. Exa's own page fetcher takes any
        // URL an agent writes, which `web_fetch` exists to avoid, so it waits
        // for an admin like any other unsorted tool.
        classify: Classify::Prefixes { read: &["web_search"], write: &[] },
        search: true,
        options: || json!([]),
        url: |_| "https://mcp.exa.ai/mcp".into(),
    },
    Preset {
        id: "betterstack",
        title: "Better Stack",
        blurb: "Lets agents query logs, metrics and errors, and read and manage incidents, on-call, monitors, status pages and dashboards.",
        secret_label: "API token",
        secret_help: "A Better Stack API token (Better Stack: Settings → API tokens). Agents act with this token’s access. \
                      A token without “Read & write” access limits every agent to reading, whatever their grants.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: false,
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Query telemetry; read incidents, on-call, monitors, status pages and dashboards",
        write_label: "Create and change incidents, monitors, status pages, dashboards and more",
        // Better Stack documents its tools by what they do rather than by
        // name, so these follow its verbs. A tool they miss waits for an admin.
        classify: Classify::Prefixes {
            read: &["list_", "get_", "search_", "render_", "plan_", "export_"],
            write: &[
                "create_", "update_", "remove_", "delete_", "add_", "edit_", "configure_", "set_", "import_", "move_", "upsert_",
                "acknowledge_", "escalate_", "resolve_", "reopen_", "invite_", "change_", "link_", "unlink_", "pause_", "resume_",
            ],
        },
        search: false,
        options: || json!([]),
        url: |_| "https://mcp.betterstack.com".into(),
    },
    Preset {
        id: "datadog",
        title: "Datadog",
        blurb: "Lets agents search logs, metrics, traces, monitors, incidents and dashboards, and act on them.",
        secret_label: "",
        secret_help: "You will be sent to Datadog to sign in. Agents act as the account you sign in with, within its roles: \
                      reading needs the MCP Read permission, and changing things needs MCP Write and your organization’s “MCP Write Access” setting. \
                      Consider a dedicated account. GovCloud sites are not supported by Datadog’s MCP server.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: true,
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Search and read logs, metrics, traces, monitors, incidents and dashboards",
        write_label: "Create and change notebooks, monitors, dashboards, cases and more",
        classify: Classify::Prefixes {
            read: &[
                "search_", "get_", "list_", "aggregate_", "analyze_", "analyse_", "validate_", "find_", "describe_", "read_", "explore_",
                "estimate_", "build_", "check_", "summarize_", "rank_", "suggest_", "verify_", "forecast_", "diagnose_", "discover_",
                "optimize_", "apm_get_", "apm_search_", "apm_investigate_", "apm_latency_", "apm_discover_", "apm_query_",
                "ddsql_get_", "ddsql_schema_", "ddsql_search_", "ddsql_read_", "ddsql_run_query", "cost_recommendations",
                "run_product_analytics_", "run_network_path",
            ],
            write: &[
                "create_", "update_", "delete_", "edit_", "upsert_", "append_", "replace_", "recover_", "clone_", "add_", "remove_",
                "execute_", "trigger_", "steer_", "manage_", "move_", "publish_", "unpublish_", "cancel_", "start_", "conclude_",
                "link_", "unlink_", "attach_", "detach_", "sync_", "mitigate_", "enable_", "disable_", "reorder_", "send_", "mute_",
                "assign_", "override_", "unblock_", "ddsql_upsert_", "ddsql_create_", "run_datadog_bits_agent",
                // Runs shell commands on hosts, whatever Datadog calls it.
                "datadog_remote_action_", "synthetics_test_wizard",
            ],
        },
        search: false,
        options: || {
            let sites: Vec<Value> = DATADOG_SITES.iter().map(|(value, label, _)| json!({ "value": value, "label": label })).collect();
            json!([
                { "key": "site", "label": "Datadog site", "choices": sites, "default": "us1" },
                { "key": "toolsets", "label": "Toolsets", "placeholder": "core, apm, error-tracking",
                  "hint": "Optional, comma-separated. Datadog offers only its core tools unless you name more, or “all”." },
            ])
        },
        url: |options| {
            let site = opt(options, "site").unwrap_or("us1");
            let host = DATADOG_SITES.iter().find(|(id, ..)| *id == site).map_or(DATADOG_SITES[0].2, |(.., host)| host);
            match opt(options, "toolsets") {
                Some(toolsets) => format!("https://{host}/v1/mcp?toolsets={}", csv(toolsets)),
                None => format!("https://{host}/v1/mcp"),
            }
        },
    },
    Preset {
        id: "figma",
        title: "Figma",
        blurb: "Lets agents read designs, variables and screenshots from Figma files, and create and edit content in them.",
        secret_label: "",
        secret_help: "You will be sent to Figma to sign in. Agents act as the account you sign in with, and reach the files it can. \
                      Figma only admits MCP clients in its MCP catalog, so the sign-in is refused until Figma has approved Hyperbeam for your installation.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: true,
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Read designs, variables, screenshots and libraries",
        write_label: "Create and edit files, designs, diagrams and Code Connect mappings",
        classify: Classify::Prefixes {
            read: &["get_", "list_", "search_", "download_", "whoami", "weave_list_", "weave_get_", "weave_find_"],
            write: &[
                "add_", "create_", "generate_", "html_to_figma", "send_", "update_", "upload_", "use_figma",
                "weave_upload_", "weave_run_", "weave_cancel_",
            ],
        },
        search: false,
        options: || json!([]),
        url: |_| "https://mcp.figma.com/mcp".into(),
    },
    Preset {
        id: "granola",
        title: "Granola",
        blurb: "Lets agents search and read meeting notes and transcripts, and organise notes into folders.",
        secret_label: "",
        secret_help: "You will be sent to Granola to sign in. Agents act as the account you sign in with, and reach the notes it can: \
                      every agent sees that person’s meetings. Transcripts and editing need a paid Granola plan.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: true,
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Search and read meeting notes, transcripts and folders",
        write_label: "Edit private notes, regenerate summaries and organise folders",
        classify: Classify::Prefixes {
            read: &["search_", "list_", "get_", "query_"],
            // Switching workspace or signing out changes the account for every agent.
            write: &["create_", "update_", "add_", "remove_", "generate_", "switch_", "sign_out"],
        },
        search: false,
        options: || json!([]),
        url: |_| "https://mcp.granola.ai/mcp".into(),
    },
    Preset {
        id: "hubspot",
        title: "HubSpot",
        blurb: "Lets agents search and read CRM records, campaigns, conversations and content, and create and update them.",
        secret_label: "",
        secret_help: "You will be sent to HubSpot to sign in and choose an account. Agents act as the user you sign in with, within that user’s permissions.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: true,
        oauth_client: true,
        oauth_client_help: "In HubSpot, go to Development → MCP Connectors → Create MCP connector, and give it the redirect URL below. \
                            Its details page shows the client ID and secret.",
        tool_prefix: "",
        headers: &[],
        read_label: "Search and read CRM records, properties, campaigns, conversations and analytics",
        write_label: "Create and update CRM records, properties, campaigns, emails, pages and posts",
        classify: Classify::Prefixes {
            read: &["get_", "search_", "read_", "query_", "discover_", "render_", "tool_guidance"],
            write: &["manage_", "submit_", "create_", "update_", "delete_"],
        },
        search: false,
        options: || json!([]),
        url: |_| "https://mcp.hubspot.com".into(),
    },
    Preset {
        id: "rootly",
        title: "Rootly",
        blurb: "Lets agents find and read incidents, alerts and on-call schedules, and create and update them.",
        secret_label: "API key",
        secret_help: "A Rootly API key (Rootly: Account → Manage API keys → Generate New API Key). A global key sees every team; \
                      a team or personal key limits agents to what that team or person can reach.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: false,
        oauth_client: false,
        oauth_client_help: "",
        tool_prefix: "",
        headers: &[],
        read_label: "Find and read incidents, alerts, on-call schedules and services",
        write_label: "Create and update incidents, action items, schedules and more",
        classify: Classify::ReadOnlyVariant(Variant::Header("X-MCP-Readonly", "true")),
        search: false,
        options: || {
            json!([{ "key": "toolsets", "label": "Toolsets", "placeholder": "core, incidents, oncall",
                     "hint": "Optional, comma-separated. Rootly offers about 280 tools with “all”; naming a few keeps agents focused." }])
        },
        url: |options| match opt(options, "toolsets") {
            Some(toolsets) => format!("https://mcp.rootly.com/mcp?toolsets={}", csv(toolsets)),
            None => "https://mcp.rootly.com/mcp".into(),
        },
    },
    Preset {
        id: "slack",
        title: "Slack",
        blurb: "Lets agents search and read messages, channels, files and canvases, and send messages and create canvases.",
        secret_label: "",
        secret_help: "You will be sent to Slack to sign in. Agents act as the person who signs in: they read what that person can, \
                      and what they send is sent in that person’s name. Consider a dedicated account.",
        auth_header: "Authorization",
        auth_prefix: "Bearer ",
        oauth: true,
        oauth_client: true,
        oauth_client_help: "Create a Slack app for your workspace at api.slack.com/apps (Slack admits only internal or Marketplace apps). \
                            Under Agents, turn on the Slack MCP server; under OAuth & Permissions, add the redirect URL below and the user scopes agents should have. \
                            Basic Information shows the client ID and secret.",
        // Every tool is `slack_…`; agents see `slack_send_message`, not `slack_slack_send_message`.
        tool_prefix: "slack_",
        headers: &[],
        read_label: "Search and read messages, channels, files, canvases and profiles",
        write_label: "Send and schedule messages, react, upload files, and create channels, canvases and lists",
        classify: Classify::Prefixes {
            read: &["search_", "read_", "list_", "get_"],
            // The first step of an upload, despite its name.
            write: &["get_file_upload", "send_", "schedule_", "create_", "update_", "add_", "remove_", "delete_", "upload_", "complete_", "invite_", "join_"],
        },
        search: false,
        options: || json!([]),
        url: |_| "https://mcp.slack.com/mcp".into(),
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
            "oauth_client": self.oauth_client,
            "oauth_client_help": self.oauth_client_help,
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
    fn a_search_preset_clears_search_and_nothing_that_fetches() {
        let exa = find("exa").unwrap();
        assert!(exa.search);
        assert_eq!(exa.by_prefix("web_search_exa", &json!({ "readOnlyHint": true })), Some(ToolLevel::Read));
        assert_eq!(exa.by_prefix("web_fetch_exa", &json!({ "readOnlyHint": true })), None);
        assert!(PRESETS.iter().filter(|p| p.search).all(|p| p.id == "exa"));
    }

    #[test]
    fn every_preset_is_usable() {
        for preset in PRESETS {
            assert!(url::Url::parse(&(preset.url)(&json!({}))).is_ok_and(|u| u.scheme() == "https"), "{}", preset.id);
            assert!(preset.id.chars().all(|c| c.is_ascii_lowercase()), "{}", preset.id);
            assert_eq!(PRESETS.iter().filter(|p| p.id == preset.id).count(), 1, "{}", preset.id);
            // A pasted key needs a name; a sign-in has none to paste.
            assert_eq!(preset.oauth, preset.secret_label.is_empty(), "{}", preset.id);
            assert!(!preset.oauth_client || preset.oauth, "{}", preset.id);
            assert_eq!(preset.oauth_client, !preset.oauth_client_help.is_empty(), "{}", preset.id);
        }
    }

    #[test]
    fn new_urls_follow_options() {
        let datadog = find("datadog").unwrap();
        assert_eq!((datadog.url)(&json!({})), "https://mcp.datadoghq.com/v1/mcp");
        assert_eq!((datadog.url)(&json!({ "site": "eu1", "toolsets": "Core, apm ," })), "https://mcp.datadoghq.eu/v1/mcp?toolsets=core%2Capm");
        assert_eq!((datadog.url)(&json!({ "site": "elsewhere" })), "https://mcp.datadoghq.com/v1/mcp");
        let rootly = find("rootly").unwrap();
        assert_eq!((rootly.url)(&json!({ "toolsets": "core,incidents" })), "https://mcp.rootly.com/mcp?toolsets=core%2Cincidents");
        let read_only = rootly.read_only(&endpoint("https://mcp.rootly.com/mcp?toolsets=core")).unwrap();
        assert_eq!(read_only.url, "https://mcp.rootly.com/mcp?toolsets=core");
        assert!(read_only.headers.contains(&("X-MCP-Readonly".into(), "true".into())));
    }

    #[test]
    fn documented_tools_sort_as_the_vendors_describe_them() {
        let cases: &[(&str, &[&str], &[&str], &[&str])] = &[
            (
                "datadog",
                &["search_datadog_logs", "get_datadog_metric", "aggregate_spans", "analyze_datadog_logs", "apm_get_service_health", "ddsql_run_query", "run_product_analytics_journey"],
                &["create_datadog_notebook", "edit_datadog_notebook", "upsert_datadog_dashboard", "execute_code", "execute_datadog_workflow", "ddsql_upsert_saved_query", "run_datadog_bits_agent", "datadog_remote_action_restricted_shell_run_command", "trigger_bits_ai_investigation"],
                &["swap_widget_type", "kubernetes-ops-center-api-server-client-load"],
            ),
            (
                "figma",
                &["get_design_context", "get_screenshot", "get_variable_defs", "list_shaders", "search_design_system", "download_assets", "whoami", "weave_list_tools"],
                &["use_figma", "generate_figma_design", "create_new_file", "html_to_figma", "send_code_to_figma", "upload_assets", "add_code_connect_map", "weave_run_model"],
                &[],
            ),
            (
                "granola",
                &["search_meetings", "list_meetings", "get_meeting_transcript", "query_granola_meetings", "get_account_info"],
                &["create_folder", "update_private_notes", "add_note_to_folder", "remove_note_from_folder", "generate_summary", "switch_workspace", "sign_out"],
                &[],
            ),
            (
                "hubspot",
                &["get_user_details", "search_crm_objects", "query_crm_data", "read_campaign_data", "discover_hubspot_schema", "render_asset", "tool_guidance"],
                &["manage_crm_objects", "manage_segment", "manage_blog_post", "manage_onboarding", "submit_feedback"],
                &[],
            ),
            (
                "slack",
                &["slack_search_public", "slack_read_channel", "slack_read_user_profile"],
                &["slack_send_message", "slack_schedule_message", "slack_create_canvas", "slack_get_file_upload_url", "slack_complete_file_upload"],
                &["slack_something_new"],
            ),
            ("betterstack", &["list_monitors", "get_incident_details"], &["remove_dashboard", "remove_chart", "create_incident", "acknowledge_incident"], &["monitors"]),
        ];
        for (id, read, write, unsorted) in cases {
            let preset = find(id).unwrap();
            for tool in *read {
                assert_eq!(preset.by_prefix(tool, &json!({})), Some(ToolLevel::Read), "{id} {tool}");
            }
            for tool in *write {
                assert_eq!(preset.by_prefix(tool, &json!({ "readOnlyHint": true })), Some(ToolLevel::Write), "{id} {tool}");
            }
            for tool in *unsorted {
                assert_eq!(preset.by_prefix(tool, &json!({})), None, "{id} {tool}");
            }
        }
        assert_eq!(find("slack").unwrap().short_name("slack_send_message"), "send_message");
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
