// Where the landing page links to. Override at build time with environment variables.

/** The docs site. By default it is served alongside this one, under /docs. */
export const DOCS_URL = (process.env.NOEMUNE_DOCS_URL ?? "/docs").replace(/\/$/, "");

/** The source repository. Links to it are hidden until it is set. */
export const REPO_URL = process.env.NOEMUNE_REPO_URL || null;

export const docs = (path = "") => `${DOCS_URL}/${path.replace(/^\//, "")}`;
