// The sidebar, in reading order. Each slug is a file in content/ (content/<slug>.md).
// Prev/next links follow this order too.

export interface NavGroup {
  title: string;
  pages: string[];
}

export const NAV: NavGroup[] = [
  { title: "Get started", pages: ["introduction", "quickstart", "concepts"] },
  { title: "Using Noemune", pages: ["starting-work", "sessions", "board", "conductor", "library", "workflows", "cli"] },
  { title: "Running a team", pages: ["team-setup", "permissions", "harnesses", "credentials", "integrations"] },
  { title: "Deploy", pages: ["install-kubernetes", "configuration", "security"] },
  { title: "Project", pages: ["status", "development"] },
];

export const ORDER = NAV.flatMap((g) => g.pages);
