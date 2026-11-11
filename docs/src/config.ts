// Where the docs are served and what they link to. Override with environment variables at build time.

/** Path the docs are served under. By default they sit beside the landing page at /docs. */
export const BASE = ("/" + (process.env.NOEMUNE_DOCS_BASE ?? "/docs").replace(/^\/+|\/+$/g, "")).replace(/^\/$/, "/");

/** The landing page. */
export const SITE_URL = process.env.NOEMUNE_SITE_URL ?? "/";

/** The source repository. "Edit this page" and source links are hidden until it is set. */
export const REPO_URL = process.env.NOEMUNE_REPO_URL || null;

/** Branch that "Edit this page" links to. */
export const REPO_BRANCH = process.env.NOEMUNE_REPO_BRANCH ?? "main";
