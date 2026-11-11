# Noemune brand: Culture

Every agent works in its own sealed dish, and the team watches what it does. The UI should feel like a
calm, well-lit lab: precise and observant, but approachable for people who never open a terminal.

Read this before changing anything visual. Tokens live in `src/app.css`, components in
`src/components/ui/`, app-level pieces (status, avatars, page layout) in `src/lib/ui.tsx`.

## Mark

`<BrandMark />` (`components/ui/brand.tsx`): a ring (the sandbox) holding a violet nucleus (the agent)
and two pink helpers. The wordmark is lowercase **noemune** in Bricolage Grotesque Bold, tracked tight.
Keep the mark in the theme colors; never recolor it to a status color.

## Color

Dark is the default theme; light is a full counterpart (`.light` on `<html>`, chosen in the user menu).
Use the Tailwind names below, never hex values or Tailwind's built-in palette.

| Token | Role |
|---|---|
| `background` / `surface` / `surface-2` / `elevated` | page, cards, wells and hover, popovers and dialogs |
| `border` / `border-strong` | dividers / control outlines |
| `foreground` / `muted` / `faint` | text, secondary text, tertiary text and icons |
| `primary` (crystal violet) | the one primary action per view, selection, focus rings, links |
| `accent` / `accent-foreground` | soft violet fill: active nav, selected choices, the user's own messages |
| `second` (safranin) | the mark only |
| `run` / `wait` / `idle` / `fail` | session state, and only that (plus success/warning/error notices) |

Session state always maps the same way: running → `run`, queued/starting → `wait`, waiting for input →
`idle`, failed → `fail`, asleep/stopped → `faint`. Use `<StatusDot>` and `<StatusBadge>`, not hand-made dots.

## Type

- **Bricolage Grotesque** (`font-display`): page titles, dialog titles, section headings, empty-state titles.
- **Figtree** (`font-sans`, the default): everything else. Body is 13.5–14px.
- **JetBrains Mono** (`font-mono`): repository names, branches, env vars, grants, tool calls, code.

Small uppercase labels (`text-[11.5px] font-semibold uppercase tracking-[0.07em] text-faint`) name panels
and groups. Do not use them for anything a person needs to read closely.

## Components

Build from the kit; add to it rather than styling one-offs inline.

| Need | Use |
|---|---|
| Buttons | `Button` (`primary`, `secondary`, `ghost`, `danger`, `danger-ghost`; `loading` for in-flight actions) |
| Form rows | `Field` + `Input` / `Textarea` / `Select`; `CheckRow`, `SwitchRow`, `RadioCards`, `ToggleChip` |
| Dialogs | `Modal` with a `footer`; `modalOnly` when a stray click could lose typed work |
| Destructive or irreversible actions | `await confirm({ title, description, destructive: true })` |
| Success feedback | `useAction().run(fn, { success: "Saved" })` (a toast), never an inline "Saved" note |
| Errors | inline `ErrorNote` next to the form; `toastError` for actions with no form |
| Loading | `Skeleton` shaped like the content; never a blank screen |
| Nothing yet | `EmptyState` with an icon, one sentence, and the action that fixes it |
| Pages | `Page` + `PageHeader` (title, description, actions, breadcrumbs) |
| Menus, tooltips, tabs | `Menu…`, `Tooltip`, `Tabs` (in-page) / `LinkTabs` (routed) |
| Relative times | `<Time iso=…>` (shows the exact time on hover) |

Icons are `lucide-react` at 16px (14px in small buttons and badges), colored `faint`/`muted` unless they
carry state.

## Voice

Calm and observational. Say what happened and what the person can do next.

- Name things the way people see them: "agent", "sandbox", "workstream", "credential". Not "session row" or "ACP".
- Buttons say exactly what they do: "Start agent", "Remove", "Revoke key". The toast confirms in past tense: "Key revoked".
- Errors explain the cause and the fix, without apology: "Sandbox launch failed: the runner refused the pod. Check its logs, then try again."
- Confirmations state the consequence: "Its sandbox shuts down and any unpushed work in it is lost."
- No exclamation marks, no emoji.

## Layout and motion

- Sidebar navigation on desktop; a slide-in sheet under 768px. Pages cap at 1180px (1480px for the Board).
- Cards are for objects (a workstream, an integration). Lists of rows sit in one card with dividers.
- Motion is short (140–160ms) and functional: dialogs fade and lift in, the running dot pulses. All of it
  respects `prefers-reduced-motion`.
- Keyboard: `C` opens New work anywhere; `⌘/Ctrl+Enter` submits multi-line forms; `Enter` sends a message.
